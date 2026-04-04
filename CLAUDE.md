# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Install dependencies
uv sync

# Run tests
uv run pytest tests/ -v

# CLI commands (from a project directory with barca.toml, or workspace root)
uv run barca reindex
uv run barca assets list
uv run barca assets show <id>
uv run barca assets refresh <id>
uv run barca jobs list
uv run barca jobs show <id>
uv run barca reset [--db] [--artifacts] [--tmp]

# Run from an example project
cd examples/basic_app && uv sync && uv run barca reindex
```

## Architecture

Barca is a minimal asset orchestrator: a pure Python uv workspace that discovers Python functions decorated with `@asset()`, materializes them on demand, and stores artifacts with content-addressed caching.

### Workspace structure

This is a uv workspace with two packages:

| Package | Path | Purpose |
|---|---|---|
| `barca` | `packages/barca-core/` | Core library — decorator, models, store, engine, hashing, tracing |
| `barca-cli` | `packages/barca-cli/` | CLI tool — typer app, table formatting |

### Core library (`packages/barca-core/src/barca/`)

| File | Responsibility |
|---|---|
| `__init__.py` | Public API: `asset`, `AssetWrapper`, `partitions`, `Partitions`, `unsafe` |
| `_asset.py` | `@asset()` decorator, `AssetWrapper` class, `partitions()` helper |
| `_unsafe.py` | `@unsafe` decorator — escape hatch for untraceable functions |
| `_trace.py` | AST dependency tracing — `extract_dependencies()`, `compute_dependency_hash()`, `analyze_purity()` |
| `_hashing.py` | Pure hash functions — `compute_codebase_hash()`, `compute_definition_hash()`, `compute_run_hash()` |
| `_models.py` | Pydantic models — `InspectedAsset`, `IndexedAsset`, `AssetInput`, `MaterializationRecord`, `AssetSummary`, `AssetDetail`, `JobDetail` |
| `_store.py` | `MetadataStore` — Turso/libSQL via `libsql-experimental` |
| `_inspector.py` | `inspect_modules()` — imports modules, finds `@asset` functions, extracts metadata + dependency hashes |
| `_engine.py` | Orchestration: `reindex()`, `refresh()`, `materialize_asset()`, `reset()`, `build_indexed_asset()` |
| `_config.py` | `barca.toml` parsing via `tomllib` |

### CLI (`packages/barca-cli/src/barca_cli/`)

| File | Responsibility |
|---|---|
| `cli.py` | Typer app — `reindex`, `reset`, `assets {list,show,refresh}`, `jobs {list,show}` |
| `display.py` | Table formatting for terminal output |

The CLI opens `.barca/metadata.db` directly and uses the same `MetadataStore` + engine functions as any library consumer.

### Dependencies

- **barca-core**: `pydantic>=2.0`, `libsql-experimental>=0.0.50`
- **barca-cli**: `barca` (workspace), `typer>=0.9`
- **dev**: `pytest>=8.0`

### Design principles

1. **Pydantic models** — all data structures are `BaseModel`. Validation at boundaries.
2. **Functional style** — pure functions wherever possible. Side effects (DB, file I/O) at the edges.
3. **Two packages** — `barca` is the reusable library; `barca-cli` is the thin CLI layer.
4. **No async** — synchronous throughout for CLI simplicity.
5. **No subprocess workers** — materialize via `importlib.import_module()` + direct function call.
6. **Turso via libsql-experimental** — DB-API 2.0: `connect()`, `execute()`, `fetchall()`, `commit()`.
7. **Same hashing protocol** — `PROTOCOL_VERSION = "0.3.0"`, JSON payload -> SHA-256.

### Asset lifecycle

1. **Index**: `reindex()` imports Python modules, finds `@asset` functions, computes `definition_hash` (SHA-256 of source + metadata + dependency cone hash + protocol version), upserts into DB.
2. **Refresh**: `refresh(store, repo_root, asset_id)` recursively materializes upstream deps, then calls the asset function, saves result as JSON to `.barcafiles/{slug}/{definition_hash}/value.json`, records success in DB.
3. **Cache**: If `run_hash` matches an existing successful materialization, the asset is skipped (content-addressed caching).
4. **Reset**: `reset()` removes `.barca/` (DB), `.barcafiles/` (artifacts), and/or `tmp/`.

### DB schema

Turso (local libSQL) at `.barca/metadata.db`. Tables: `assets`, `asset_definitions`, `codebase_snapshots`, `materializations`, `job_logs`, `asset_inputs`, `materialization_inputs`.

### Key constraints

- `continuity_key` must be unique per asset (defaults to `{relative_file}:{function_name}`, overridable via `@asset(name=...)`).
- No file should exceed ~500 lines; split further if needed.
- `dependency_cone_hash` traces per-function dependencies via AST; falls back to `codebase_hash` if tracing fails.
- `@unsafe` decorated functions skip dependency tracing entirely.
