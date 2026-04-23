# AGENTS.md

Guidance for AI coding agents working in this repository. Written to match what
actually happens on a clean machine — not just what `pyproject.toml` claims.

## Environment setup

Barca targets **free-threaded CPython 3.14t** (GIL disabled). Version mismatches
here are the #1 source of spurious failures, so validate up front.

```bash
# 1. uv must be recent enough to know about stable 3.14 + free-threaded builds.
#    uv <= 0.8 resolves "3.14" to a beta (3.14.0b4), which ships a typing._eval_type
#    incompatible with modern pydantic and will crash on import with:
#       TypeError: _eval_type() got an unexpected keyword argument 'prefer_fwd_module'
uv self update                # need >= 0.11

# 2. Install the pinned interpreter. `.python-version` requests 3.14.3t; if uv
#    only has a beta cached, install the current stable free-threaded build.
uv python install 3.14t       # e.g. cpython-3.14.4+freethreaded

# 3. Sync the workspace.
uv sync

# 4. Sanity check — must be a stable (non-beta) free-threaded 3.14.
uv run python -c "import sys; print(sys.version)"
# expected: 3.14.<N> ... free-threading build, NOT 3.14.0b4
```

Never commit with a release-candidate or beta interpreter. If `uv sync` brings
in `3.14.0b4`, stop and run `uv python install --reinstall 3.14t` before doing
anything else.

### Example projects

The `examples/` apps each have their own `.venv` and `.python-version`. Sync
them the same way:

```bash
cd examples/basic_app
uv sync
uv run barca reindex
```

If `.python-version` in an example pins a beta you don't have, overwrite it
with a stable free-threaded version that `uv python list` shows as installed,
then `rm -rf .venv && uv sync`. Don't commit those `.python-version` bumps
unless that is the actual task.

## Running the server + UI

The React UI is **not** pre-built and not shipped in the Python package. The
server will start happily and serve `/ui/` as 404 unless you build it first:

```bash
cd packages/barca/src/barca/server/ui
npm ci           # requires Node 24+ (see .nvmrc)
npm run build    # outputs to ./dist, which the FastAPI app mounts
```

Then, from an example project:

```bash
cd examples/basic_app
PYTHON_GIL=0 uv run barca serve --port 8400
# UI:   http://localhost:8400/ui/
# API:  http://localhost:8400/api/...
# Docs: http://localhost:8400/docs
```

`PYTHON_GIL=0` silences the "GIL is enabled" warning and enables true thread
parallelism for partitioned assets.

### Database lock

`MetadataStore` uses pyturso, which takes an **exclusive file lock** on
`.barca/metadata.db`. Only one barca process may touch a given project at a
time. Common failure mode:

```
turso.Error: Locking error: Failed locking file. File is locked by another process
```

Kill the existing `barca serve` / `barca run` before starting another. When
orchestrating from an agent, check for a live server on the port (or
`ps -ef | grep "barca serve"`) before spawning a new one.

## Pre-push checklist

Run these before every push. All must exit 0.

```bash
# Lint + format everything (ruff, whitespace, yaml/toml checks)
prek run --all-files

# Python tests
uv run pytest tests/ -v

# E2E (builds the UI via Playwright's webServer config)
npm run test:e2e
```

## CLI cheatsheet

From any project with a `barca.toml` (examples/*, or the workspace root).
Prefix with `uv run` unless the venv is activated.

```bash
barca reindex                        # discover @asset/@sensor/@effect functions
barca assets list
barca assets show <id>
barca assets refresh <id>            # -j N for partition parallelism
barca sensors {list,show,trigger}
barca jobs {list,show}
barca run                            # production loop: maintain declared freshness
barca dev                            # dev mode: watch files, update staleness live
barca serve [--port 8400]            # HTTP API + background scheduler + UI
barca prune                          # drop unreachable history + artifacts
barca reset [--db] [--artifacts] [--tmp]
```

`barca reconcile` does **not** exist — use `barca run` (once or continuously)
or hit `POST /api/run/pass` on the server.

## Workspace layout

Single package at `packages/barca/`. The CLI and server live inside it as
submodules, not separate distributions (CLAUDE.md's three-package split is
out of date):

```
packages/barca/src/barca/
  __init__.py         # public API: asset, sensor, effect, cron, partitions, ...
  _asset.py           # @asset decorator + AssetWrapper
  _sensor.py          # @sensor decorator
  _effect.py          # @effect decorator (leaf node, never cached)
  _schedule.py        # cron(), CronSchedule, schedule eligibility
  _reconciler.py      # single-pass DAG walk
  _engine.py          # reindex / refresh / materialize / reset
  _inspector.py       # import modules, find decorated functions
  _trace.py           # AST dependency tracing
  _hashing.py         # codebase / definition / run hashes (SHA-256)
  _models.py          # all Pydantic models
  _store.py           # MetadataStore — pyturso (libSQL) wrapper
  _notebook.py        # load_inputs / materialize / read_asset / list_versions
  _config.py          # barca.toml parsing
  cli/                # typer app, table formatting
  server/             # FastAPI app, scheduler, SSE, React UI
    app.py            # routes are thin wrappers over service.py
    service.py        # pure business logic (no FastAPI deps)
    scheduler.py      # asyncio reconcile loop
    ui/               # React + Vite app (build with npm run build)
```

`packages/barca` is the only distribution. `examples/*` install it via
`tool.uv.sources = { barca = { path = "...", editable = true } }`.

## Node kinds, schedules, lifecycle

| Kind   | Decorator   | Default schedule | Cached             | Usable as input |
|--------|-------------|------------------|--------------------|-----------------|
| asset  | `@asset()`  | `"manual"`       | yes (by `run_hash`)| yes             |
| sensor | `@sensor()` | `"always"`       | no                 | yes             |
| effect | `@effect()` | `"always"`       | no                 | no (leaf only)  |

Schedules: `"manual"`, `"always"`, or `cron("0 5 * * *")`.

Lifecycle:
1. **index** — hash source + deps + protocol version → upsert into DB
2. **refresh** — materialize upstreams, call function, write
   `.barcafiles/{slug}/{definition_hash}/value.json`
3. **reconcile / run_pass** — topo-walk the DAG, run stale + eligible nodes
4. **cache** — identical `run_hash` → skip execution

## Design constraints to preserve

- Pure Python. No native extensions, no subprocess workers.
- Pydantic at all boundaries.
- Routes stay thin — business logic lives in `server/service.py`.
- Files should stay under ~500 lines; split otherwise.
- Protocol version is part of every hash; bump `PROTOCOL_VERSION` when the
  hashing contract changes (this invalidates all existing caches).
- `continuity_key` defaults to `{relative_file}:{function_name}`; `@asset(name=...)`
  pins it across renames — preserve this when editing the indexer.
- `@effect` functions cannot be inputs to anything.
- Notebook helpers auto-discover project root via `barca.toml`; don't hardcode
  CWD.

## Common gotchas

- **Pydantic + Python 3.14 beta**: any `TypeError: _eval_type() got an
  unexpected keyword argument 'prefer_fwd_module'` means you're on 3.14.0b4.
  Fix the interpreter, not pydantic.
- **UI 404**: `/ui/` 404s if `packages/barca/src/barca/server/ui/dist/` is
  missing. Run `npm run build`.
- **Turso lock**: only one process per `.barca/metadata.db`. Kill stale
  servers before starting a new one.
- **Turso vs stdlib sqlite**: the store uses `pyturso`, not `sqlite3`. Don't
  switch drivers casually; schema + connection semantics differ.
- **Free-threaded warnings**: the CLI prints a `GIL is enabled` warning when
  run under the non-free-threaded build. For serious work always use `3.14t`
  and set `PYTHON_GIL=0`.
