---
title: Development Setup
description: How to install and develop barca from source.
---

How to install and develop barca from source.

## Quick start

```bash
git clone https://github.com/ExSidius/barca.git
cd barca
uv sync                              # Python dev deps (pytest, etc.)
cargo build --release                # build the Rust binary
maturin develop --release            # install binary + Python stubs into .venv
cargo test                           # run the Rust test suite
```

## Running an example

```bash
.venv/bin/barca get examples/basic_app/example_project/assets.py    # materialize all assets
.venv/bin/barca plan examples/basic_app/example_project/assets.py   # inspect the execution plan
```

## How it works

Barca is a Cargo workspace with a Python package layered on top:

| Crate/Package | Path | Purpose |
|---------|------|---------|
| `barca-core` | `crates/barca-core/` | Core library — parser, DAG, execution planning, hashing |
| `barca-cli` | `crates/barca-cli/` | CLI binary (the `barca` command) |
| `barca-server` | `crates/barca-server/` | HTTP API + cron scheduler, used by `barca serve` |
| `barca` (Python) | `python/barca/` | No-op decorator stubs, the batch worker (`_worker.py`), and artifact serialization |

`maturin develop --release` builds the Rust binary and installs it, plus the Python stubs,
into `.venv` in one step — this is what `pyproject.toml`'s `[tool.maturin]` config drives.

## Running tests

```bash
# Rust unit + integration tests (crates/*/tests/ and inline #[test] modules)
cargo test

# Python tests (worker, runtime, storage backends)
uv run pytest python/tests -q
```

CI additionally runs shell-based integration tests against the built wheel:
`tests/integration/*.sh` (CLI behavior, caching, environments, remote state).

## Project structure

```
barca/
├── Cargo.toml                # Rust workspace root
├── crates/
│   ├── barca-core/           # Core library: models, parser, DAG, planning, hashing
│   ├── barca-cli/            # CLI binary
│   └── barca-server/         # HTTP server + cron scheduler
├── python/
│   ├── barca/                # Decorator stubs, worker, artifact I/O
│   └── tests/                # Python tests (pytest)
├── examples/                 # Example projects
├── benchmarks/               # Performance benchmarks
├── tests/integration/        # Shell-based CLI integration tests
├── pyproject.toml            # Maturin build config
└── site/                     # This documentation site
```
