# Barca guide

This guide walks you through building a real pipeline with barca, from a single function to a multi-stage DAG with sensors, effects, and partitions.

## Prerequisites

- Python >= 3.12
- barca installed (`pip install barca`)

Verify it works:

```bash
barca --help
```

## 1. Your first asset

An asset is a Python function that returns data. That's it.

Create a file called `pipeline.py`:

```python
from barca import asset

@asset()
def greeting() -> dict:
    return {"message": "Hello from barca!"}
```

Run it:

```bash
barca get pipeline.py
```

You'll see output like:

```json
{"elapsed_seconds":0.039,"steps_executed":1,"phases":1,"final_output":{"message":"Hello from barca!"}}
```

And on stderr, the timing breakdown:

```
[barca] 1 nodes, 0 edges, 1 phases, 1 streams | plan: 0.8ms | exec: 37ms | total: 38ms
```

**What just happened?**

1. The Rust binary parsed `pipeline.py` using ruff's AST parser (no import, pure text analysis)
2. Found one `@asset()` decorator, extracted the function name and metadata
3. Built a trivial DAG (one node, no edges)
4. Generated an execution plan with one phase
5. Spawned a Python worker, which imported your module and called `greeting()`
6. Collected the return value and persisted it to `.barca/metadata.db`

The `@asset()` decorator itself does nothing at runtime -- it's an identity function. Your code runs exactly the same with or without barca installed.

## 2. Dependencies between assets

Assets can depend on other assets via `inputs=`. Barca resolves the DAG and executes them in the right order.

```python
from barca import asset

@asset()
def raw_data() -> list[dict]:
    return [
        {"name": "Alice", "score": 92},
        {"name": "Bob", "score": 85},
        {"name": "Carol", "score": 97},
    ]

@asset(inputs={"data": raw_data})
def summary(data: list[dict]) -> dict:
    scores = [d["score"] for d in data]
    return {
        "count": len(scores),
        "mean": sum(scores) / len(scores),
        "top": max(data, key=lambda d: d["score"])["name"],
    }
```

```bash
barca get pipeline.py
```

Barca sees that `summary` depends on `raw_data`, so it:

1. Creates two phases
2. Phase 1: executes `raw_data` in a worker
3. Passes `raw_data`'s output to phase 2 as a provided input
4. Phase 2: executes `summary` with the data injected as the `data` kwarg

You can inspect the plan without running anything:

```bash
barca plan pipeline.py
```

```json
{
  "total_steps": 2,
  "phases": [
    {
      "reason": "Independent",
      "streams": [{"stream_id": 0, "steps": ["raw_data"]}]
    },
    {
      "reason": "Dependent",
      "streams": [{"stream_id": 1, "steps": ["summary"]}]
    }
  ]
}
```

## 3. Parallel execution

When assets are independent (no edges between them), barca runs them in parallel as separate worker streams within the same phase.

```python
from barca import asset

@asset()
def users() -> list[dict]:
    return [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]

@asset()
def products() -> list[dict]:
    return [{"id": 1, "name": "Widget"}, {"id": 2, "name": "Gadget"}]

@asset()
def orders() -> list[dict]:
    return [{"user_id": 1, "product_id": 2, "qty": 3}]

@asset(inputs={"users": users, "products": products, "orders": orders})
def report(users: list[dict], products: list[dict], orders: list[dict]) -> dict:
    return {
        "total_users": len(users),
        "total_products": len(products),
        "total_orders": len(orders),
    }
```

The plan will look like:

```
Phase 1 (Independent): users, products, orders  ← 3 parallel streams
Phase 2 (Dependent):   report                    ← waits for all 3
```

Barca spawns up to `available_parallelism()` workers per phase. On an 8-core machine, all three source assets run concurrently.

## 4. Diamond DAGs

Real pipelines aren't linear chains. They fork and join. Barca handles this naturally.

```python
from barca import asset

@asset()
def raw_sales() -> list[dict]:
    return [{"product": "A", "amount": 100}, {"product": "B", "amount": 200}]

@asset()
def raw_inventory() -> list[dict]:
    return [{"product": "A", "stock": 50}, {"product": "B", "stock": 10}]

@asset(inputs={"sales": raw_sales})
def clean_sales(sales: list[dict]) -> list[dict]:
    return [s for s in sales if s["amount"] > 0]

@asset(inputs={"inventory": raw_inventory})
def clean_inventory(inventory: list[dict]) -> list[dict]:
    return [i for i in inventory if i["stock"] > 0]

@asset(inputs={"sales": clean_sales, "inventory": clean_inventory})
def dashboard(sales: list[dict], inventory: list[dict]) -> dict:
    return {
        "total_revenue": sum(s["amount"] for s in sales),
        "low_stock": [i["product"] for i in inventory if i["stock"] < 20],
    }
```

The execution plan:

```
Phase 1: raw_sales, raw_inventory           ← parallel
Phase 2: clean_sales, clean_inventory       ← parallel (each depends on one source)
Phase 3: dashboard                          ← waits for both clean steps
```

## 5. Sensors

Sensors observe external state. They're source nodes in the DAG that return `(update_detected, output)`.

```python
from barca import asset, sensor

@sensor()
def check_inbox() -> tuple[bool, list[str]]:
    from pathlib import Path
    files = list(Path("inbox").glob("*.csv"))
    return bool(files), [str(f) for f in files]

@asset(inputs={"files": check_inbox})
def process_inbox(files: list[str]) -> dict:
    return {"processed": len(files), "files": files}
```

Sensors are never cached -- they always re-run. Downstream assets only execute when the sensor reports `True` as the first element.

## 6. Effects

Effects are leaf nodes that perform side effects. They can depend on assets but nothing can depend on them.

```python
from barca import asset, effect

@asset()
def daily_report() -> dict:
    return {"revenue": 42000, "orders": 150}

@effect(inputs={"report": daily_report})
def send_slack_notification(report: dict) -> None:
    # In production, this would call Slack's API
    print(f"Daily revenue: ${report['revenue']:,}")

@effect(inputs={"report": daily_report})
def write_to_s3(report: dict) -> None:
    # In production, this would upload to S3
    print(f"Uploading report with {report['orders']} orders")
```

Both effects run in the same phase (they're independent of each other) after `daily_report` completes.

## 7. Partitions

Partitions fan a single asset definition into N independent runs, one per partition key.

```python
from barca import asset, partitions, collect

@asset(partitions={"region": partitions(["us-east", "us-west", "eu-west"])})
def regional_sales(region: str) -> dict:
    # In production, this would query a database filtered by region
    return {"region": region, "total": hash(region) % 10000}

@asset(inputs={"sales": collect(regional_sales)})
def global_summary(sales: dict) -> dict:
    total = sum(v["total"] for v in sales.values())
    return {"regions": len(sales), "global_total": total}
```

- `regional_sales` runs 3 times, once per region
- `collect(regional_sales)` aggregates all partition outputs into a single dict
- `global_summary` receives all three results at once

## 8. Multi-file pipelines

Barca can parse multiple Python files. Assets can reference functions across files as long as all files are passed to the CLI.

```
my_project/
  sources.py      # @asset defs for raw data
  transforms.py   # @asset defs that depend on sources
  effects.py      # @effect defs
```

```bash
barca get my_project/sources.py my_project/transforms.py my_project/effects.py
```

Barca merges all discovered nodes into a single DAG and plans execution across the full graph.

## 9. Freshness markers

Control when assets should re-run:

```python
from barca import asset, Always, Manual, Schedule

@asset(freshness=Always())
def always_fresh() -> dict:
    """Re-runs on every reconcile cycle."""
    return {"ts": time.time()}

@asset(freshness=Manual())
def on_demand() -> dict:
    """Only runs when explicitly triggered."""
    return {"manual": True}

@asset(freshness=Schedule("0 5 * * *"))
def daily_at_5am() -> dict:
    """Eligible for execution at 5 AM daily."""
    return {"scheduled": True}
```

## 10. Inspecting plans

`barca plan` is your debugging tool. It shows you exactly what barca will do without executing anything.

```bash
# See the plan as formatted JSON
barca plan pipeline.py | python -m json.tool

# Count total steps
barca plan pipeline.py | python -c "import json,sys; print(json.load(sys.stdin)['total_steps'])"
```

The plan shows:
- **Phases**: groups of work that execute sequentially
- **Streams**: parallel workers within a phase
- **Steps**: individual asset functions within a stream
- **Reason**: why a phase boundary exists (`Independent` or `Dependent`)

## Putting it together

Here's a complete pipeline that uses everything:

```python
# pipeline.py
from barca import asset, sensor, effect, partitions, collect

# Sensor: poll for new data
@sensor()
def check_data_lake() -> tuple[bool, dict]:
    # Check if new parquet files landed
    return True, {"path": "s3://bucket/raw/", "files": 3}

# Source assets (parallel)
@asset(partitions={"table": partitions(["users", "events", "purchases"])})
def extract(table: str) -> dict:
    return {"table": table, "rows": 1000}

# Transform (runs per partition, inheriting from extract)
@asset(inputs={"raw": extract})
def transform(raw: dict) -> dict:
    return {"table": raw["table"], "clean_rows": raw["rows"] - 10}

# Aggregate all partitions
@asset(inputs={"tables": collect(transform)})
def merge(tables: dict) -> dict:
    total = sum(v["clean_rows"] for v in tables.values())
    return {"total_rows": total, "tables": len(tables)}

# Sensor-driven asset
@asset(inputs={"lake_status": check_data_lake, "data": merge})
def report(lake_status, data: dict) -> dict:
    _, status = lake_status
    return {"source": status["path"], **data}

# Side effects
@effect(inputs={"report": report})
def notify(report: dict) -> None:
    print(f"Pipeline complete: {report['total_rows']} rows from {report['tables']} tables")
```

```bash
barca plan pipeline.py   # inspect the execution plan
barca get pipeline.py    # run it
```

## Tips

- **Decorators are no-ops.** Your code works without barca installed. `from barca import asset` imports an identity function. This means you can unit test your functions normally.

- **Outputs must be JSON-serializable.** Barca serializes return values to pass between phases and persist to the database. Stick to dicts, lists, strings, numbers, and booleans.

- **Use `barca plan` liberally.** It's free (no execution) and shows you exactly how barca decomposes your DAG.

- **Check `.barca/metadata.db`.** It's a SQLite database. You can query it directly:
  ```bash
  sqlite3 .barca/metadata.db "SELECT node_id, status, created_at FROM materializations ORDER BY created_at DESC LIMIT 10"
  ```

- **Stderr is for diagnostics.** Barca prints timing and topology info to stderr. Stdout is reserved for structured JSON output. Pipe stdout to `jq` for clean formatting:
  ```bash
  barca get pipeline.py 2>/dev/null | jq .
  ```
