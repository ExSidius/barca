<p align="center">
  <h1 align="center">barca</h1>
  <p align="center"><strong>The invisible asset orchestrator.</strong><br/>Rust plans it. Python runs it. You just write functions.</p>
</p>

<p align="center">
  <a href="https://github.com/recursia-io/barca/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/recursia-io/barca/ci.yml?branch=main&style=flat-square&label=CI" /></a>
  <a href="https://pypi.org/project/barca/"><img alt="PyPI" src="https://img.shields.io/pypi/v/barca?style=flat-square&color=3572A5" /></a>
  <img alt="Python" src="https://img.shields.io/badge/python-%E2%89%A53.12-3572A5?style=flat-square" />
  <img alt="Rust" src="https://img.shields.io/badge/rust-2024_edition-dea584?style=flat-square" />
  <a href="https://github.com/recursia-io/barca/blob/main/LICENSE"><img alt="License" src="https://img.shields.io/github/license/recursia-io/barca?style=flat-square" /></a>
</p>

---

Barca is an asset orchestrator that adds **zero perceptible overhead** to your Python pipelines. A compiled Rust binary handles parsing, DAG construction, and execution planning. Python does what it's best at: running your code.

```python
# pipeline.py
from barca import asset

@asset()
def raw_data() -> list[dict]:
    return [{"x": 1}, {"x": 2}, {"x": 3}]

@asset(inputs={"data": raw_data})
def summary(data: list[dict]) -> dict:
    return {"count": len(data), "total": sum(d["x"] for d in data)}
```

```
$ barca get pipeline.py
{"elapsed_seconds":0.042,"steps_executed":2,"phases":2,"final_output":{"count":3,"total":6}}
```

No config files. No YAML. No daemon. Just functions and a fast binary.

## Install

Barca is designed for use with [uv](https://docs.astral.sh/uv/):

```bash
uv add barca
```

This gives you:
- The `barca` CLI binary (compiled Rust)
- Python API: `barca.get()`, `barca.plan()`
- Decorator stubs for `@asset`, `@sensor`, `@effect` (IDE autocomplete + type checking)

For optional parquet (DataFrame) support:

```bash
uv add 'barca[parquet]'
```

All in one wheel, built with [maturin](https://www.maturin.rs/). Requires Python >= 3.12.

### From source

```bash
git clone https://github.com/ExSidius/barca.git
cd barca
uv sync
cargo build --release
maturin develop --release    # installs into current .venv
```

## Quick start

```python
# assets.py
from barca import asset

@asset()
def hello() -> dict:
    return {"message": "Hello from barca!"}
```

```bash
barca get assets.py
```

That's it. Barca parses your Python source with [ruff](https://github.com/astral-sh/ruff)'s AST parser (no import, pure static analysis), builds a dependency graph, generates a phased execution plan, spawns Python workers, and persists results to a local SQLite database -- all in under 40ms for a trivial asset.

## How it works

```
                    ┌─────────────────────────────────────┐
                    │          barca get pipeline.py       │
                    └──────────────┬──────────────────────┘
                                   │
                    ┌──────────────▼──────────────────────┐
                    │         Rust binary (barca)          │
                    │                                      │
                    │  1. Parse Python source (ruff AST)   │
                    │  2. Build DAG (petgraph)              │
                    │  3. Generate execution plan           │
                    │  4. Initialize DB (.barca/metadata.db)│
                    │  5. Spawn Python workers per phase    │
                    │  6. Collect outputs, persist to DB    │
                    └──────────────┬──────────────────────┘
                                   │
                    ┌──────────────▼──────────────────────┐
                    │      Python worker (per phase)       │
                    │                                      │
                    │  - Loads modules via importlib        │
                    │  - Executes steps in tier order       │
                    │  - LRU cache for in-process results   │
                    │  - Emits JSON lines to stdout         │
                    └─────────────────────────────────────┘
```

**Key design decisions:**

- **Static analysis only** -- Rust never imports your Python code. It parses source text and extracts decorator metadata from the AST.
- **Phased execution** -- The planner decomposes the DAG into sequential phases. Within each phase, independent streams run in parallel workers.
- **No framework lock-in** -- Decorators are identity functions. Your code runs standalone without barca installed.
- **Single binary** -- One `pip install` gives you everything. No JVM, no Docker, no scheduler service.

## Decorators

```python
from barca import asset, sensor, effect, sink, unsafe
from barca import Always, Manual, Schedule
from barca import partitions, partitions_from, collect, asset_ref
```

### `@asset`

Cached computation node. The workhorse.

```python
@asset()
def prices() -> dict:
    return {"AAPL": 150, "MSFT": 380}

@asset(inputs={"data": prices})
def report(data: dict) -> str:
    return f"Tracked {len(data)} tickers"
```

### `@sensor`

Observes external state. Returns `(update_detected, output)`.

```python
@sensor()
def inbox_files() -> tuple[bool, list[str]]:
    files = list(Path("inbox").glob("*.csv"))
    return bool(files), [str(f) for f in files]
```

### `@effect`

Side-effect leaf node. Never cached, can't be used as input.

```python
@effect(inputs={"report": report})
def publish(report: str) -> None:
    print(f"Publishing: {report}")
```

### `@sink`

Stacks on `@asset` to write outputs to files.

```python
@asset()
@sink("output/data.json", serializer="json")
def my_data() -> dict:
    return {"rows": 42}
```

### Freshness markers

| Marker | Behavior |
|--------|----------|
| `Always` | Auto-materializes whenever stale (default for `@effect`) |
| `Manual` | Only runs on explicit refresh |
| `Schedule("0 5 * * *")` | Cron expression |

### Partitions

Fan a single asset definition into N independent materializations:

```python
@asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
def prices(ticker: str) -> dict:
    return {"ticker": ticker, "price": get_price(ticker)}
```

| Function | Purpose |
|----------|---------|
| `partitions(values)` | Static list of partition keys |
| `partitions_from(source)` | Derive partitions from upstream asset |
| `collect(asset_fn)` | Aggregate all partitions of an upstream |
| `asset_ref(ref_string)` | Canonical asset reference |

## CLI

```
barca get [target] <file.py> [file.py ...] Get asset(s) — cache-aware
barca plan <file.py> [file.py ...]         Emit execution plan as JSON
barca --help                               Show help
```

Shorthand: `barca pipeline.py` works as `barca get pipeline.py` (all assets).

## Python API

```python
import barca

# Get all assets in a file (returns the last asset's value)
value = barca.get("pipeline.py")
print(value)  # {"count": 3, "total": 6}

# Get a specific asset's value (cache-aware)
value = barca.get("summary", "pipeline.py")
print(value)  # {"count": 3, "total": 6}

# Inspect the execution plan
plan = barca.plan("pipeline.py")
print(plan["total_steps"])  # 2
```

All output formats work transparently: dicts, lists, sets, DataFrames, and arbitrary Python objects are serialized as JSON, pickle, or parquet and deserialized automatically.

### `barca plan` -- inspect without running

```bash
$ barca plan pipeline.py
{
  "total_steps": 2,
  "phases": [
    { "reason": "Independent", "streams": [{"stream_id": 0, "steps": ["raw_data"]}] },
    { "reason": "Dependent",   "streams": [{"stream_id": 1, "steps": ["summary"]}] }
  ]
}
```

### `barca get` -- execute and get results

Parses source, builds DAG, spawns workers, collects outputs, persists to `.barca/metadata.db`. With a target, only the target's subgraph runs. Without a target, all assets run.

Output is a JSON summary:

```json
{
  "elapsed_seconds": 0.042,
  "steps_executed": 2,
  "phases": 2,
  "final_output": {"count": 3, "total": 6}
}
```

Use `--no-cache` to skip cache lookups and execute everything fresh.

Diagnostics go to stderr:

```
[barca] 2 nodes, 1 edges, 2 phases, 2 streams | plan: 1.2ms | exec: 38ms | total: 40ms
```

## Benchmarks

All benchmarks measured with [hyperfine](https://github.com/sharkdp/hyperfine) (3 warmup runs, 10 measured runs) on the same machine. Barca is compared against Dagster and Prefect running equivalent pipelines.

### Trivial (1 asset, zero work)

Measures pure framework overhead -- how long it takes to do nothing.

| Framework | Mean | Relative |
|-----------|------|----------|
| **barca** | **38.0 ms** | **1.00x** |
| dagster | 538.1 ms | 14.2x |
| prefect | 3977.7 ms | 104.7x |

Barca's total overhead (parse + plan + spawn + persist) is **38ms**. Dagster needs ~0.5s. Prefect needs ~4s.

### Benchmark suite

The `benchmarks/` directory contains 12 scenarios covering a range of DAG topologies and workloads:

| Benchmark | Assets | Topology | What it tests |
|-----------|--------|----------|--------------|
| `trivial` | 1 | single node | Pure framework overhead |
| `chain_100` | 100 | linear chain | Sequential dependency resolution |
| `fan_out_500` | 500 | flat (independent) | Wide parallelism, process spawning |
| `fan_out_500_50ms` | 500 | flat + 50ms sleep | Parallelism under I/O latency |
| `deep_diamond` | 18 | diamond (5-wide, 6-deep) | Fan-out/fan-in patterns |
| `wide_layers` | varies | parallel layers | Tier-based parallel execution |
| `large_payloads` | varies | varied | JSON serialization overhead |
| `map_reduce` | varies | map-reduce | Scatter-gather pattern |
| `mixed_io_cpu` | varies | varied | Mixed I/O and CPU workloads |
| `multi_file_discovery` | varies | multi-file | Cross-file asset discovery |
| `iris_pipeline` | varies | diamond | ML pipeline (iris dataset) |
| `spaceflights` | 10 | diamond (3-wide, 6-deep) | Full ML pipeline (Kedro-style) |

Run any benchmark:

```bash
cd benchmarks/trivial
./bench.sh 10    # 10 measured runs
```

Each benchmark includes equivalent Dagster and Prefect implementations for apples-to-apples comparison.

## Architecture

```
Cargo.toml                  Rust workspace root
crates/
  barca-core/               Engine: parser, DAG, planner, dispatch, DB, cache
  barca-cli/                Thin CLI shell (clap → barca-core)
python/barca/
  __init__.py               Decorator stubs + API exports
  api.py                    Python API (get/plan via subprocess)
  _worker.py                Execution worker (invoked by Rust binary)
  _artifacts.py             Artifact serialization (json/pickle/parquet)
  py.typed                  PEP 561 marker
pyproject.toml              Maturin build config
```

### Tech stack

| Layer | Technology |
|-------|-----------|
| Parser | [ruff](https://github.com/astral-sh/ruff) Python AST (static, no import) |
| DAG | [petgraph](https://github.com/petgraph/petgraph) |
| Database | [Turso/libSQL](https://turso.tech/) (local SQLite) |
| Serialization | [serde](https://serde.rs/) + serde_json |
| Hashing | SHA-256 (content-addressed artifacts) |
| Build | [maturin](https://www.maturin.rs/) (Rust binary + Python stubs in one wheel) |
| Python runtime | Python >= 3.12 |

### Node kinds

| Kind | Decorator | Cached | Can be input |
|------|-----------|--------|-------------|
| **asset** | `@asset()` | Yes | Yes |
| **sensor** | `@sensor()` | No | Yes |
| **effect** | `@effect()` | No | No (leaf) |

## Development

```bash
git clone https://github.com/recursia-io/barca.git
cd barca

# Build
cargo build --release
maturin develop --release

# Test
cargo test

# Run
barca get examples/basic_app/example_project/assets.py
barca plan examples/basic_app/example_project/assets.py
```

## Project status

Barca is in active development. The core pipeline (parse -> DAG -> plan -> execute -> persist) is working and benchmarked. See the [guide](./docs/guide.md) for a walkthrough.

## License

[MIT](./LICENSE)
