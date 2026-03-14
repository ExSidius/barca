# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Run the server using the basic_app example (cd into it for barca.toml + venv)
cd examples/basic_app && cargo run -p barca-cli
# or: cd examples/basic_app && cargo run -p barca-cli -- serve

# CLI commands (open DB directly, no server needed)
cargo run -p barca-cli -- assets list
cargo run -p barca-cli -- assets show <id>
cargo run -p barca-cli -- assets refresh <id>
cargo run -p barca-cli -- jobs list
cargo run -p barca-cli -- jobs show <id>
cargo run -p barca-cli -- reindex
cargo run -p barca-cli -- reset [--db] [--artifacts] [--tmp]

# Run with hot reload (requires: cargo install cargo-watch)
just dev

# Build Tailwind CSS (run after changing Tailwind classes in templates)
just build-css

# Build and install the barca Python extension into the active venv
just build-py
# or: cd crates/barca-py && maturin develop

# Run tests
cargo test -p barca-server -p barca-cli
```

Logging uses `tracing` with `RUST_LOG=info` by default.

## Architecture

Barca is a minimal asset orchestrator: a Rust (axum) backend that discovers Python functions decorated with `@asset()`, materializes them on demand, stores artifacts, and serves a reactive UI.

### Workspace structure

This is a Cargo workspace with four crates:

| Crate | Purpose |
|---|---|
| `crates/barca-cli` | Unified CLI (`barca` binary) — clap subcommands, table formatting, serve entry point |
| `crates/barca-core` | Shared Rust library — models, hashing, serialization types |
| `crates/barca-server` | Axum server library — routes, SSE, templates, store, python bridge trait |
| `crates/barca-py` | PyO3 native extension — `@asset()` decorator, inspect CLI, worker CLI |

### Startup flow (`crates/barca-server/src/main.rs`)
1. Load `barca.toml` → resolve Python modules to index
2. Open Turso (local libSQL) at `.barca/metadata.db`
3. Call `reindex()` → inspect Python modules via `uv run python -m barca.inspect`, upsert into DB
4. Spawn background worker (`run_refresh_queue_worker`) that processes the materialization queue
5. Start axum server on port 3000

### Core modules (`crates/barca-server/src/`)
| File | Responsibility |
|---|---|
| `lib.rs` | Orchestration: `reindex()`, `enqueue_refresh_request()`, `run_refresh_queue_worker()`, `execute_refresh_job()` |
| `server.rs` | Axum routes and SSE response handlers; all rendering logic that needs request context |
| `templates.rs` | All HTML generation as Rust string functions (no template engine) |
| `store.rs` | Turso (libSQL) layer — assets, definitions, materializations |
| `python_bridge.rs` | `PythonBridge` trait + `UvPythonBridge` impl (subprocess to `uv run python -m barca.inspect` / `barca.worker`) |
| `config.rs` | Parses `barca.toml` |

### Shared library (`crates/barca-core/src/`)
| File | Responsibility |
|---|---|
| `models.rs` | Shared data structs (`InspectedAsset`, `WorkerResponse`, `IndexedAsset`, etc.) |
| `hashing.rs` | Definition hash computation (deterministic, includes protocol version + uv.lock hash) |

### Python extension (`crates/barca-py/`)
A PyO3 native extension built with maturin. The native Rust module is `_barca`, wrapped by a Python package `barca/`:
- `src/lib.rs` — `#[pymodule] _barca`: `asset()` decorator, `inspect_modules()`, `materialize_asset()`
- `python/barca/__init__.py` — re-exports `asset` and `AssetWrapper` from `_barca`
- `python/barca/inspect.py` — thin CLI stub for `python -m barca.inspect`
- `python/barca/worker.py` — thin CLI stub for `python -m barca.worker`

Users install the extension with `maturin develop` (dev) or `pip install barca` (release). The server shells out to `python -m barca.inspect` / `python -m barca.worker` as subprocesses — same model as before, but the code behind those commands is now compiled Rust.

### Asset lifecycle
1. **Index**: `reindex()` calls `python_bridge.inspect_modules()` → computes `definition_hash` (SHA-256 of source + metadata + uv.lock + protocol version) → upserts into `assets` + `asset_definitions` tables
2. **Enqueue**: `POST /assets/{id}/materialize` → `enqueue_refresh_request()` checks for duplicate runs/active jobs before inserting into `materializations` queue
3. **Execute**: Worker loop claims jobs → re-inspects asset → calls `python_bridge.materialize_asset()` → moves artifact to `.barcafiles/{slug}/{definition_hash}/value.json` → marks success/failure in DB → broadcasts completion via `tokio::sync::broadcast`
4. **Serve**: SSE streams in `server.rs` subscribe to the broadcast channel and push `PatchElements` updates to the client

### UI stack
- **Datastar** (v1.0.0-RC.8): SSE-based DOM patching. Use `PatchElements::new(html).selector("#id").write_as_axum_sse_event()` for all patches. Attribute syntax: `data-on:click="@post('/url')"` (colon separator; `@` prefix). Signals: `data-signals='{mySignal: false}'` (JS object notation, not JSON). Event variable: `evt` (not `$event`). Panel open/close uses `$_panelOpen` signal with `data-class:open`; all panel content loaded via `@get` (no custom JS EventSource management).
- **Tailwind CSS**: Built via `just build-css` (standalone Tailwind CLI in `bin/`), embedded via `include_str!("../../../static/css/output.css")`. Dark mode is class-based. Run `just build-css` after touching any Tailwind classes.
- **Web Components**: `<asset-status-badge label="..." tone="...">` defined inline in `templates::web_components()`.
- **Asset panel**: Opens via pure Datastar — `$_panelOpen = true; @get('/assets/{id}/panel/stream')`. No custom JavaScript. SSE events processed natively by Datastar, patching `#panel-content`.

### CLI architecture (`crates/barca-cli/src/`)
| File | Responsibility |
|---|---|
| `main.rs` | Clap Parser + subcommand dispatch |
| `commands.rs` | Command implementations — each opens DB directly via the same store/lib.rs functions |
| `display.rs` | Table formatters using `comfy-table` |

The CLI does NOT call the HTTP API. It opens `.barca/metadata.db` and uses the same `MetadataStore` + `lib.rs` orchestration as the server. For write operations (refresh/reindex), it creates a full `AppState` with a real `UvPythonBridge` and spawns the worker in-process.

### PythonBridge trait
`python_bridge::PythonBridge` is an async trait (`async_trait`). `AppState.python` is `Arc<dyn PythonBridge>`.
- `UvPythonBridge` — real impl, shells out to `uv run python -m barca.*`
- `MockPythonBridge` — used in API tests, returns canned data without Python

### JSON API + OpenAPI
All `/api/*` endpoints are annotated with `#[utoipa::path(...)]`. OpenAPI spec at `/api/openapi.json`, Swagger UI at `/api/docs`.

Available endpoints:
- `GET /api/assets` — list all assets
- `GET /api/assets/{id}` — asset detail
- `POST /api/assets/{id}/materialize` — enqueue refresh
- `POST /api/reindex` — re-inspect Python modules
- `GET /api/jobs` — list recent jobs
- `GET /api/jobs/{id}` — job detail

### Key constraints
- **Minimize custom JavaScript** — use Datastar attributes (`data-on:click`, `@get`, `@post`, signals, `data-class:`) instead of writing JavaScript. Only use JS when Datastar genuinely cannot handle the interaction.
- All HTML templates live in `crates/barca-server/src/templates.rs` as Rust functions — no external template files.
- No file should exceed ~500 lines; split further if needed.
- No polling: use server-push SSE via broadcast channels.
- `continuity_key` must be unique per asset (defaults to `file:function_name`, overridable via `@asset(name=...)`).
- Always call `templates::escape_html()` when interpolating user/code data into HTML strings.
- The server **never embeds Python** — it spawns subprocesses. No `pyo3` dependency in `barca-server`.
