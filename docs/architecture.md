# Architecture

This document describes the Barca architecture.

## High-level shape

Barca is a pure Python uv workspace with three packages:

1. **barca** (core library) — decorators, models, store, engine, hashing, tracing, reconciler
2. **barca-cli** — typer-based CLI tool
3. **barca-server** — optional FastAPI HTTP API + background scheduler

Users define assets, sensors, and effects with Python decorators. Barca discovers them via `importlib`, materializes them via direct function call, and stores artifacts with content-addressed caching.

## Workspace structure

```
packages/
├── barca-core/src/barca/       # Core library
│   ├── __init__.py             # Public API: asset, sensor, effect, cron, partitions
│   ├── _asset.py               # @asset() decorator, AssetWrapper
│   ├── _sensor.py              # @sensor() decorator, SensorWrapper
│   ├── _effect.py              # @effect() decorator, EffectWrapper
│   ├── _schedule.py            # cron(), CronSchedule, is_schedule_eligible()
│   ├── _reconciler.py          # reconcile() — single-pass DAG walk
│   ├── _engine.py              # reindex(), refresh(), trigger_sensor(), reset()
│   ├── _store.py               # MetadataStore — SQLite/libSQL persistence
│   ├── _models.py              # Pydantic models for all data structures
│   ├── _inspector.py           # inspect_modules() — discovers decorated functions
│   ├── _hashing.py             # SHA-256 hashing protocol
│   ├── _trace.py               # AST dependency tracing
│   ├── _config.py              # barca.toml parsing
│   └── _unsafe.py              # @unsafe escape hatch
├── barca-cli/src/barca_cli/    # CLI tool
│   ├── cli.py                  # Typer app — reindex, reset, reconcile, serve, assets, sensors, jobs
│   └── display.py              # Table formatting for terminal output
└── barca-server/src/barca_server/  # HTTP server (optional)
    ├── app.py                  # FastAPI app factory, lifespan, route handlers
    ├── service.py              # Pure business logic (no FastAPI deps)
    ├── scheduler.py            # Background reconcile loop
    └── logging.py              # Structured JSON logging
```

## Design principles

1. **Pure Python** — no native extensions, no subprocess workers, no Rust. Materialize via `importlib.import_module()` + direct function call.
2. **Pydantic models** — all data structures are `BaseModel`. Validation at boundaries.
3. **Functional style** — pure functions wherever possible. Side effects (DB, file I/O) at the edges.
4. **Three packages** — `barca` is the reusable library; `barca-cli` is the thin CLI layer; `barca-server` is the optional HTTP server.
5. **Routes are thin wrappers** — FastAPI handlers validate params and delegate to `service.py`. No business logic in route functions.
6. **Free-threaded Python** — defaults to 3.13t (GIL disabled). Partition parallelism via `ThreadPoolExecutor`.
7. **Same hashing protocol** — `PROTOCOL_VERSION = "0.3.0"`, JSON payload -> SHA-256.
8. **sqlite3 thread safety** — `MetadataStore` must be created in the same thread that uses it. Server routes create stores inside `to_thread` calls.

## Node kinds

| Kind | Decorator | Schedule default | Cached | Can be input |
|------|-----------|-----------------|--------|-------------|
| **asset** | `@asset()` | `"manual"` | Yes (by `run_hash`) | Yes |
| **sensor** | `@sensor()` | `"always"` | No (always re-runs) | Yes |
| **effect** | `@effect()` | `"always"` | No (always re-runs) | No (leaf node) |

Sensors are source nodes that bring external state into the graph. They return `(update_detected: bool, output)` tuples and record append-only observation history.

Effects are leaf nodes that push graph state to external systems. They execute after their upstream assets/sensors.

## Execution model

Barca executes user functions directly in the same Python process:

1. `importlib.import_module(module_path)` loads the user's module
2. `getattr(mod, function_name)` retrieves the decorated function
3. The original function (unwrapped from the decorator) is called with resolved inputs
4. Results are serialized to JSON and stored in `.barcafiles/`

There are no subprocess workers, no IPC, and no serialization boundaries between the orchestrator and user code. This keeps execution fast and tracebacks accurate.

For partitioned assets, Barca uses `ThreadPoolExecutor` with free-threaded Python (3.13t, GIL disabled) for true parallel execution.

## Storage

### Metadata store

SQLite at `.barca/metadata.db`. Tables:

- `assets` — discovered nodes (assets, sensors, effects)
- `asset_definitions` — versioned definition snapshots
- `codebase_snapshots` — merkle tree hashes
- `materializations` — execution records for assets
- `sensor_observations` — execution records for sensors
- `effect_executions` — execution records for effects
- `asset_inputs` — DAG edges (parameter → upstream asset)

### Artifact store

Filesystem at `.barcafiles/`:

```
.barcafiles/{asset_slug}/{definition_hash}/
├── value.json     # serialized return value
└── code.txt       # source text at time of materialization
```

## Reconciliation

`reconcile()` performs a single-pass DAG walk:

1. **Reindex** — discover all nodes from Python modules
2. **Topo-sort** — Kahn's algorithm over the DAG
3. **Walk** — for each node in order:
   - **Sensors**: check schedule eligibility, execute, record observation, propagate `update_detected` downstream
   - **Assets**: check staleness (definition changed or upstream refreshed), check schedule, check cache, materialize if needed
   - **Effects**: check upstream freshness, execute if upstream was refreshed

The reconciler can run as a single pass (`barca reconcile`), in a loop (`barca reconcile --watch`), or as a background scheduler in the server (`barca serve`).

## Dependencies

- **barca (core)**: `pydantic>=2.0`, `croniter>=2.0`, `sqlite3` (stdlib); optional `libsql-experimental` for Turso remote
- **barca-cli**: `barca` (workspace), `typer>=0.9`
- **barca-server**: `barca` (workspace), `fastapi>=0.115`, `uvicorn[standard]>=0.34`
- **dev**: `pytest>=8.0`, `niquests>=3.0`
