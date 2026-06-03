# Benchmark Results

Last run: 2026-06-03 | Apple Silicon (M-series) | Rust release build

## Methodology

### Environment
- **Hardware**: Apple Silicon (M-series), same machine for all benchmarks
- **Barca**: Python 3.14.3 (from workspace .venv), Rust release binary
- **Dagster**: Python 3.12.0 (latest compatible version), dagster latest
- **Prefect**: Python 3.12.0, prefect latest
- **Python version difference**: disclosed — dagster and prefect do not yet support Python 3.14

### Parallelism
- **Barca**: multi-process dispatch (Rust spawns N Python worker processes, one per stream)
- **Dagster**: `materialize()` API runs single-threaded in-process. Dagster's multiprocess executor requires a running dagster instance (server mode), which is a fundamentally different deployment model. Script-mode benchmarks (cold start) are inherently sequential.
- **Prefect**: `ConcurrentTaskRunner` with `.submit()` tested for benchmarks with ≤18 independent tasks. For >50 tasks, prefect's internal SQLite state tracking crashes under concurrent load. For the tested parallel benchmarks, the per-task overhead (~150ms) exceeded any parallelism benefit.

### What this means
Barca's parallelism advantage is real but partly architectural: barca was designed for multi-process dispatch from the start. Dagster and Prefect were designed around server-mode execution where parallelism is handled by the server's executor pool. In cold-start script mode, both run sequentially. This is a genuine user-facing difference — barca is faster for script-invoked pipelines.

### Measurement
- hyperfine with --warmup 3 for sub-100ms benchmarks (≥10 runs)
- hyperfine with --warmup 1 for longer benchmarks (3-5 runs)
- All times are wall-clock cold start (process spawn to exit)

## Summary

| # | Benchmark | barca | dagster | prefect | vs dagster |
|---|---|---:|---:|---:|---:|
| 1 | Trivial (1 asset) | **29ms** | 535ms | 3.9s | 18.7x |
| 2 | Chain 100 (linear) | **37ms** | 1.1s | 4.3s | 29.8x |
| 3 | Fan-out 500 (independent) | **87ms** | 2.3s | 4.0s | 26.8x |
| 4 | Fan-out 500×50ms (parallel I/O) | **1.9s** | 33.6s | 33.5s | 17.7x |
| 5 | Spaceflights (diamond, sklearn) | **574ms** | 1.2s | 3.9s | 2.1x |
| 6 | Deep Diamond (18 assets, 5-wide) | **83ms** | 678ms | 4.0s | 8.2x |
| 7 | Wide Layers (63 assets, 3×20) | **167ms** | 919ms | 3.9s | 5.5x |
| 8 | Mixed I/O+CPU | **231ms** | 1.1s | 4.0s | 4.6x |
| 9 | Large Payloads (10k rows/step) | **203ms** | 631ms | 6.0s | 3.1x |
| 10 | Map/Reduce (1→50→1) | **89ms** | 917ms | 4.1s | 10.3x |
| 11 | Multi-file (50 files, 100 assets) | **60ms** | 911ms | 4.0s | 15.1x |
| 12 | ETL Pipeline (100k rows) | **604ms** | 953ms | 14.2s | 1.6x |
| 13 | Wide Join (10→1) | **58ms** | 635ms | 4.1s | 10.9x |
| 14 | Backfill (10-step × 10 runs) | **282ms** | 6.2s | 40.1s | 22.1x |

### Parallelism note

Benchmarks 3, 4, 6, 7, 8, 10, 11, 13 have independent nodes that barca parallelizes across multiple processes. Dagster and Prefect run these sequentially in script mode (their parallelism requires server-mode deployment). Benchmarks 1, 2, 5, 9, 12, 14 are sequential by nature — no parallelism advantage for any framework.

For the sequential-only benchmarks, barca's advantage comes purely from lower framework overhead (Rust planning + minimal Python import time).

## Reproducing

```bash
cargo build --release && maturin develop --release

# Set up competitor venvs (one time):
for bench in benchmarks/*/dagster; do
  uv venv --python 3.12 "$bench/.venv" --clear
  uv pip install --python "$bench/.venv/bin/python" dagster
done
for bench in benchmarks/*/prefect; do
  uv venv --python 3.12 "$bench/.venv" --clear
  uv pip install --python "$bench/.venv/bin/python" prefect
done

# Benchmarks with sklearn need it:
uv pip install --python benchmarks/spaceflights/dagster/.venv/bin/python scikit-learn
uv pip install --python benchmarks/spaceflights/prefect/.venv/bin/python scikit-learn

# Run all:
for bench in trivial chain_100 fan_out_500 spaceflights deep_diamond wide_layers mixed_io_cpu large_payloads map_reduce multi_file_discovery etl_duckdb wide_join incremental_backfill; do
  echo "=== $bench ==="
  hyperfine --warmup 1 --runs 3 benchmarks/$bench/*/run.sh
done
```
