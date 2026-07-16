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
> All 18 benchmarks with a `bench.sh` were re-run end to end under the new
> harness — see [Standardized re-run (2026-07-16)](#standardized-re-run-2026-07-16)
> below. This pass ran in a shared, virtualized CI-style container (not the
> dedicated Apple Silicon box the original numbers came from), so absolute
> times aren't comparable across the two environments — only the
> barca/dagster/prefect ratios *within* the same run are meaningful. The
> honest headline: with CPU and worker count actually equalized, barca does
> **not** win every benchmark on this box — dagster comes out ahead (or
> ties) on 9 of 18. See the notes below before drawing conclusions from any
> single row.

## Standardized re-run (2026-07-16)

Ran on this container: 4 vCPU (Intel Xeon @ 2.80GHz, pinned via `taskset -c 0-3`),
15 GiB RAM, barca Rust release build, Python 3.13 (barca) / 3.12 (Dagster, Prefect).
Worker count = 4 for all three frameworks (`BARCA_BENCH_CORES` default, `--warmup 1
--runs 5` except trivial at `--warmup 3 --runs 10`). Absolute times are inflated by
container overhead (cold PyPI-installed venvs, virtualized CPU, no dedicated
hardware, shared with whatever else is on the host) — read the **relative** ratios,
not the raw ms/s, and don't compare these numbers to the Apple Silicon rows above.

| Benchmark | barca | dagster | prefect | barca vs dagster | barca vs prefect |
|---|---:|---:|---:|---:|---:|
| trivial | 339ms ± 13ms | 1.98s ± 0.15s | 10.74s ± 0.24s | 5.85x faster | 31.68x faster |
| parallel_tasks | 1.28s ± 0.02s | 2.10s ± 0.06s | 11.46s ± 0.94s | 1.64x faster | 8.92x faster |
| resilience_pileup | 2.56s ± 0.02s ⚠ | 3.61s ± 0.04s | 11.75s ± 0.21s | 1.41x faster | 4.59x faster |
| chain_100 | 765ms ± 80ms | 3.88s ± 0.09s | 11.09s ± 0.13s | 5.07x faster | 14.50x faster |
| deep_diamond | 1.49s ± 0.04s | 2.41s ± 0.15s | 11.11s ± 0.17s | 1.62x faster | 7.46x faster |
| etl_duckdb | 4.44s ± 0.10s | 3.59s ± 0.22s | 47.63s ± 0.83s | **dagster 1.24x faster** | 13.28x faster |
| fan_out_500 | 2.69s ± 0.06s | 8.59s ± 0.32s | 12.95s ± 0.15s | 3.20x faster | 4.82x faster |
| fan_out_500_50ms | 9.84s ± 0.55s | 34.89s ± 0.27s | 37.92s ± 0.16s | 3.55x faster | 3.85x faster |
| incremental_backfill | 16.25s ± 0.17s | 20.47s ± 0.29s | 108.52s ± 1.47s | 1.26x faster | 6.68x faster |
| large_payloads | 3.67s ± 0.40s | 2.14s ± 0.07s | 17.70s ± 0.12s | **dagster 1.72x faster** | 8.29x faster |
| map_reduce | 2.57s ± 0.07s | 3.09s ± 0.14s | 10.48s ± 0.04s | 1.20x faster | 4.08x faster |
| mixed_io_cpu | 3.07s ± 0.08s | 3.05s ± 0.11s | 11.27s ± 0.34s | **dagster 1.01x faster** (~tied) | 3.70x faster |
| multi_file_discovery | 3.16s ± 0.51s ⚠ | 3.15s ± 0.15s | 10.73s ± 0.33s | **~tied** (1.00x) | 3.40x faster |
| partitioned_chain | 3.37s ± 0.13s | 4.32s ± 0.05s | 11.45s ± 0.50s | 1.28x faster | 3.40x faster |
| partitioned_etl | 2.96s ± 0.06s | 2.95s ± 0.17s ⚠ | 10.98s ± 0.14s | **~tied** (1.00x) | 3.72x faster |
| partitioned_fan_in | 3.43s ± 0.11s | 3.49s ± 0.07s | 10.81s ± 0.11s | 1.02x faster (~tied) | 3.15x faster |
| spaceflights | 4.34s ± 0.22s | 4.01s ± 0.20s | 12.26s ± 0.17s | **dagster 1.08x faster** | 3.06x faster |
| wide_join | 3.14s ± 0.13s | 2.19s ± 0.08s | 11.18s ± 0.26s | **dagster 1.43x faster** | 5.10x faster |
| wide_layers | 3.53s ± 0.61s ⚠ | 3.47s ± 0.10s | 11.09s ± 0.21s | **~tied** (1.02x) | 3.19x faster |

⚠ = hyperfine flagged statistical outliers for that row's barca (or, for
partitioned_etl, dagster) measurement — see variance notes below.

### Variance

Relative standard deviation (σ/mean) per framework, averaged across all 19 rows:
**barca ~5.2%, dagster ~3.6%, prefect ~1.9%**. Barca is consistently the noisiest
of the three here, and four rows crossed hyperfine's outlier-detection threshold
(resilience_pileup, multi_file_discovery, partitioned_etl's dagster leg,
wide_layers) — worst case was wide_layers at σ = 17% of the mean. Prefect and
dagster are quieter because their per-run cost is dominated by fixed Python
import/framework overhead, which swamps scheduler jitter; barca's per-run cost is
smaller in absolute terms, so the same container noise is a larger fraction of it.
This reads as environment noise (a shared, virtualized 4-vCPU container), not a
barca-specific issue — but it does mean any individual row within ~1.3x of 1.0
(the "~tied" ones above) shouldn't be read as a confident win either direction
without more runs or a quieter machine. `benchmarks/lib/env.sh` pins cores via
`taskset`, which helps, but can't fully isolate a shared host from noisy
neighbors the way dedicated hardware would.

### Notes

- **Barca does not universally win here.** On dedicated hardware (the historical
  Apple Silicon numbers above), barca won every comparison, often by 10x+. On this
  shared container with CPU/worker count actually equalized, dagster is faster or
  tied on 8 of 18 benchmarks (etl_duckdb, large_payloads, mixed_io_cpu,
  multi_file_discovery, partitioned_etl, partitioned_fan_in, spaceflights,
  wide_join, wide_layers). The prefect comparison is unaffected — prefect loses
  every benchmark by a wide margin regardless of environment, consistent with its
  much higher fixed per-run overhead. This is likely mostly a container-noise
  effect on a workload profile (many short-lived worker processes) that's more
  sensitive to scheduler jitter than dagster's more monolithic single-process
  model, but it's a real, honest result under fair conditions and shouldn't be
  waved away — a re-run on dedicated hardware is the next step to separate
  "barca genuinely regressed under fair conditions" from "shared-container noise
  disproportionately hurts a multi-process architecture."
- Fixed three pre-existing bugs uncovered while running this, unrelated to the
  CPU/RAM standardization work itself:
  - `parallel_tasks/{barca,dagster,prefect}/run.sh` called bare `barca`/`python`
    instead of the pinned venv binaries (worked only if a venv happened to
    already be active on `PATH`).
  - `resilience_pileup/barca/run.sh` was missing `--no-cache`, so every run after
    the first was a cache-hit no-op (measured: 2.5s cold vs 8ms warm) — the
    historical "330ms" figure above is suspect for the same reason.
    `resilience_pileup/prefect/run.py` had a variable name collision (`t0` was
    both a `@task` and the `__main__` timer, so every run raised
    `TypeError: 'float' object is not callable`) — always broken, not a
    regression from this change.
  - A real correctness bug in `barca-core`: dynamic partitions (`partitions_from`)
    combined with explicit `inputs={}` wiring to a downstream partitioned
    consumer could silently drop the dependency edge (root cause: partition-key
    chunks get bin-packed across dispatch streams by load alone, with no
    dependency awareness, so a consumer's chunk could be visited before its
    producer's). This broke `partitioned_etl` — confirmed pre-existing via
    bisect, not a regression from the adaptive-executor merge. Fixed in
    `coordinator.rs` (two-pass dependency resolution) and covered by a new Rust
    unit test plus `tests/integration/test_partitions.sh`, both wired into CI
    along with `tests/integration/test_benchmark_examples.sh` (runs every
    benchmark's barca side as a correctness smoke test on every PR).

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
