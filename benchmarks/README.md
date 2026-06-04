# Benchmarks

Compares Barca, Dagster, and Prefect on orchestration overhead and parallel throughput.

All measurements use [hyperfine](https://github.com/sharkdp/hyperfine). See [RESULTS.md](RESULTS.md) for the latest numbers.

## Methodology

### What's controlled
- **Same machine**: all benchmarks run on the same hardware
- **Same workload**: each benchmark implements identical logic across all three frameworks
- **Cold start**: every run starts a fresh process (no warm caches)
- **Parallel where applicable**: benchmarks with independent nodes enable parallelism in all frameworks

### What differs
- **Python version**: Barca uses Python 3.14 (from workspace .venv). Dagster and Prefect use Python 3.12 (latest compatible with both)
- **Parallelism model**: Barca uses multi-process (Rust spawns N Python workers). Dagster uses multiprocess executor. Prefect uses ConcurrentTaskRunner
- **DB persistence**: Barca writes to Turso/libSQL. Dagster and Prefect use their own internal SQLite stores

### Parallelism configuration
- **Barca**: pool_size = cpu_count (automatic)
- **Dagster**: `multiprocess_executor` with `max_concurrent = cpu_count` for parallel benchmarks
- **Prefect**: `ConcurrentTaskRunner` with `.submit()` for parallel benchmarks

## Benchmark suite

### Overhead & scaling

| Benchmark | Assets | Topology | Parallelism |
|---|---|---|---|
| `trivial` | 1 | single node | N/A |
| `chain_100` | 100 | linear chain | None (sequential) |
| `fan_out_500` | 500 | independent | All frameworks parallel |
| `fan_out_500_50ms` | 500 | independent + 50ms I/O | All frameworks parallel |
| `incremental_backfill` | 10 × 10 | linear chain × 10 runs | None (sequential) |
| `multi_file_discovery` | 98 | independent (50 files) | All frameworks parallel |

### DAG topologies

| Benchmark | Assets | Topology | Parallelism |
|---|---|---|---|
| `deep_diamond` | 18 | 5-wide → merge → chain | Parallel fan-out |
| `wide_layers` | 63 | 3 × 20 + aggregation | Parallel within layers |
| `map_reduce` | 52 | 1 → 50 → 1 | Parallel mappers |
| `wide_join` | 11 | 10 dims → 1 fact | Parallel dims |

### Real workloads

| Benchmark | Assets | Topology | Parallelism |
|---|---|---|---|
| `spaceflights` | 10 | diamond + sklearn | Parallel sources |
| `mixed_io_cpu` | 8 | 5 API calls → merge → compute | Parallel API calls |
| `etl_duckdb` | 12 | raw → staging → marts | Parallel sources |
| `large_payloads` | 5 | linear chain, 10k rows/step | None (sequential) |

## Running benchmarks

```bash
# Prerequisites: Rust toolchain, uv, hyperfine
cargo build --release && maturin develop --release

# Run individual benchmark (sets up venvs on first run):
benchmarks/trivial/bench.sh 10

# Or run manually with hyperfine:
hyperfine --warmup 3 --runs 10 benchmarks/trivial/*/run.sh
```

Each benchmark directory contains:
- `barca/` — barca implementation + `run.sh`
- `dagster/` — equivalent Dagster implementation + `run.sh`
- `prefect/` — equivalent Prefect implementation + `run.sh`
