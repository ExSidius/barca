# Development Setup

How to install and develop barca from source.

## Quick start

```bash
git clone https://github.com/ExSidius/barca.git
cd barca
uv sync                              # install all workspace packages + dev deps
uv run pytest tests/ -v              # run tests
```

## Running an example

```bash
cd examples/basic_app && uv sync
uv run barca reindex
uv run barca assets list
uv run barca assets refresh 1
```

Or the iris ML pipeline:

```bash
cd examples/iris_pipeline && uv sync
uv run barca reindex
uv run barca assets refresh 1        # cascades all upstream deps
```

## How it works

Barca is a uv workspace with three Python packages:

| Package | Path | Purpose |
|---------|------|---------|
| `barca` | `packages/barca-core/` | Core library — decorators, models, store, engine |
| `barca-cli` | `packages/barca-cli/` | CLI — typer app |
| `barca-server` | `packages/barca-server/` | HTTP API — FastAPI + uvicorn (optional) |

`uv sync` at the workspace root installs all three packages in development mode. The CLI entry point `barca` is provided by `barca-cli`.

## Running tests

```bash
# Full test suite
uv run pytest tests/ -v

# Specific test file
uv run pytest tests/test_sensor.py -v

# Just server tests (requires niquests)
uv run pytest tests/test_server.py -v
```

## Free-threaded Python

Barca defaults to Python 3.13t (free-threaded, GIL disabled) via `.python-version`. This enables true thread parallelism for partitioned assets.

To opt out and use standard Python:

```bash
echo "3.13" > .python-version
uv sync
```

## Project structure

```
barca/
├── packages/
│   ├── barca-core/          # Core library
│   ├── barca-cli/           # CLI tool
│   └── barca-server/        # HTTP server (optional)
├── tests/                   # All tests (pytest)
├── examples/                # Example projects
├── benchmarks/              # Performance benchmarks
├── docs/                    # Documentation and specs
├── pyproject.toml           # Workspace root
└── .python-version          # 3.13t (free-threaded)
```
