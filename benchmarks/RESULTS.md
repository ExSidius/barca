# Benchmark Results

Last run: 2026-06-03 | Apple Silicon (M-series) | Rust release build

## Methodology

### Environment
- **Hardware**: Apple Silicon (M-series), same machine for all benchmarks
- **Barca**: Python 3.14.3 (workspace .venv), Rust release binary
- **Dagster**: Python 3.12.0 (latest compatible), dagster latest
- **Prefect**: Python 3.12.0, prefect latest
- **Airflow**: Python 3.12.0, apache-airflow latest (3.2.2)
- **Python version note**: dagster, prefect, and airflow do not yet support Python 3.14

### Execution modes tested

| Framework | Mode | Parallelism |
|---|---|---|
| **Barca** | multi-process | Rust spawns N Python workers (pool_size = cpu_count) |
| **Dagster** (script) | `materialize()` | Sequential in-process (only mode available in script mode) |
| **Dagster** (server) | `dagster dev` + GraphQL | multiprocess_executor spawns subprocess per asset |
| **Prefect** (sequential) | direct task calls | Sequential (default) |
| **Prefect** (parallel) | `ConcurrentTaskRunner` + `.submit()` | Thread-based concurrency (max_workers=16) |
| **Airflow** (dags test) | `dags test` | Sequential in-process (testing mode) |
| **Airflow** (LocalExecutor) | scheduler + trigger | Parallel subprocesses (requires PostgreSQL — SQLite locks under concurrency) |

### Measurement
- hyperfine with --warmup 3 for sub-100ms benchmarks (≥10 runs)
- hyperfine with --warmup 1 for longer benchmarks (3-5 runs)
- All times are wall-clock cold start (process spawn to exit)
- Server-mode benchmarks: server pre-started (warm), timing is trigger-to-completion

## Summary: script mode (cold start)

All frameworks invoked as scripts — no pre-started servers.

| # | Benchmark | barca | dagster | prefect | airflow |
|---|---|---:|---:|---:|---:|
| 1 | Trivial (1 asset) | **29ms** | 535ms | 3.9s | 3.5s |
| 2 | Chain 100 (linear) | **37ms** | 1.1s | 4.3s | 83s |
| 3 | Fan-out 500 (no work) | **87ms** | 2.3s | 4.0s | — |
| 4 | Fan-out 500×50ms | **1.9s** | 33.7s | 33.4s | 461s |
| 5 | Spaceflights (sklearn) | **574ms** | 1.2s | 3.9s | — |
| 6 | Deep Diamond (18 assets) | **83ms** | 678ms | 4.0s | 17s |
| 7 | Wide Layers (63 assets) | **167ms** | 919ms | 3.9s | — |
| 8 | Mixed I/O+CPU | **231ms** | 1.1s | 4.0s | — |
| 9 | Large Payloads (10k rows) | **203ms** | 631ms | 6.0s | — |
| 10 | Map/Reduce (1→50→1) | **89ms** | 917ms | 4.1s | — |
| 11 | Multi-file (50 files) | **60ms** | 911ms | 4.0s | — |
| 12 | ETL Pipeline (100k rows) | **604ms** | 953ms | 14.2s | — |
| 13 | Wide Join (10→1) | **58ms** | 635ms | 4.1s | — |
| 14 | Backfill (10-step × 10) | **282ms** | 6.2s | 40.1s | — |

*Airflow `dags test` has ~800ms per-task overhead. Airflow 3's LocalExecutor was tested with both SQLite and PostgreSQL — SQLite fails on concurrent writes, PostgreSQL fails because Airflow 3's task runner requires an API server connection (`httpx.ConnectError: Connection refused`). The `dags test` in-process mode is the only reliable local execution path for Airflow 3.x. Dashes indicate benchmarks not yet run for Airflow.*

## Resilience / anti-pileup (resilience_pileup)

Failure-path behavior, not happy-path overhead: 8 independent healthy 2-asset
chains + one *poison* chain whose head fails twice (with `retry_backoff=0.5s`)
before recovering. The question is whether one flaky asset stalls runnable work.

| Framework | Mode | Expected wall-clock |
|---|---|---|
| **Barca** | Rust-owned retries; healthy chains run in-process; backoff sits in a delay-queue (holds no worker slot) | ≈ `max(healthy work, total backoff ≈ 1.5s)` |
| Dagster | `materialize()` script mode (sequential) | ≈ `sum(work) + 1.5s backoff` |
| Prefect | direct task calls (sequential) | ≈ `sum(work) + 1.5s backoff` |

Run `benchmarks/resilience_pileup/bench.sh 5` (requires `hyperfine` + per-framework
`.venv`s) to populate measured numbers. The barca side is validated end-to-end;
competitor numbers depend on a machine with `dagster`/`prefect` installed.

## Parallel mode comparison (fan_out_500_50ms)

500 independent tasks, each sleeping 50ms. Sequential minimum: 25.0s. This is the benchmark where parallelism matters most.

| Framework | Mode | Time | Speedup vs sequential |
|---|---|---:|---:|
| **Barca** | 16 worker processes | **1.9s** | 13.2x |
| **Prefect** | ConcurrentTaskRunner (16 workers) | **4.2s** | 6.0x |
| Dagster | script mode (sequential) | 33.7s | 1.0x |
| Prefect | script mode (sequential) | 33.4s | 1.0x |
| Dagster | server + multiprocess_executor | 68.5s | 0.5x (slower than sequential!) |

### Why dagster server mode is slower

Dagster's `multiprocess_executor` spawns a **full Python subprocess per asset**. For 500 trivial tasks, the subprocess spawn overhead (~100ms each) exceeds the 50ms of actual work. This executor is designed for long-running, heavyweight assets — not 500 lightweight ones.

### Why prefect ConcurrentTaskRunner helps

Prefect's ConcurrentTaskRunner uses **threads** (not processes), avoiding subprocess spawn overhead. With `max_workers=16`, it achieves 6x speedup on I/O-bound tasks. However, per-task overhead (~6ms) still limits throughput vs barca's ~0.1ms per-task overhead.

## Reproducing

```bash
cargo build --release && maturin develop --release

# Script-mode benchmarks:
for bench in trivial chain_100 fan_out_500 fan_out_500_50ms spaceflights deep_diamond \
             wide_layers mixed_io_cpu large_payloads map_reduce multi_file_discovery \
             etl_duckdb wide_join incremental_backfill; do
  echo "=== $bench ==="
  hyperfine --warmup 1 --runs 3 benchmarks/$bench/*/run.sh
done

# Server-mode (dagster):
cd benchmarks/fan_out_500_50ms/dagster_server
DAGSTER_HOME=/tmp/dagster_bench .venv/bin/dagster dev -f definitions.py -p 3333 &
sleep 10  # wait for server
.venv/bin/python trigger.py
pkill -f "dagster dev"

# Parallel-mode (prefect):
benchmarks/fan_out_500_50ms/prefect_server/run.sh
```
