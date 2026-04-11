# Barca

A modern, minimal asset orchestrator.

Barca is a pure Python asset orchestrator that discovers functions decorated with `@asset()`, `@sensor()`, `@effect()`, and `@sink()`, materializes them on demand or at their declared freshness level, and stores artifacts with content-addressed caching.

`freshness` is the core primitive — it declares how eagerly Barca keeps each asset's output up to date.

```python
from barca import asset, sensor, effect, sink, Schedule, Always, Manual, partitions, JsonSerializer

@sensor(freshness=Schedule("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    return True, ["inbox/a.csv", "inbox/b.csv"]

@asset(inputs={"paths": inbox_files})
def parse_inbox(paths: tuple[bool, list[str]]) -> list[dict]:
    _, files = paths
    return [{"file": f, "rows": 100} for f in files]

@asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
def prices(ticker: str) -> dict:
    return {"ticker": ticker, "price": len(ticker) * 100}

@asset(freshness=Always)
@sink('./report.json', serializer=JsonSerializer)
def report() -> dict:
    return {"status": "ok"}

@effect(inputs={"rows": parse_inbox})
def publish_rows(rows: list[dict]) -> None:
    pass  # push to external system
```

## Install

```bash
uv add barca
```

## Quick Start

```bash
uv init --app my-project
cd my-project
uv add barca
```

Write your first asset:

```python
# assets.py
from barca import asset

@asset()
def hello() -> dict:
    return {"message": "Hello from barca!"}
```

Run:

```bash
uv run barca reindex             # discover @asset() functions
uv run barca assets list         # list all indexed assets
uv run barca assets refresh 1    # materialize an asset
uv run barca dev                 # development mode — file watcher, live staleness
uv run barca run                 # production mode — continuously maintains freshness
uv run barca serve               # HTTP API + background scheduler
```

## Notebook Usage

```python
from barca import load_inputs, materialize, read_asset

# Materialize an asset (with caching) and get its value
data = materialize(my_asset)

# Load upstream inputs, then call the function as plain Python
kwargs = load_inputs(downstream)
result = downstream(**kwargs)

# Read the latest artifact without re-materializing
value = read_asset(my_asset)
```

## Architecture

Three packages, one uv workspace:

| Package | Path | Purpose |
|---------|------|---------|
| `barca` | `packages/barca-core/` | Core library — decorators, models, store, engine, hashing, tracing |
| `barca-cli` | `packages/barca-cli/` | CLI tool — typer app, table formatting |
| `barca-server` | `packages/barca-server/` | HTTP API + background scheduler (optional) |

## Node Kinds

| Kind | Decorator | Freshness default | Cached | Can be input |
|------|-----------|------------------|--------|-------------|
| **asset** | `@asset()` | `Always` | Yes (by `run_hash`) | Yes |
| **sensor** | `@sensor()` | `Manual` | No (always re-runs) | Yes |
| **effect** | `@effect()` | `Always` | No (always re-runs) | No (leaf node) |
| **sink** | `@sink(path, serializer=)` | — (attached to asset) | No | No (leaf node) |

## Two Main Modes

- **`barca dev`** — file watcher and live staleness in the UI; does not materialise anything. For development.
- **`barca run`** — continuously maintains the DAG at each asset's declared freshness level. For production.

## Next Steps

- [Getting Started](getting-started.md) — full walkthrough
- [Architecture](architecture.md) — design details
- [API Reference](api/decorators.md) — decorator and helper docs
- [CLI Reference](cli.md) — all CLI commands
- [Workflow Specs](workflows/01-single-asset-no-inputs.md) — detailed behavior specs
