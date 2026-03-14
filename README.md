<p align="center">
  <h1 align="center">barca</h1>
  <p align="center">A modern, minimal asset orchestrator.</p>
</p>

<p align="center">
  <a href="https://github.com/ExSidius/barca/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/ExSidius/barca/actions/workflows/ci.yml/badge.svg" /></a>
  <a href="https://github.com/ExSidius/barca/releases"><img alt="Release" src="https://img.shields.io/github/v/release/ExSidius/barca?include_prereleases&label=release" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-2021_edition-orange" />
  <img alt="Python" src="https://img.shields.io/badge/python-%E2%89%A53.11-blue" />
  <img alt="License" src="https://img.shields.io/github/license/ExSidius/barca" />
</p>

---

Barca is a Rust backend that discovers Python functions decorated with `@asset()`, materializes them on demand, stores versioned artifacts, and serves a reactive UI — all from a single binary.

```python
from barca import asset, partitions

@asset()
def revenue() -> dict:
    return {"q1": 100, "q2": 200}

@asset(inputs={"data": revenue})
def report(data: dict) -> str:
    return f"Total: {data['q1'] + data['q2']}"

@asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
def prices(ticker: str) -> dict:
    return {"ticker": ticker, "price": len(ticker) * 100}
```

```
$ barca reindex
┌────┬─────────┬────────────┬──────────┬───────────┐
│ ID ┆ Name    ┆ Module     ┆ Function ┆ Status    │
╞════╪═════════╪════════════╪══════════╪═══════════╡
│ 1  ┆ revenue ┆ my_project ┆ revenue  ┆ never run │
│ 2  ┆ report  ┆ my_project ┆ report   ┆ never run │
│ 3  ┆ prices  ┆ my_project ┆ prices   ┆ never run │
└────┴─────────┴────────────┴──────────┴───────────┘

$ barca assets refresh 3
Waiting for materialization of asset #3 (3 jobs)...
Asset #3
  Name:   prices
  Status: #42 (success)
```

## Install

```bash
uv add barca
```

This installs both the `barca` CLI and the Python `@asset()` decorator.

## Quick Start

```bash
uv init my-project
cd my-project
uv add barca
```

Write your assets anywhere in your project — barca discovers them automatically:

```python
# main.py (or any .py file)
from barca import asset

@asset()
def hello() -> dict:
    return {"message": "Hello from barca!"}
```

Run:

```bash
barca reindex                    # discover assets from Python files
barca assets list                # list all indexed assets
barca assets refresh 1           # materialize an asset
barca                            # start the web UI at localhost:3000
```

No config file needed. Barca scans your project for `@asset()`-decorated functions automatically.

### CLI Reference

```
barca                            Start the server (default)
barca serve                      Start the server
barca reindex                    Re-inspect Python modules
barca assets list                List all indexed assets
barca assets show <id>           Show asset detail
barca assets refresh <id>        Trigger materialization and wait
barca jobs list                  List recent jobs
barca jobs show <id>             Show job detail
barca reset [--db] [--artifacts] Clean generated files
```

## Features

**Implemented** (workflows 1–4):

- **Asset discovery** — decorate any Python function with `@asset()`, barca finds it
- **Dependency tracking** — declare upstream inputs with `@asset(inputs={"x": upstream})`, barca resolves the DAG, materializes upstreams first, and passes artifacts as kwargs
- **Partitioned assets** — `@asset(partitions={"key": partitions([...])})` fans out into N parallel jobs, one per partition value
- **Parallel execution** — partition batches pre-fetch shared data once, then dispatch up to 64 concurrent Python workers with near-zero lock contention
- **Artifact versioning** — each materialization is keyed by a `definition_hash` (source + deps + lock file) and a `run_hash` (definition + upstream versions + partition key); identical runs are cached
- **Asset continuity** — rename or move assets while preserving lineage via `@asset(name="stable_name")`
- **Reactive UI** — Datastar-powered SSE interface with real-time job status, asset panels, and log streaming
- **JSON API** — full REST API with OpenAPI spec at `/api/docs`
- **CLI** — unified `barca` binary for all operations (no HTTP needed — reads the DB directly)

**Planned** (workflows 5–9):

| Workflow | Description | Status |
|----------|-------------|--------|
| 5 | Schedule-driven reconciliation | Spec ready |
| 6 | Sensors and external observations | Spec ready |
| 7 | Notebook workflow (`load_inputs()`) | Spec ready |
| 8 | Backfill and replay | Spec ready |
| 9 | Execution controls (timeout, cancel, retry) | Spec ready |

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    barca (Rust)                          │
│                                                         │
│  ┌──────────┐  ┌──────────────┐  ┌───────────────────┐  │
│  │ barca-cli│  │ barca-server │  │    barca-core     │  │
│  │          │  │              │  │                   │  │
│  │ CLI bin  │  │ axum routes  │  │ models, hashing   │  │
│  │ (clap)   │  │ SSE/Datastar │  │ serialization     │  │
│  │          │  │ store (Turso)│  │                   │  │
│  │          │  │ templates    │  │                   │  │
│  │          │  │ PythonBridge │  │                   │  │
│  └────┬─────┘  └──────┬───────┘  └───────────────────┘  │
│       │               │                                  │
│       └───────┬───────┘                                  │
│               │ subprocess                               │
│  ┌────────────▼────────────┐                             │
│  │       barca-py          │                             │
│  │  PyO3 native extension  │                             │
│  │  @asset() decorator     │                             │
│  │  inspect / worker CLIs  │                             │
│  └─────────────────────────┘                             │
└─────────────────────────────────────────────────────────┘
         │                              │
         ▼                              ▼
   .barca/metadata.db           .barcafiles/
   (Turso — assets,             (versioned artifacts:
    definitions, jobs)           value.json, metadata.json)
```

**Four crates**, one workspace:

| Crate | Purpose | Lines |
|-------|---------|-------|
| `barca-cli` | Unified CLI binary — clap subcommands, table formatting | ~400 |
| `barca-core` | Shared library — models, hashing, serialization types | ~300 |
| `barca-server` | Axum server — routes, SSE, templates, store, PythonBridge trait | ~3,500 |
| `barca-py` | PyO3 native extension — `@asset()` decorator, inspect, worker | ~500 |

**Key design decisions:**
- The server never embeds Python — it spawns subprocesses via the `PythonBridge` trait
- The CLI opens the database directly — it does not call the HTTP API
- All HTML is generated as Rust string functions — no template engine, no external files
- No custom JavaScript — the UI uses Datastar attributes for all interactivity
- No polling — real-time updates via SSE broadcast channels

## Asset Lifecycle

1. **Index** — `reindex()` inspects Python modules, computes a `definition_hash` (SHA-256 of source + metadata + uv.lock + protocol version), upserts into the database
2. **Enqueue** — checks for duplicate/active jobs, resolves the dependency DAG, enqueues upstream assets first, then inserts partition jobs in batch
3. **Execute** — worker claims jobs, verifies definition hasn't changed, resolves upstream artifacts, spawns Python subprocess, stores artifact at `.barcafiles/{slug}/{hash}/value.json`
4. **Serve** — SSE broadcasts completion; Datastar patches the DOM in real time

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Backend | Rust + [axum](https://github.com/tokio-rs/axum) 0.8 |
| Database | [Turso](https://turso.tech/) (libSQL) |
| Python FFI | [PyO3](https://pyo3.rs/) 0.25 + [maturin](https://www.maturin.rs/) |
| Async | [tokio](https://tokio.rs/) 1.45 |
| UI | [Datastar](https://data-star.dev/) (SSE-based DOM patching) + Tailwind CSS |
| API docs | [utoipa](https://github.com/juhaku/utoipa) (OpenAPI + Swagger UI) |
| CLI | [clap](https://github.com/clap-rs/clap) 4 + [comfy-table](https://github.com/Nukesor/comfy-table) |
| Python env | [uv](https://docs.astral.sh/uv/) |

## Testing

```bash
# Unit + API tests (mock Python bridge, fast)
cargo test -p barca-server -p barca-cli --test cli_tests

# End-to-end integration tests (real Python, real subprocesses)
just test-e2e
```

| Suite | Tests | What it covers |
|-------|-------|---------------|
| API tests | 33 active, 27 planned | Asset CRUD, reindex, materialization, dependencies, partitions, continuity |
| CLI tests | 5 | Help output, error handling |
| E2E w1 | 6 | Basic assets — reindex, list, show, refresh, caching, reset |
| E2E w2 | 3 | Dependencies — upstream triggers, artifact content, reuse |
| E2E w3 | 4 | Partitions — small (3), large (10,000), caching, artifact verification |

The 10,000-partition E2E test runs all partitions through real Python subprocesses with 64-way parallelism, completing in ~60 seconds.

## Development

```bash
# Prerequisites
cargo install cargo-watch    # optional, for hot reload
uv tool install maturin      # for building the Python extension

# Hot reload
just dev

# Build Tailwind CSS (after changing template classes)
just build-css

# Build the Python extension + CLI into the active venv
just build-py

# Run the example app
cd examples/basic_app && uv sync && cargo run -p barca-cli
```

## Docs

- [Architecture](./docs/architecture.md)
- [Core constraints](./docs/core-constraints.md)
- [Datastar reference](./docs/datastar-reference.md)

### Workflow specs

| # | Workflow | Status |
|---|---------|--------|
| 1 | [Single asset, no inputs](./docs/workflows/01-single-asset-no-inputs.md) | Done |
| 2 | [Single asset with one input](./docs/workflows/02-single-asset-one-input.md) | Done |
| 3 | [Parametrized assets and partitions](./docs/workflows/03-parametrized-assets-and-partitions.md) | Done |
| 4 | [Asset continuity (rename/move)](./docs/workflows/04-asset-continuity-rename-and-move.md) | Done |
| 5 | [Schedule-driven reconciliation](./docs/workflows/05-schedule-driven-reconciliation-and-effects.md) | Planned |
| 6 | [Sensors and external observations](./docs/workflows/06-sensors-and-external-observations.md) | Planned |
| 7 | [Notebook workflow](./docs/workflows/07-notebook-workflow.md) | Planned |
| 8 | [Backfill and replay](./docs/workflows/08-backfill-and-replay.md) | Planned |
| 9 | [Execution controls](./docs/workflows/09-execution-controls-and-ad-hoc-params.md) | Planned |
