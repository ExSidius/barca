# Barca

A modern, minimal asset orchestrator.

Barca is a pure Python asset orchestrator that discovers functions decorated with `@asset()`, `@sensor()`, and `@effect()`, materializes them on demand or via schedule, and stores artifacts with content-addressed caching.

```python
from barca import asset, sensor, effect, cron, partitions

@sensor(schedule=cron("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    return True, ["inbox/a.csv", "inbox/b.csv"]

@asset(inputs={"paths": inbox_files}, schedule="always")
def parse_inbox(paths: list[str]) -> list[dict]:
    return [{"file": p, "rows": 100} for p in paths]

@asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
def prices(ticker: str) -> dict:
    return {"ticker": ticker, "price": len(ticker) * 100}

@effect(inputs={"rows": parse_inbox}, schedule="always")
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
uv run barca reconcile           # single-pass reconcile
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
| `barca` | `packages/barca-core/` | Core library — decorators, models, store, engine, hashing, tracing, reconciler |
| `barca-cli` | `packages/barca-cli/` | CLI tool — typer app, table formatting |
| `barca-server` | `packages/barca-server/` | HTTP API + background scheduler (optional) |

## Node Kinds

| Kind | Decorator | Schedule default | Cached | Can be input |
|------|-----------|-----------------|--------|-------------|
| **asset** | `@asset()` | `"manual"` | Yes (by `run_hash`) | Yes |
| **sensor** | `@sensor()` | `"always"` | No (always re-runs) | Yes |
| **effect** | `@effect()` | `"always"` | No (always re-runs) | No (leaf node) |

## Next Steps

- [Getting Started](getting-started.md) — full walkthrough
- [Architecture](architecture.md) — design details
- [API Reference](api/decorators.md) — decorator and helper docs
- [CLI Reference](cli.md) — all CLI commands
- [Workflow Specs](workflows/01-single-asset-no-inputs.md) — detailed behavior specs
