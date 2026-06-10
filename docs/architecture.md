# Architecture

This document describes the Barca architecture.

## High-level shape

Barca is an **invisible asset orchestrator**: a Rust binary parses Python source statically,
builds a DAG, plans execution, and dispatches work to short-lived Python worker processes. Users
install a single `barca` package that ships both the CLI binary and the Python decorator stubs.

The Rust workspace is split into layered crates so each concern stays independent:

```
barca-core    pure library — parse, dag, planner, dispatch, db, cache, hash, commands
   ▲          (no HTTP / UI dependencies)
   │
barca-server  axum HTTP/JSON API + in-memory run tracking + dev file watcher
   ▲
   │
barca-cli     the `barca` binary — thin clap dispatcher; `serve` calls barca-server
```

A future **ui** is a separate (non-Rust) package that consumes the `barca-server` HTTP API as
its contract. The CLI, the server, and any UI are peers over the same core.

## Workspace structure

```
Cargo.toml                  Rust workspace root
crates/
  barca-core/               Core library
    src/
      model.rs              Domain types: NodeKind, Freshness, ExtractedNode, DagNode, StepId, …
      parse.rs              Python AST extraction via ruff (no import)
      cone.rs               Dependency-cone analysis for definition hashing
      dag.rs                petgraph DAG construction + validation
      planner.rs            Phase/stream execution planning
      dispatch.rs           Coordinates worker pool via UDS, manages the global ready queue
      cache.rs              run_hash computation for cache lookups
      hash.rs               SHA-256 definition/run hashing
      db.rs                 Turso/libSQL persistence (.barca/metadata.db)
      commands.rs           User-facing ops: get, plan, history, stats, list_assets, build_dag
  barca-server/             HTTP server (optional, started by `barca serve`)
    src/
      lib.rs                serve()/app() entrypoints + ServeConfig
      routes.rs             axum Router (the single API boundary)
      handlers.rs           Endpoint handlers — delegate to commands via spawn_blocking
      state.rs              AppState, RunState/RunStatus, DAG cache
      error.rs              BarcaError → HTTP status + JSON mapping
      watch.rs              notify-based dev file watcher (--watch)
  barca-cli/                The `barca` binary
    src/main.rs             clap Cli enum + thin dispatch to commands / barca-server
python/barca/               Python stubs + worker (shipped in the same wheel)
  __init__.py               No-op decorator stubs (identity functions)
  _worker.py                Batch worker invoked as `python -m barca._worker`
  _artifacts.py             json / pickle / parquet serialization
pyproject.toml              Maturin build (binary + Python stubs in one wheel)
```

## How it works

1. **Rust coordinator** (`barca get <file.py>`):
   - Parses Python with ruff's AST — pure static analysis, never imports user code.
   - Builds a petgraph DAG from `@asset` / `@sensor` / `@task` decorators.
   - Generates a tiered execution plan, persists plan + results to a local Turso/libSQL DB.
   - Maintains a pool of stateless Python worker processes and a global ready queue.
   - Workers pull one task at a time from the ready queue via Unix domain socket (UDS).

2. **Python worker** (`python -m barca._worker`):
   - Stateless: connects to the coordinator's UDS and pulls one task at a time from the
     global ready queue.
   - Imports user modules via `importlib.util.spec_from_file_location`.
   - Executes the task, serializes results (json / pickle / parquet), and reports back over
     the Unix domain socket protocol. No DB access — Rust owns all persistence.
   - For `parallel()` calls: the coordinator freezes the calling worker (SIGSTOP), spawns a
     temp replacement to maintain pool capacity, and adds the child items to the ready queue.
     When all children complete, the temp is killed and the parent is resumed (SIGCONT).

3. **Python stubs** (`from barca import asset, …`):
   - Pure no-ops so user code runs standalone and gets IDE/type support.

4. **HTTP server** (`barca serve`):
   - The `barca-server` crate reuses `barca_core::commands::*` directly (no subprocess).
   - Runs on a multi-thread Tokio runtime driving axum; each core call is run via
     `spawn_blocking` because the core commands build their own current-thread runtime.
   - Runs execute in background tasks and are tracked in a `DashMap`; clients poll `/status`.
   - See [Server API](server-api.md).

## Node kinds

| Kind | Decorator | Default freshness | Cached | Can be input |
|------|-----------|-------------------|--------|--------------|
| **asset** | `@asset()` | `Always` | Yes (by `run_hash`) | Yes |
| **sensor** | `@sensor()` | `Manual` | No (always re-runs) | Yes |
| **task** | `@task()` | `Always` | No (always re-runs) | Yes (to other tasks) |

Sensors bring external state into the graph and return `(update_detected, output)`. Tasks
produce side effects and always re-run; they can be inputs to other tasks.

## Storage

A local Turso/libSQL database at `.barca/metadata.db` holds:

- `materializations` — per-step execution records keyed by `(node_id, run_hash)`, with the
  artifact path/format/size and elapsed time.
- `runs` — per-run history (command, files, target, status, step counts, timing).

Artifacts are written under `.barca/artifacts/`. Data passes between worker batches as serialized
files (json / pickle / parquet), never in-process.

## Design principles

1. **Invisible** — the orchestrator adds near-zero perceptible overhead.
2. **Static analysis** — never import user code during planning.
3. **Rust for planning, Python for execution** — each does what it is best at.
4. **Single install** — `uv add barca` ships the binary and Python stubs together.
5. **Turso for persistence** — Rust owns the DB; Python workers have no DB access.
6. **Artifact-based data passing** — serialized files between worker batches.
7. **Layered crates** — core has no HTTP/UI awareness; the server is the single API boundary
   so the CLI, server, and a future UI are independent peers over the same core.

## Dependencies

- **Rust**: `ruff_python_parser` (AST), `petgraph` (DAG), `turso` (DB), `serde`/`serde_json`,
  `sha2`; the server adds `axum`, `tokio` (multi-thread), `dashmap`, and `notify`.
- **Python**: stdlib only at runtime (`pyarrow` optional for parquet).
- **Build**: `maturin` packages the Rust binary + Python stubs into one wheel.
