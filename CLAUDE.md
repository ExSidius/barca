# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Install dependencies (uses free-threaded Python 3.13t by default via .python-version)
uv sync

# Run tests
uv run pytest tests/ -v

# CLI commands (from a project directory with barca.toml, or workspace root)
uv run barca reindex
uv run barca assets list
uv run barca assets show <id>
uv run barca assets refresh <id>         # default: -j cpu_count (parallel)
uv run barca assets refresh <id> -j 1   # sequential
uv run barca assets refresh <id> -j 64  # 64 threads
uv run barca sensors list                 # list all sensors
uv run barca sensors show <id>            # sensor detail + observation history
uv run barca sensors trigger <id>         # manually trigger a sensor
uv run barca reconcile                    # single-pass reconcile
uv run barca reconcile --watch            # continuous reconcile loop
uv run barca reconcile --watch --interval 30
uv run barca serve                        # HTTP API + background scheduler
uv run barca serve --port 8400 --interval 60 --log-level info
uv run barca jobs list
uv run barca jobs show <id>
uv run barca reset [--db] [--artifacts] [--tmp]

# Run from an example project
cd examples/basic_app && uv sync && uv run barca reindex
```

## Architecture

Barca is a minimal asset orchestrator: a pure Python uv workspace that discovers Python functions decorated with `@asset()`, `@sensor()`, and `@effect()`, materializes them on demand or via schedule, and stores artifacts with content-addressed caching.

### Workspace structure

This is a uv workspace with three packages:

| Package | Path | Purpose |
|---|---|---|
| `barca` | `packages/barca-core/` | Core library — decorators, models, store, engine, hashing, tracing, reconciler |
| `barca-cli` | `packages/barca-cli/` | CLI tool — typer app, table formatting |
| `barca-server` | `packages/barca-server/` | HTTP API + background scheduler — FastAPI, uvicorn (optional) |

### Core library (`packages/barca-core/src/barca/`)

| File | Responsibility |
|---|---|
| `__init__.py` | Public API: `asset`, `sensor`, `effect`, `cron`, `partitions`, `Partitions`, `unsafe`, `load_inputs`, `materialize`, `read_asset`, `list_versions` |
| `_asset.py` | `@asset()` decorator, `AssetWrapper` class, `partitions()` helper |
| `_sensor.py` | `@sensor()` decorator, `SensorWrapper` class — external state observers |
| `_effect.py` | `@effect()` decorator, `EffectWrapper` class — external state side-effects |
| `_schedule.py` | Schedule primitives — `cron()`, `CronSchedule`, `is_schedule_eligible()` |
| `_reconciler.py` | `reconcile()` — single-pass DAG walk: reindex, topo-sort, staleness, execute |
| `_unsafe.py` | `@unsafe` decorator — escape hatch for untraceable functions |
| `_trace.py` | AST dependency tracing — `extract_dependencies()`, `compute_dependency_hash()`, `analyze_purity()` |
| `_hashing.py` | Pure hash functions — `compute_codebase_hash()`, `compute_definition_hash()`, `compute_run_hash()` |
| `_models.py` | Pydantic models — `InspectedAsset`, `IndexedAsset`, `AssetInput`, `MaterializationRecord`, `AssetSummary`, `AssetDetail`, `JobDetail`, `SensorObservation`, `EffectExecution`, `ReconcileResult` |
| `_store.py` | `MetadataStore` — SQLite (stdlib) or Turso/libSQL via `libsql-experimental` |
| `_inspector.py` | `inspect_modules()` — imports modules, finds `@asset`/`@sensor`/`@effect` functions, extracts metadata + dependency hashes |
| `_engine.py` | Orchestration: `reindex()`, `refresh()`, `trigger_sensor()`, `materialize_asset()`, `reset()`, `build_indexed_asset()` |
| `_notebook.py` | Notebook helpers — `load_inputs()`, `materialize()`, `read_asset()`, `list_versions()` for interactive/notebook usage |
| `_config.py` | `barca.toml` parsing via `tomllib` |

### CLI (`packages/barca-cli/src/barca_cli/`)

| File | Responsibility |
|---|---|
| `cli.py` | Typer app — `reindex`, `reset`, `reconcile`, `serve`, `assets {list,show,refresh}`, `sensors {list,show,trigger}`, `jobs {list,show}` |
| `display.py` | Table formatting for terminal output |

### Server (`packages/barca-server/src/barca_server/`)

| File | Responsibility |
|---|---|
| `app.py` | FastAPI app factory, lifespan, thin route handlers |
| `service.py` | Pure business logic layer — routes delegate here, no FastAPI deps |
| `scheduler.py` | Background reconcile loop (asyncio task) |
| `logging.py` | Structured JSON logging configuration |

The server is **optional** — all CLI commands work without it. The server adds HTTP API endpoints and a background scheduler.

### Dependencies

- **barca-core**: `pydantic>=2.0`, `croniter>=2.0`, `pyturso>=0.5.0` (Turso/libSQL DB driver with MVCC); optional `BARCA_TURSO_URL` for remote sync
- **barca-cli**: `barca` (workspace), `typer>=0.9`
- **barca-server**: `barca` (workspace), `fastapi>=0.115`, `uvicorn[standard]>=0.34`
- **dev**: `pytest>=8.0`, `niquests>=3.0`

### Design principles

1. **Pydantic models** — all data structures are `BaseModel`. Validation at boundaries.
2. **Functional style** — pure functions wherever possible. Side effects (DB, file I/O) at the edges.
3. **Three packages** — `barca` is the reusable library; `barca-cli` is the thin CLI layer; `barca-server` is the optional HTTP server.
4. **Routes are thin wrappers** — FastAPI handlers validate params and delegate to `service.py`. No business logic in route functions.
5. **No subprocess workers** — materialize via `importlib.import_module()` + direct function call.
6. **Free-threaded Python** — defaults to 3.13t (GIL disabled). Partition parallelism via `ThreadPoolExecutor`. Opt out by changing `.python-version` to `3.13`.
7. **Turso primary** — `pyturso` package (DB-API 2.0) for MVCC concurrency; optional `BARCA_TURSO_URL` for remote sync via `turso.sync`.
8. **Same hashing protocol** — `PROTOCOL_VERSION = "0.3.0"`, JSON payload -> SHA-256.
9. **DB thread safety** — Turso/libSQL supports MVCC for concurrent reads. `MetadataStore` should still be created per-thread for server routes (`to_thread` pattern).

### Node kinds

| Kind | Decorator | Schedule default | Cached | Can be input |
|---|---|---|---|---|
| **asset** | `@asset()` | `"manual"` | Yes (by `run_hash`) | Yes |
| **sensor** | `@sensor()` | `"always"` | No (always re-runs) | Yes |
| **effect** | `@effect()` | `"always"` | No (always re-runs) | No (leaf node) |

### Schedule types

| Schedule | Behavior |
|---|---|
| `"manual"` | Only runs via explicit `barca assets refresh` |
| `"always"` | Runs whenever stale + upstream ready (during reconcile) |
| `cron("0 5 * * *")` | Runs when a cron tick has occurred since last run |

### Asset lifecycle

1. **Index**: `reindex()` imports Python modules, finds decorated functions, computes `definition_hash` (SHA-256 of source + metadata + dependency cone hash + protocol version), upserts into DB.
2. **Refresh**: `refresh(store, repo_root, asset_id)` recursively materializes upstream deps, then calls the asset function, saves result as JSON to `.barcafiles/{slug}/{definition_hash}/value.json`, records success in DB.
3. **Reconcile**: `reconcile(store, repo_root)` walks the entire DAG in topo order, checks staleness (definition changed, upstream refreshed) and schedule eligibility, executes sensors/assets/effects as needed.
4. **Cache**: If `run_hash` matches an existing successful materialization, the asset is skipped (content-addressed caching).
5. **Reset**: `reset()` removes `.barca/` (DB), `.barcafiles/` (artifacts), and/or `tmp/`.

### Server API endpoints

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

### DB schema

SQLite at `.barca/metadata.db`. Tables: `assets`, `asset_definitions`, `codebase_snapshots`, `materializations`, `job_logs`, `asset_inputs`, `materialization_inputs`, `sensor_observations`, `effect_executions`.

### Key constraints

- `continuity_key` must be unique per asset (defaults to `{relative_file}:{function_name}`, overridable via `@asset(name=...)`).
- No file should exceed ~500 lines; split further if needed.
- `dependency_cone_hash` traces per-function dependencies via AST; falls back to `codebase_hash` if tracing fails.
- `@unsafe` decorated functions skip dependency tracing entirely.
- Effects cannot be used as inputs to other nodes (they are leaf nodes).
- Sensors return `(update_detected: bool, output)` tuples.
- `@sensor()` and `@effect()` accept optional `description: str` and `tags: list[str]` metadata parameters.
- Notebook helpers (`load_inputs`, `materialize`, `read_asset`, `list_versions`) auto-discover the project root via `barca.toml`.
