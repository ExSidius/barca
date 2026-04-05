<p align="center">
  <h1 align="center">barca</h1>
  <p align="center">A modern, minimal asset orchestrator.</p>
</p>

<p align="center">
  <a href="https://github.com/ExSidius/barca/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/ExSidius/barca/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://github.com/ExSidius/barca/releases"><img alt="Release" src="https://img.shields.io/github/v/release/ExSidius/barca?include_prereleases&label=release" /></a>
  <img alt="Python" src="https://img.shields.io/badge/python-%E2%89%A53.13-blue" />
  <img alt="License" src="https://img.shields.io/github/license/ExSidius/barca" />
</p>

---

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

```
$ uv run barca reindex
ID | Kind   | Name        | Module       | Function    | Schedule | Status
---+--------+-------------+--------------+-------------+----------+---------
1  | sensor | inbox_files | mod.pipeline | inbox_files | */5 ...  | never run
2  | asset  | parse_inbox | mod.pipeline | parse_inbox | always   | never run
3  | asset  | prices      | mod.pipeline | prices      | manual   | never run
4  | effect | publish     | mod.pipeline | publish     | always   | never run

$ uv run barca assets refresh 3 -j 4
Asset #3
  Name:   prices
  Status: #12 (success)
```

## Install

```bash
uv add barca
```

This installs the Python `@asset()` decorator and the `barca` CLI into your project's virtualenv.

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
uv run barca reconcile           # single-pass reconcile (all scheduled nodes)
uv run barca serve               # HTTP API + background scheduler
```

No config file needed. Barca scans your project for decorated functions automatically.

<details>
<summary>Prefer bare <code>barca</code> commands?</summary>

Activate your virtualenv first:

```bash
source .venv/bin/activate    # or: . .venv/bin/activate.fish
barca reindex
barca assets list
barca assets refresh 1
```
</details>

### CLI Reference

All commands below use `uv run` prefix. If you've activated your virtualenv, you can omit it.

```
uv run barca reindex                       Re-inspect Python modules
uv run barca assets list                   List all indexed assets
uv run barca assets show <id>              Show asset detail
uv run barca assets refresh <id>           Trigger materialization
uv run barca assets refresh <id> -j 4     Parallel partition workers
uv run barca sensors list                  List all sensors
uv run barca sensors show <id>             Show sensor detail + observation history
uv run barca sensors trigger <id>          Manually trigger a sensor
uv run barca jobs list                     List recent jobs
uv run barca jobs show <id>                Show job detail
uv run barca reconcile                     Single-pass reconcile
uv run barca reconcile --watch             Continuous reconcile loop
uv run barca serve                         HTTP API + background scheduler
uv run barca reset [--db] [--artifacts]    Clean generated files
```

## Features

**Implemented** (workflows 1–6):

- **Asset discovery** — decorate any Python function with `@asset()`, barca finds it
- **Dependency tracking** — declare upstream inputs with `@asset(inputs={"x": upstream})`, barca resolves the DAG, materializes upstreams first, and passes artifacts as kwargs
- **Partitioned assets** — `@asset(partitions={"key": partitions([...])})` fans out into N parallel jobs, one per partition value
- **Parallel execution** — partition batches use `ThreadPoolExecutor` with free-threaded Python (3.13t, GIL disabled) for true thread parallelism
- **Artifact versioning** — each materialization is keyed by a `definition_hash` (source + deps + dependency cone hash) and a `run_hash` (definition + upstream versions + partition key); identical runs are cached
- **Asset continuity** — rename or move assets while preserving lineage via `@asset(name="stable_name")`
- **Schedule-driven reconciliation** — `"manual"`, `"always"`, or `cron("0 5 * * *")` schedules with single-pass or continuous reconcile
- **Sensors** — `@sensor()` decorator for external state observation; returns `(update_detected, output)` tuples; dedicated CLI and API surface
- **Effects** — `@effect()` decorator for side-effect leaf nodes; execute after upstream assets
- **HTTP API** — FastAPI server with REST endpoints for assets, sensors, jobs, and reconciliation
- **CLI** — `barca` command for all operations (reads DB directly, no server needed)

**Planned** (workflows 7–9):

| Workflow | Description | Status |
|----------|-------------|--------|
| 7 | Notebook workflow (`load_inputs()`) | Spec ready |
| 8 | Backfill and replay | Spec ready |
| 9 | Execution controls (timeout, cancel, retry) | Spec ready |

## Architecture

```
┌─────────────────────────────────────────────────────┐
│               barca (Python, uv workspace)           │
│                                                      │
│  ┌──────────┐  ┌──────────────┐  ┌───────────────┐  │
│  │ barca-cli│  │ barca-server │  │  barca (core) │  │
│  │          │  │              │  │               │  │
│  │ typer    │  │ FastAPI      │  │ decorators    │  │
│  │ commands │  │ routes       │  │ models        │  │
│  │ display  │  │ scheduler    │  │ engine        │  │
│  │          │  │ service      │  │ reconciler    │  │
│  │          │  │              │  │ store (SQLite)│  │
│  │          │  │              │  │ hashing       │  │
│  │          │  │              │  │ tracing       │  │
│  └──────────┘  └──────────────┘  └───────────────┘  │
└─────────────────────────────────────────────────────┘
         │                              │
         ▼                              ▼
   .barca/metadata.db           .barcafiles/
   (SQLite — assets,            (versioned artifacts:
    definitions, jobs,           value.json, code.txt)
    observations)
```

**Three packages**, one uv workspace:

| Package | Path | Purpose |
|---------|------|---------|
| `barca` | `packages/barca-core/` | Core library — decorators, models, store, engine, hashing, tracing, reconciler |
| `barca-cli` | `packages/barca-cli/` | CLI tool — typer app, table formatting |
| `barca-server` | `packages/barca-server/` | HTTP API + background scheduler — FastAPI, uvicorn (optional) |

**Key design decisions:**
- Pure Python — no native extensions, no subprocess workers
- Materialize via `importlib.import_module()` + direct function call
- Free-threaded Python (3.13t) for true thread parallelism in partitions
- The CLI opens the database directly — no server needed
- The server is optional — adds HTTP API and background scheduler
- SQLite (stdlib) for metadata; optional Turso/libSQL for remote

### Node Kinds

| Kind | Decorator | Schedule default | Cached | Can be input |
|------|-----------|-----------------|--------|-------------|
| **asset** | `@asset()` | `"manual"` | Yes (by `run_hash`) | Yes |
| **sensor** | `@sensor()` | `"always"` | No (always re-runs) | Yes |
| **effect** | `@effect()` | `"always"` | No (always re-runs) | No (leaf node) |

### Asset Lifecycle

1. **Index** — `reindex()` imports Python modules, finds decorated functions, computes `definition_hash` (SHA-256 of source + metadata + dependency cone hash + protocol version), upserts into DB
2. **Refresh** — recursively materializes upstream deps, calls the function, saves result as JSON to `.barcafiles/{slug}/{definition_hash}/value.json`, records success
3. **Reconcile** — walks the entire DAG in topo order, checks staleness and schedule eligibility, executes sensors/assets/effects as needed
4. **Cache** — if `run_hash` matches an existing successful materialization, the asset is skipped

### Server API Endpoints

```
GET  /health                           → {"status": "ok", "scheduler_running": bool}
GET  /assets                           → [AssetSummary, ...]
GET  /assets/{id}                      → AssetDetail
POST /assets/{id}/refresh              → AssetDetail
GET  /sensors                          → [AssetSummary, ...]  (kind=sensor only)
GET  /sensors/{id}/observations        → [SensorObservation, ...]
POST /sensors/{id}/trigger             → SensorObservation
POST /reconcile                        → ReconcileResult
GET  /jobs                             → [JobDetail, ...]
GET  /jobs/{id}                        → JobDetail
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Core | Python 3.13+ (free-threaded) |
| Models | [Pydantic](https://docs.pydantic.dev/) 2.0 |
| Database | SQLite (stdlib); optional [Turso/libSQL](https://turso.tech/) |
| CLI | [Typer](https://typer.tiangolo.com/) 0.9 |
| Server | [FastAPI](https://fastapi.tiangolo.com/) 0.115 + [uvicorn](https://www.uvicorn.org/) |
| Scheduling | [croniter](https://github.com/kiorky/croniter) 2.0 |
| Python env | [uv](https://docs.astral.sh/uv/) |

## Testing

```bash
uv run pytest tests/ -v
```

| Suite | Tests | What it covers |
|-------|-------|---------------|
| `test_basic` | 5 | Reindex, refresh, caching, reset, idempotency |
| `test_deps` | 5 | Dependency resolution, upstream triggers, artifact passing |
| `test_partitions` | 6 | Partition fan-out, parallel execution, caching |
| `test_reconciler` | 4 | Always/manual schedules, sensor→asset→effect pipeline |
| `test_sensor` | 10 | Decorator, discovery, observations, trigger, guards |
| `test_effect` | 3 | Decorator, leaf-node validation, upstream execution |
| `test_schedule` | 6 | Cron, manual, always schedules |
| `test_trace` | 5 | AST dependency tracing, cross-file deps, purity analysis |
| `test_codebase_hash` | 3 | Hash stability and invalidation |
| `test_server` | 14 | All HTTP endpoints, sensor e2e, reconcile integration |

## Development

```bash
git clone https://github.com/ExSidius/barca.git
cd barca
uv sync                              # install all workspace packages
uv run pytest tests/ -v              # run tests
cd examples/basic_app && uv sync     # run an example
uv run barca reindex
uv run barca assets refresh 1
```

## Docs

- [Architecture](./docs/architecture.md)
- [Core constraints](./docs/core-constraints.md)
- [Development](./docs/development.md)
- [Testing](./docs/testing.md)

### Workflow specs

| # | Workflow | Status |
|---|---------|--------|
| 1 | [Single asset, no inputs](./docs/workflows/01-single-asset-no-inputs.md) | Done |
| 2 | [Single asset with one input](./docs/workflows/02-single-asset-one-input.md) | Done |
| 3 | [Parametrized assets and partitions](./docs/workflows/03-parametrized-assets-and-partitions.md) | Done |
| 4 | [Asset continuity (rename/move)](./docs/workflows/04-asset-continuity-rename-and-move.md) | Done |
| 5 | [Schedule-driven reconciliation](./docs/workflows/05-schedule-driven-reconciliation-and-effects.md) | Done |
| 6 | [Sensors and external observations](./docs/workflows/06-sensors-and-external-observations.md) | Done |
| 7 | [Notebook workflow](./docs/workflows/07-notebook-workflow.md) | Planned |
| 8 | [Backfill and replay](./docs/workflows/08-backfill-and-replay.md) | Planned |
| 9 | [Execution controls](./docs/workflows/09-execution-controls-and-ad-hoc-params.md) | Planned |
