<p align="center">
  <h1 align="center">barca</h1>
  <p align="center">A modern, minimal asset orchestrator.</p>
</p>

<p align="center">
  <a href="https://github.com/ExSidius/barca/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/ExSidius/barca/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://github.com/ExSidius/barca/releases"><img alt="Release" src="https://img.shields.io/github/v/release/ExSidius/barca?include_prereleases&label=release" /></a>
  <img alt="Python" src="https://img.shields.io/badge/python-%E2%89%A53.14-blue" />
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

### Install from GitHub

To install the latest development version directly from the repository:

```bash
uv add "barca @ git+https://github.com/ExSidius/barca.git#subdirectory=packages/barca"
```

Or with pip:

```bash
pip install "barca @ git+https://github.com/ExSidius/barca.git#subdirectory=packages/barca"
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
uv run barca reconcile           # single-pass reconcile (all scheduled nodes)
uv run barca serve               # HTTP API + background scheduler
```

No config file needed. Barca scans your project for decorated functions automatically.

### Notebook Usage

```python
from barca import load_inputs, materialize, read_asset

# Materialize an asset (with caching) and get its value
data = materialize(my_asset)

# Load upstream inputs for a function, then call it as plain Python
kwargs = load_inputs(downstream)
result = downstream(**kwargs)

# Read the latest artifact without re-materializing
value = read_asset(my_asset)
```

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

## Learning Path

Work through these in order to get familiar with barca progressively. Each section has a minimal example; click the link for the full specification and edge-case details.

### 1. Single asset — index, materialize, cache &nbsp;[→ full spec](./docs/workflows/01-single-asset-no-inputs.md)

The simplest possible node. Barca indexes it, materializes it once, then reuses the cached artifact on every subsequent run until the code changes.

```python
from barca import asset

@asset()
def hello() -> dict:
    return {"message": "Hello from barca!"}
```

```bash
uv run barca reindex             # discover @asset() functions
uv run barca assets refresh 1   # materialize → .barcafiles/.../value.json
uv run barca assets refresh 1   # instant: cache hit, no re-execution
```

---

### 2. Asset dependencies — declare inputs, barca resolves the DAG &nbsp;[→ full spec](./docs/workflows/02-single-asset-one-input.md)

Pass upstream functions via `inputs=`. Barca materializes them first and injects their outputs as kwargs. The downstream `run_hash` covers upstream version, so any upstream change cascades automatically.

```python
@asset()
def raw_data() -> list[dict]:
    return [{"x": 1}, {"x": 2}, {"x": 3}]

@asset(inputs={"data": raw_data})
def summary(data: list[dict]) -> dict:
    return {"count": len(data), "total": sum(d["x"] for d in data)}
```

```bash
uv run barca assets refresh 2   # materializes raw_data first, then summary
```

---

### 3. Partitioned assets — fan-out with parallel workers &nbsp;[→ full spec](./docs/workflows/03-parametrized-assets-and-partitions.md)

`partitions()` turns one asset definition into N independent materializations, each cached separately. Use `-j` to run them in parallel with free-threaded Python.

```python
from barca import asset, partitions

@asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
def prices(ticker: str) -> dict:
    return {"ticker": ticker, "price": len(ticker) * 100}
```

```bash
uv run barca assets refresh 1 -j 4   # 4 parallel workers
```

---

### 4. Asset continuity — preserve history across renames &nbsp;[→ full spec](./docs/workflows/04-asset-continuity-rename-and-move.md)

Without an explicit `name=`, barca keys identity to `file_path:function_name`. Renaming or moving the function breaks the link. Use `name=` to pin a stable key so lineage and cached artifacts survive refactors.

```python
# Before rename: continuity_key = "my_project/etl.py:load_raw"
@asset()
def load_raw() -> list[dict]: ...

# After rename: continuity_key unchanged — history preserved
@asset(name="load_raw")
def ingest_raw_data() -> list[dict]: ...
```

---

### 5. Schedules, reconciliation, and effects &nbsp;[→ full spec](./docs/workflows/05-schedule-driven-reconciliation-and-effects.md)

Assets default to `"manual"`. Set `schedule="always"` or a cron expression to make nodes run automatically during `barca reconcile`. `@effect()` is a side-effect leaf node (write to DB, send email) that is never cached and can't be used as an input.

```python
from barca import asset, effect, cron

@asset(schedule=cron("0 5 * * *"))   # refresh daily at 5 AM
def daily_report() -> dict:
    return {"rows": 42}

@effect(inputs={"report": daily_report}, schedule="always")
def publish(report: dict) -> None:
    print(f"Publishing {report}")    # push to Slack, S3, etc.
```

```bash
uv run barca reconcile          # single pass — runs eligible stale nodes
uv run barca reconcile --watch  # continuous loop
```

---

### 6. Sensors — bring external state into the DAG &nbsp;[→ full spec](./docs/workflows/06-sensors-and-external-observations.md)

Sensors are ingress nodes that poll external systems. They return `(update_detected: bool, output)`. Downstream assets only re-run when `update_detected=True`. Each execution is recorded as an observation (not a cached artifact).

```python
from barca import sensor, asset

@sensor(schedule="always")
def inbox_files() -> tuple[bool, list[str]]:
    files = list(Path("inbox").glob("*.csv"))
    return bool(files), [str(f) for f in files]

@asset(inputs={"paths": inbox_files}, schedule="always")
def parse_inbox(paths: list[str]) -> list[dict]:
    return [{"file": p, "rows": 0} for p in paths]
```

---

### 7. Notebook workflow — call assets as plain Python &nbsp;[→ full spec](./docs/workflows/07-notebook-workflow.md)

`load_inputs()` resolves upstream materializations and returns them as a plain `dict`, so you can call any asset function directly in a notebook or REPL without re-running the full pipeline.

```python
from barca import load_inputs, materialize, read_asset, list_versions

# resolve and inject upstream artifacts
kwargs = load_inputs(parse_inbox)
result = parse_inbox(**kwargs)

# or let barca materialize and cache in one call
data = materialize(parse_inbox)

# read the latest artifact without re-executing
value = read_asset(daily_report)
```

---

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
uv run barca reset [--db] [--artifacts] [--tmp]    Clean generated files
```

## Features

**Implemented** (workflows 1–7):

- **Asset discovery** — decorate any Python function with `@asset()`, barca finds it
- **Dependency tracking** — declare upstream inputs with `@asset(inputs={"x": upstream})`, barca resolves the DAG, materializes upstreams first, and passes artifacts as kwargs
- **Partitioned assets** — `@asset(partitions={"key": partitions([...])})` fans out into N parallel jobs, one per partition value
- **Parallel execution** — partition batches use `ThreadPoolExecutor` with free-threaded Python (3.14t, GIL disabled) for true thread parallelism
- **Artifact versioning** — each materialization is keyed by a `definition_hash` (source + deps + dependency cone hash) and a `run_hash` (definition + upstream versions + partition key); identical runs are cached
- **Asset continuity** — rename or move assets while preserving lineage via `@asset(name="stable_name")`
- **Schedule-driven reconciliation** — `"manual"`, `"always"`, or `cron("0 5 * * *")` schedules with single-pass or continuous reconcile
- **Sensors** — `@sensor()` decorator for external state observation; returns `(update_detected, output)` tuples; dedicated CLI and API surface
- **Effects** — `@effect()` decorator for side-effect leaf nodes; execute after upstream assets
- **HTTP API** — FastAPI server with REST endpoints for assets, sensors, jobs, and reconciliation
- **CLI** — `barca` command for all operations (reads DB directly, no server needed)
- **Notebook helpers** — `load_inputs()`, `materialize()`, `read_asset()`, `list_versions()` for interactive notebook iteration without recomputation
- **Escape hatch** — `@unsafe` decorator skips AST dependency tracing for functions that cannot be statically analyzed

**Planned** (workflows 8–9):

| Workflow | Description | Status |
|----------|-------------|--------|
| 8 | Backfill and replay | Spec ready |
| 9 | Execution controls (timeout, cancel, retry) | Spec ready |

## Architecture

```
┌──────────────────────────────────────────────────┐
│            barca (Python, uv workspace)           │
│                                                   │
│  ┌────────────────────────────────────────────┐   │
│  │                  barca                     │   │
│  │                                            │   │
│  │  decorators · models · engine · reconciler │   │
│  │  store (SQLite/Turso) · hashing · tracing  │   │
│  │                                            │   │
│  │  barca.cli    — typer commands, display    │   │
│  │  barca.server — FastAPI, scheduler         │   │
│  └────────────────────────────────────────────┘   │
└──────────────────────────────────────────────────┘
         │                       │
         ▼                       ▼
   .barca/metadata.db      .barcafiles/
   (SQLite — assets,       (versioned artifacts:
    definitions, jobs,      value.json, code.txt)
    observations)
```

**One package** at `packages/barca/`, with an optional `[server]` extra:

| Import path | Purpose |
|-------------|---------|
| `barca` | Public API — `@asset`, `@sensor`, `@effect`, `cron`, `partitions`, notebook helpers |
| `barca.cli` | CLI entry point — `barca` command, table formatting |
| `barca.server` | HTTP API + background scheduler |

**Key design decisions:**
- Pure Python — no native extensions, no subprocess workers
- Materialize via `importlib.import_module()` + direct function call
- Free-threaded Python (3.14t) for true thread parallelism in partitions
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
| Core | Python 3.14+ (free-threaded) |
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
| `test_notebook` | 14 | Notebook helpers: materialize, load_inputs, read_asset, list_versions |
| `test_server` | 14 | All HTTP endpoints, sensor e2e, reconcile integration |

## Development

```bash
git clone https://github.com/ExSidius/barca.git
cd barca
uv sync                              # install the package + dev deps
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

See the [Learning Path](#learning-path) above for annotated examples. The full specifications (edge cases, DB schema, acceptance criteria) live in `docs/workflows/`:

| # | Workflow | Spec |
|---|---------|------|
| 1 | Single asset, no inputs | [01-single-asset-no-inputs.md](./docs/workflows/01-single-asset-no-inputs.md) |
| 2 | Single asset with one input | [02-single-asset-one-input.md](./docs/workflows/02-single-asset-one-input.md) |
| 3 | Parametrized assets and partitions | [03-parametrized-assets-and-partitions.md](./docs/workflows/03-parametrized-assets-and-partitions.md) |
| 4 | Asset continuity (rename/move) | [04-asset-continuity-rename-and-move.md](./docs/workflows/04-asset-continuity-rename-and-move.md) |
| 5 | Schedule-driven reconciliation and effects | [05-schedule-driven-reconciliation-and-effects.md](./docs/workflows/05-schedule-driven-reconciliation-and-effects.md) |
| 6 | Sensors and external observations | [06-sensors-and-external-observations.md](./docs/workflows/06-sensors-and-external-observations.md) |
| 7 | Notebook workflow | [07-notebook-workflow.md](./docs/workflows/07-notebook-workflow.md) |
| 8 | Backfill and replay *(planned)* | [08-backfill-and-replay.md](./docs/workflows/08-backfill-and-replay.md) |
| 9 | Execution controls *(planned)* | [09-execution-controls-and-ad-hoc-params.md](./docs/workflows/09-execution-controls-and-ad-hoc-params.md) |
