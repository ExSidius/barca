# Getting Started

## Prerequisites

- Python 3.13+ (free-threaded 3.13t recommended)
- [uv](https://docs.astral.sh/uv/) package manager

## Installation

```bash
uv init --app my-project
cd my-project
uv add barca
```

This installs the `@asset()` decorator and the `barca` CLI into your project's virtualenv.

## Your First Asset

Create `assets.py`:

```python
from barca import asset

@asset()
def hello() -> dict:
    return {"message": "Hello from barca!"}
```

## Index and Materialize

```bash
uv run barca reindex             # discover decorated functions
uv run barca assets list         # see what was found
uv run barca assets refresh 1    # materialize the asset
uv run barca assets show 1       # view the result
```

No config file needed. Barca scans your project for files that import from `barca` and discovers decorated functions automatically.

## Adding Dependencies

Assets can declare upstream inputs:

```python
from barca import asset

@asset()
def raw_data() -> list[dict]:
    return [{"x": 1}, {"x": 2}, {"x": 3}]

@asset(inputs={"data": raw_data})
def summary(data: list[dict]) -> dict:
    return {"count": len(data), "sum": sum(d["x"] for d in data)}
```

When you refresh `summary`, Barca automatically materializes `raw_data` first and passes its output as the `data` kwarg.

## Adding Sensors

Sensors observe external state:

```python
from barca import sensor, cron

@sensor(schedule=cron("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    files = list(Path("inbox").glob("*.csv"))
    return len(files) > 0, [str(f) for f in files]
```

Sensors return `(update_detected: bool, output)` tuples. During reconciliation, downstream assets only run when `update_detected` is `True`.

## Adding Effects

Effects are leaf nodes that produce side effects:

```python
from barca import asset, effect

@effect(inputs={"data": summary}, schedule="always")
def publish(data: dict) -> None:
    print(f"Publishing: {data}")
```

Effects cannot be used as inputs to other nodes.

## Partitioned Assets

Fan out into parallel jobs:

```python
from barca import asset, partitions

@asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
def prices(ticker: str) -> dict:
    return {"ticker": ticker, "price": len(ticker) * 100}
```

```bash
uv run barca assets refresh 1 -j 4    # 4 parallel workers
```

## Reconciliation

Run all scheduled nodes in dependency order:

```bash
uv run barca reconcile                 # single pass
uv run barca reconcile --watch         # continuous loop
uv run barca reconcile --watch --interval 30
```

## HTTP Server

Start the optional HTTP API with a background scheduler:

```bash
uv run barca serve
uv run barca serve --port 8400 --interval 60
```

See the [Server API reference](server-api.md) for all endpoints.

## Notebook Usage

Use Barca interactively from Jupyter:

```python
from barca import load_inputs, materialize, read_asset, list_versions

# Materialize with caching
data = materialize(my_asset)

# Load inputs for a function, call it as plain Python
kwargs = load_inputs(downstream)
result = downstream(**kwargs)

# Read latest artifact without re-materializing
value = read_asset(my_asset)

# View materialization history
versions = list_versions(my_asset)
```

## Schedule Types

| Schedule | Behavior |
|----------|----------|
| `"manual"` | Only runs via explicit `barca assets refresh` |
| `"always"` | Runs whenever stale + upstream ready (during reconcile) |
| `cron("0 5 * * *")` | Runs when a cron tick has occurred since last run |

## Next Steps

- [Architecture](architecture.md) — how Barca works internally
- [Core Constraints](core-constraints.md) — deliberate design boundaries
- [API Reference](api/decorators.md) — full decorator documentation
- [Workflow Specs](workflows/01-single-asset-no-inputs.md) — detailed behavior specifications
