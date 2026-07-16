# Benchmarks

Compares Barca, Dagster, and Prefect on orchestration overhead and parallel throughput.

All measurements use [hyperfine](https://github.com/sharkdp/hyperfine). See [RESULTS.md](RESULTS.md) for the latest numbers.

## Methodology

### What's controlled
- **Same machine**: all benchmarks run on the same hardware
- **Same workload**: each benchmark implements identical logic across all three frameworks
- **Cold start**: every run starts a fresh process (no warm caches)
- **Parallel where applicable**: benchmarks with independent nodes enable parallelism in all frameworks
- **CPU affinity**: every measured process is pinned to the same fixed set of cores via
  `taskset` (see `benchmarks/lib/env.sh`), so the OS scheduler can't migrate work mid-run
  and load on unrelated cores doesn't leak into the measurement
- **Worker count**: Barca's `pool_size`, Dagster's `max_concurrent`, and Prefect's
  `max_workers` are all set to the *same* value for a given run — none of them is
  over- or under-subscribed relative to the others (previously Dagster/Prefect
  benchmarks hardcoded 16 workers while Barca auto-detected `cpu_count`, which made
  parallel-throughput comparisons depend on how many cores the benchmark machine had)
- **RAM**: total system RAM and CPU model are captured and printed in every `bench.sh`
  run's banner for transparency; no benchmark here is memory-bound enough to warrant
  an artificial memory ceiling
- **Peak memory (opt-in)**: `BARCA_BENCH_MEMORY=1 benchmarks/<name>/bench.sh` adds an
  extra untimed pass per framework that reports peak memory for the *whole process
  tree* (parent + every child it spawns), via a fresh cgroup per run — falls back to
  `/usr/bin/time -v` (single-process only, clearly labeled) if no cgroup memory
  controller is writable. This matters because barca is multi-process (Rust
  coordinator + N Python workers): a naive single-process wrapper would only see the
  coordinator and make barca look artificially light next to Dagster/Prefect's single
  process. Off by default since it re-runs each framework once more and adds
  wall-clock, especially for slow-starting frameworks.

### What differs
- **Python version**: Barca uses Python 3.14 (from workspace .venv). Dagster and Prefect use Python 3.12 (latest compatible with both)
- **Parallelism model**: Barca uses multi-process (Rust spawns N Python workers). Dagster uses multiprocess executor. Prefect uses ConcurrentTaskRunner
- **DB persistence**: Barca writes to Turso/libSQL. Dagster and Prefect use their own internal SQLite stores

### Parallelism configuration

`benchmarks/lib/env.sh` exports `BARCA_POOL_SIZE` and `BARCA_BENCH_WORKERS` (both
default to `BARCA_BENCH_CORES`, default `4`) for every framework to read:

- **Barca**: `pool_size` — set via the `BARCA_POOL_SIZE` env var (falls back to `cpu_count` if unset)
- **Dagster**: `multiprocess_executor` — `max_concurrent` would read `BARCA_BENCH_WORKERS` if wired up (falls back to 16 if unset)
- **Prefect**: `ConcurrentTaskRunner` — `max_workers` would read `BARCA_BENCH_WORKERS` if wired up (falls back to 16 if unset)

In practice, only `deep_diamond/prefect/run.py` and `mixed_io_cpu/prefect/run.py`
(plus the `*_server` variants, which aren't invoked by `bench.sh`) actually read
`BARCA_BENCH_WORKERS` today — the rest of the script-mode Dagster/Prefect files
call `materialize()` / `execute_in_process()` / `@flow` with framework defaults.
For those, cross-framework worker parity comes from the shared `taskset` CPU pin
alone (same core budget), not a matched worker count. Override the pin with
`BARCA_BENCH_CORES=8 benchmarks/trivial/bench.sh`.

For the `*_server` benchmarks (persistent `dagster dev` / long-running Prefect
processes), source `benchmarks/lib/env.sh` in the shell that runs `start.sh` — the
worker count is read once, at server startup, from the environment.

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
| `etl_duckdb_dataframes` | 12 | raw → staging → marts (DataFrame/parquet payloads) | Parallel sources |
| `large_payloads` | 5 | linear chain, 10k rows/step | None (sequential) |

### Partitioned workloads

| Benchmark | Steps | Topology | Parallelism |
|---|---|---|---|
| `partitioned_chain` | 150 | 3 assets × 50 partitions | Parallel partitions |
| `partitioned_etl` | — | ETL with partitioning | Parallel partitions |
| `partitioned_fan_in` | — | Fan-in with partitions | Partition-aligned |
| `partitioned_10k` | ~10k | Docker-based cross-framework | Parallel partitions |

### Dynamic dispatch & resilience (v0.2.0)

| Benchmark | Steps | What it tests |
|---|---|---|
| `parallel_tasks` | N (param) | `parallel()` runtime dispatch — SIGSTOP/SIGCONT, temp workers |
| `resilience_pileup` | 18 | Failure/retry behavior — one flaky chain shouldn't stall healthy work |

## Running benchmarks

```bash
# Prerequisites: Rust toolchain, uv, hyperfine, taskset (util-linux)
cargo build --release && maturin develop --release

# Run individual benchmark (sets up venvs on first run):
benchmarks/trivial/bench.sh 10

# Override the pinned core count / worker count (default: 4 cores):
BARCA_BENCH_CORES=8 benchmarks/trivial/bench.sh 10

# Or run manually with hyperfine, pinned and worker-matched the same way bench.sh does:
source benchmarks/lib/env.sh
hyperfine --warmup 3 --runs 10 \
  "$(bench_pin benchmarks/trivial/barca/run.sh)" \
  "$(bench_pin benchmarks/trivial/dagster/run.sh)" \
  "$(bench_pin benchmarks/trivial/prefect/run.sh)"
```

Each benchmark directory contains:
- `barca/` — barca implementation + `run.sh`
- `dagster/` — equivalent Dagster implementation + `run.sh`
- `prefect/` — equivalent Prefect implementation + `run.sh`
