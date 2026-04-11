# Getting Started

## Prerequisites

- Python 3.14+ (free-threaded 3.14t recommended)
- [uv](https://docs.astral.sh/uv/) package manager

## Installation

```bash
uv init --app my-project
cd my-project
echo "3.14t" > .python-version
echo "PYTHON_GIL=0" > .env
uv python install 3.14t
uv add barca
```

The `.python-version` file pins the project to free-threaded Python. `PYTHON_GIL=0` ensures the GIL stays disabled even when C extensions (such as the turso DB driver) haven't yet declared GIL safety — without it, those extensions silently re-enable the GIL at import time and barca will warn you. Pass `--env-file .env` to `uv run` or export `PYTHON_GIL=0` in your shell to apply it automatically.

This installs the `@asset()` decorator and the `barca` CLI into your project's virtualenv.

## Your First Asset

Create `assets.py`:

```python
from barca import asset

@asset()
def hello() -> dict:
    return {"message": "Hello from barca!"}
```

The default freshness is `Always` — Barca keeps this asset up to date automatically during `barca run`.

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

Sensors observe external state. They must use `Manual` or `Schedule` freshness — `Always` is not valid for sensors.

```python
from barca import sensor, Schedule

@sensor(freshness=Schedule("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    files = list(Path("inbox").glob("*.csv"))
    return len(files) > 0, [str(f) for f in files]
```

Sensors return `(update_detected: bool, output)` tuples. The full tuple is passed as input to downstream assets. Downstream assets only become stale when `update_detected` is `True`.

## Adding Effects

Effects are standalone side-effect functions — sending email, writing to a database, calling an external API. They are leaf nodes and cannot be used as inputs to other nodes.

```python
from barca import asset, effect, Always

@effect(inputs={"data": summary}, freshness=Always)
def publish(data: dict) -> None:
    print(f"Publishing: {data}")
```

## Writing Outputs to Files

Use `@sink` stacked on `@asset` to write an asset's output to a path when it materialises. Paths are fsspec-compatible (local, `s3://`, `gs://`, etc.).

```python
from barca import asset, sink, Always, JsonSerializer

@asset(freshness=Always)
@sink('./output.json', serializer=JsonSerializer)
@sink('s3://my-bucket/output.json', serializer=JsonSerializer)
def report() -> dict:
    return {"rows": 42}
```

A sink failure does not fail the parent asset, but surfaces prominently in `assets list` and job logs.

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

For dynamically-partitioned assets (where the partition set comes from an upstream asset), the partition set is resolved lazily at run time. Until the partition-defining asset has materialised, `assets list` shows the partition set as "pending".

## Development Mode

Use `barca dev` during development to watch for file changes and see live staleness state in the UI, without triggering any materialization.

```bash
uv run barca dev
```

Use `barca assets refresh` to materialize individual assets during development.

## Production Mode

Use `barca run` in production to continuously maintain the DAG at each asset's declared freshness level.

```bash
uv run barca run
```

## HTTP Server

Start the optional HTTP API with a background scheduler (uses the same semantics as `barca run`):

```bash
uv run barca serve
uv run barca serve --port 8400
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

## Freshness Types

| Freshness | Behavior |
|-----------|----------|
| `Always` (default) | Auto-materialises whenever stale and upstreams are ready during `barca run` |
| `Manual` | Only runs via explicit `barca assets refresh`; blocks downstream `Always` assets from auto-updating |
| `Schedule("0 5 * * *")` | Runs when a cron tick has elapsed since last run |

## Next Steps

- [Architecture](architecture.md) — how Barca works internally
- [Core Constraints](core-constraints.md) — deliberate design boundaries
- [API Reference](api/decorators.md) — full decorator documentation
- [Workflow Specs](workflows/01-single-asset-no-inputs.md) — detailed behavior specifications
