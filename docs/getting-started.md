# Getting Started

## Prerequisites

- Python >= 3.12
- [uv](https://docs.astral.sh/uv/) package manager

## Installation

```bash
uv init --app my-project
cd my-project
uv add barca
```

This installs the `@asset()` decorator and the `barca` CLI into your project's virtualenv.

## 1. Your first asset

Create `pipeline.py`:

```python
from barca import asset

@asset()
def hello() -> dict:
    return {"message": "Hello from barca!"}
```

Run it:

```bash
uv run barca get pipeline.py
```

You'll see structured JSON on stdout and timing on stderr:

```
[barca] 1 nodes, 0 edges, 1 phases, 1 streams | plan: 0.8ms | exec: 37ms | total: 38ms
```

The `@asset()` decorator itself does nothing at runtime -- it's an identity function. Your code runs exactly the same with or without barca installed.

## 2. Two assets with a dependency

Assets can depend on other assets via `inputs=`. Barca resolves the DAG and executes them in the right order.

```python
from barca import asset

@asset()
def raw_data() -> list[dict]:
    return [{"x": 1}, {"x": 2}, {"x": 3}]

@asset(inputs={"data": raw_data})
def summary(data: list[dict]) -> dict:
    return {"count": len(data), "sum": sum(d["x"] for d in data)}
```

```bash
uv run barca get pipeline.py
```

Barca sees that `summary` depends on `raw_data`, creates two phases, executes `raw_data` first, then passes its output to `summary` as the `data` kwarg.

## 3. Inspect the plan

You can see what barca will do without executing anything:

```bash
uv run barca plan pipeline.py
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

The plan shows phases (sequential groups), streams (parallel workers within a phase), and steps (individual functions within a stream).

## 4. Add a task

Tasks are for side-effects -- deploying, notifying, writing to external systems. They always re-run and are never cached.

```python
from barca import asset, task

@asset()
def report() -> dict:
    return {"rows": 42, "status": "ok"}

@task(inputs={"data": report})
def send_notification(data: dict) -> None:
    print(f"Report ready: {data['rows']} rows, status={data['status']}")
```

Use `barca run` to execute tasks (and their upstream assets):

```bash
uv run barca run pipeline.py
```

`barca get` materializes assets only. `barca run` also fires tasks.

## 5. Parallel execution

When assets are independent (no edges between them), barca runs them in parallel as separate worker streams.

```python
from barca import asset

@asset()
def users() -> list[dict]:
    return [{"id": 1, "name": "Alice"}, {"id": 2, "name": "Bob"}]

@asset()
def products() -> list[dict]:
    return [{"id": 1, "name": "Widget"}, {"id": 2, "name": "Gadget"}]

@asset(inputs={"users": users, "products": products})
def catalog(users: list[dict], products: list[dict]) -> dict:
    return {"total_users": len(users), "total_products": len(products)}
```

The plan will look like:

```
Phase 1 (Independent): users, products    <- 2 parallel streams
Phase 2 (Dependent):   catalog            <- waits for both
```

Barca spawns up to `available_parallelism()` workers per phase. On an 8-core machine, both source assets run concurrently.

## 6. Parallel dispatch within a task

Use `parallel()` and `parallel_map()` to fan out work inside a task:

```python
from functools import partial
from barca import task, parallel, parallel_map

@task()
def fetch_page(url: str) -> dict:
    return {"url": url, "status": 200}

@task()
def fetch_all() -> list:
    urls = ["https://example.com/1", "https://example.com/2", "https://example.com/3"]
    results = parallel_map(fetch_page, urls)
    return results
```

When running inside a barca worker, `parallel()` dispatches branches to separate Rust-managed worker processes. When running standalone, it executes sequentially.

## 7. Partitioned assets

Fan out a single asset definition into N independent runs, one per partition key:

```python
from barca import asset, partitions, collect

@asset(partitions={"region": partitions(["us-east", "us-west", "eu-west"])})
def regional_sales(region: str) -> dict:
    return {"region": region, "total": hash(region) % 10000}

@asset(inputs={"sales": collect(regional_sales)})
def global_summary(sales: dict) -> dict:
    total = sum(v["total"] for v in sales.values())
    return {"regions": len(sales), "global_total": total}
```

- `regional_sales` runs 3 times, once per region
- `collect(regional_sales)` aggregates all partition outputs into a single dict
- `global_summary` receives all three results at once

## 8. Cache behavior

Run the same pipeline twice:

```bash
uv run barca get pipeline.py   # first run: executes everything
uv run barca get pipeline.py   # second run: instant (cache hit)
```

Barca uses content-addressed hashing of function source and upstream outputs. If nothing changed, the second run returns cached results with zero re-execution.

Change the code and run again -- barca detects the source hash changed and re-executes only the affected nodes and their downstream dependents.

## Next steps

- [Guide](guide.md) -- full tutorial from first asset to sensors, partitions, and freshness
- [Patterns](patterns/02-asset-to-task.md) -- common patterns and anti-patterns
- [CLI Reference](cli.md) -- all CLI commands
