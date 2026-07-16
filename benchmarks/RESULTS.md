# Benchmark Results

Last run: 2026-06-10 | Apple Silicon (M-series) | Rust release build | v0.2.0

> **Note (2026-07-16):** the benchmark harness was since standardized for CPU/worker
> fairness — every framework's parallelism is now pinned to the same core count and
> worker count instead of Barca auto-detecting `cpu_count` while Dagster/Prefect
> hardcoded 16 workers (see `benchmarks/README.md#methodology` and
> `benchmarks/lib/env.sh`). The numbers below predate that change and should be
> treated as illustrative, not reproducible as-is; re-run via `bench.sh` for
> current, standardized numbers.
>
> The three benchmarks with an existing `bench.sh` (`trivial`, `parallel_tasks`,
> `resilience_pileup`) were re-run under the new harness — see
> [Standardized re-run (2026-07-16)](#standardized-re-run-2026-07-16) below. The
> remaining benchmarks in this file still need a re-run on real hardware; they
> weren't re-run because this pass ran in a shared, virtualized CI-style
> container (not the dedicated Apple Silicon box the original numbers came
> from), so absolute times aren't comparable across the two environments — only
> the barca/dagster/prefect ratios *within* the same run are meaningful.

## Standardized re-run (2026-07-16)

Ran on this container: 4 vCPU (Intel Xeon @ 2.80GHz, pinned via `taskset -c 0-3`),
15 GiB RAM, barca Rust release build, Python 3.13 (barca) / 3.12 (Dagster, Prefect).
Worker count = 4 for all three frameworks (`BARCA_BENCH_CORES` default). Absolute
times are inflated by container overhead (cold PyPI-installed venvs, virtualized
CPU, no dedicated hardware) — read the **relative** ratios, not the raw ms/s, and
don't compare these numbers to the Apple Silicon rows above.

| Benchmark | barca | dagster | prefect | barca vs dagster | barca vs prefect |
|---|---:|---:|---:|---:|---:|
| Trivial (1 asset, 10 runs) | 499ms ± 242ms | 2.19s ± 0.08s | 11.35s ± 0.19s | 4.4x faster | 22.8x faster |
| Parallel fan-out (10 tasks, 5 runs) | 1.24s ± 0.02s | 1.88s ± 0.03s | 10.94s ± 0.32s | 1.5x faster | 8.8x faster |
| Resilience pileup (18 steps, 5 runs) | 3.00s ± 0.80s | 3.57s ± 0.06s | 12.02s ± 0.38s | 1.2x faster | 4.0x faster |

Notes from this run:
- Barca's trivial and resilience_pileup numbers show high variance (σ up to 27% of
  the mean) with hyperfine flagging statistical outliers — consistent with
  container scheduling noise rather than anything in barca itself (dagster/prefect
  show low variance on the same box since their per-run cost is dominated by fixed
  Python import/framework overhead, which swamps scheduler jitter).
- Fixed two pre-existing bugs uncovered while running this, unrelated to the
  CPU/RAM standardization work: `parallel_tasks/{barca,dagster,prefect}/run.sh`
  called bare `barca`/`python` instead of the pinned venv binaries (worked only if
  a venv happened to already be active on `PATH`); `resilience_pileup/barca/run.sh`
  was missing `--no-cache`, so every run after the first was a cache-hit no-op
  (measured: 2.5s cold vs 8ms warm) — the historical "330ms" figure above is
  suspect for the same reason. `resilience_pileup/prefect/run.py` had a variable
  name collision (`t0` was both a `@task` and the `__main__` timer, so every run
  raised `TypeError: 'float' object is not callable`) — always broken, not a
  regression from this change.

## Methodology

### Environment
- **Hardware**: Apple Silicon (M-series), same machine for all benchmarks
- **Barca**: Python 3.14.3 (workspace .venv), Rust release binary
- **Dagster**: Python 3.12.0 (latest compatible), dagster latest
- **Prefect**: Python 3.12.0, prefect latest
- **Airflow**: Python 3.12.0, apache-airflow latest (3.2.2)
- **Python version note**: dagster, prefect, and airflow do not yet support Python 3.14

### Architecture (v0.2.0)

Barca uses a **stateless worker pool** with **Unix domain sockets** for coordination:
- Rust owns a global ready queue and assigns one task at a time to idle workers
- Workers receive tasks, execute, report back — no pre-assigned queues
- `parallel()` uses **SIGSTOP/SIGCONT** to freeze the requesting worker, spawn a temp replacement, and dispatch children across the pool
- Nested `parallel()` works recursively (frozen processes stack, active pool stays at `pool_size`)

### Execution modes tested

| Framework | Mode | Parallelism |
|---|---|---|
| **Barca** | multi-process | Rust spawns N Python workers via UDS (pool_size = cpu_count) |
| **Dagster** (script) | `materialize()` | Sequential in-process (only mode available in script mode) |
| **Dagster** (server) | `dagster dev` + GraphQL | multiprocess_executor spawns subprocess per asset |
| **Prefect** (sequential) | direct task calls | Sequential (default) |
| **Prefect** (parallel) | `ConcurrentTaskRunner` + `.submit()` | Thread-based concurrency (max_workers=16) |
| **Airflow** (dags test) | `dags test` | Sequential in-process (testing mode) |
| **Airflow** (LocalExecutor) | scheduler + trigger | Parallel subprocesses (requires PostgreSQL) |

### Measurement
- hyperfine with --warmup 2-3 for sub-100ms benchmarks (5-10 runs)
- hyperfine with --warmup 1 for longer benchmarks (3-5 runs)
- All times are wall-clock cold start (process spawn to exit)

## Summary: script mode (cold start)

All frameworks invoked as scripts — no pre-started servers.

| # | Benchmark | barca | dagster | prefect | airflow |
|---|---|---:|---:|---:|---:|
| 1 | Trivial (1 asset) | **38ms** | 500ms | 3.6s | 3.5s |
| 2 | Chain 100 (linear) | **72ms** | 999ms | 3.7s | 83s |
| 3 | Fan-out 500 (no work) | **439ms** | 2.1s | 4.3s | — |
| 4 | Fan-out 500×50ms | **2.2s** | 31.5s | 31.2s | 461s |
| 5 | Spaceflights (sklearn) | **720ms** | 1.1s | 3.8s | — |
| 6 | Deep Diamond (18 assets) | **159ms** | 678ms | 4.0s | 17s |
| 7 | Wide Layers (63 assets) | **1.2s** | 919ms | 3.9s | — |
| 8 | Mixed I/O+CPU | **334ms** | 1.1s | 4.0s | — |
| 9 | Large Payloads (10k rows) | **210ms** | 631ms | 6.0s | — |
| 10 | Map/Reduce (1→50→1) | **443ms** | 917ms | 4.1s | — |
| 11 | Multi-file (50 files) | **60ms** | 911ms | 4.0s | — |
| 12 | ETL Pipeline (100k rows) | **500ms** | 953ms | 14.2s | — |
| 13 | Wide Join (10→1) | **267ms** | 635ms | 4.1s | — |
| 14 | Backfill (10-step × 10) | **282ms** | 6.2s | 40.1s | — |
| 15 | Partitioned Chain (3×50) | **1.2s** | — | — | — |
| 16 | Resilience Pileup | **330ms** | — | — | — |
| 17 | parallel(1000) tasks | **1.7s** | — | — | — |
| 18 | Nested parallel (3×5) | **650ms** | — | — | — |

*Rows 5-14 dagster/prefect numbers from 2026-06-03 run; barca numbers updated 2026-06-10.*

## Internal performance profile

Profiler thresholds (all pass):

| Check | Measured | Threshold |
|---|---:|---:|
| Trivial (total) | 31ms | <100ms |
| Plan 100 nodes | 5.6ms | <50ms |
| Plan 2002 nodes | 21ms | <500ms |
| Per-step overhead | 0.40ms | <1.0ms |

### Timing breakdown (trivial, 1 asset)

| Phase | Time |
|---|---:|
| Rust parse + plan | 4ms |
| Python process spawn | ~18ms |
| UDS connect + execute + serialize | ~5ms |
| DB init + write | ~5ms |
| **Total** | **32ms** |

Python process startup is the dominant fixed cost. For workloads with >10 steps, it amortizes to <0.6ms/step.

## Parallel mode comparison (fan_out_500_50ms)

500 independent tasks, each sleeping 50ms. Sequential minimum: 25.0s.

| Framework | Mode | Time | Speedup vs sequential |
|---|---|---:|---:|
| **Barca** | 16 worker processes (UDS) | **2.2s** | 11.4x |
| **Prefect** | ConcurrentTaskRunner (16 workers) | **4.2s** | 6.0x |
| Dagster | script mode (sequential) | 31.5s | 1.0x |
| Prefect | script mode (sequential) | 31.2s | 1.0x |
| Dagster | server + multiprocess_executor | 68.5s | 0.5x (slower than sequential!) |

### Dynamic parallel dispatch (v0.2.0)

`parallel()` at runtime — dispatches tasks dynamically, not known at plan time.

| N items | Time | Notes |
|---:|---:|---|
| 10 | 300ms | includes process spawn overhead |
| 200 | 530ms | previously hung in v0.1.x |
| 1000 | 1.7s | 1000 unique results, SIGSTOP/SIGCONT |
| Nested 3×5 | 650ms | 3 outer × 5 inner = 15 leaf tasks |

## Reproducing

```bash
cargo build --release && maturin develop --release

# Script-mode benchmarks:
for bench in trivial chain_100 fan_out_500 fan_out_500_50ms spaceflights deep_diamond \
             wide_layers mixed_io_cpu large_payloads map_reduce multi_file_discovery \
             etl_duckdb wide_join incremental_backfill partitioned_chain resilience_pileup; do
  echo "=== $bench ==="
  hyperfine --warmup 1 --runs 3 benchmarks/$bench/*/run.sh
done

# Performance profiler:
python benchmarks/perf/profile.py

# Parallel tasks:
barca run fan_out_1000 /tmp/parallel_1000.py --agent
```
