# Benchmarks

Compares Barca, Prefect, and Dagster on orchestration overhead and parallel throughput.

## Quick start

```bash
# Prerequisites: cargo, uv, Python 3.11+
# One-time setup:
cd benchmarks/barca_bench && uv sync && uv pip install maturin && \
  uv run maturin develop --manifest-path ../../crates/barca-py/Cargo.toml --release && cd ..
cd prefect_bench && uv venv && uv pip install "prefect>=3.0" "scikit-learn" && cd ..
cd dagster_bench && uv venv && uv pip install "dagster>=1.9" "scikit-learn" && cd ..
cargo build -p barca-cli --release

# Run all benchmarks (3 iterations each):
bash run_all.sh 3
```

## Benchmarks

| # | Name | What it measures |
|---|------|-----------------|
| 1a | 500 trivial jobs | Pure framework overhead (no-op tasks) |
| 1b | 500 jobs x 50ms | Parallel throughput with simulated I/O |
| 2 | Cold start | Time to materialize 1 asset from scratch |
| 3 | Server pickup | HTTP POST → job complete latency (Barca only) |
| 4 | Spaceflights | 10-asset diamond DAG with sklearn (adapted from Kedro) |

## Execution models

| Framework | Model | Parallelism |
|-----------|-------|-------------|
| Barca (`-j N`) | N concurrent Python subprocesses | True process parallelism, configurable |
| Barca (`-j 1`) | Sequential subprocesses | One at a time |
| Prefect | ThreadPoolTaskRunner (64 threads) | Thread parallelism in 1 process |
| Dagster | In-process executor | Sequential, no parallelism |

## Known issues

**Barca per-job subprocess overhead (~60ms/job).** Each Barca partition spawns a
fresh `uv run python -m barca.worker` process. That ~60ms startup cost is
amortized well at high concurrency (`-j 4` runs 4 subprocesses in parallel, so
500 jobs only pay ~125 sequential rounds of overhead). But at `-j 1` it's
catastrophic: 500 × 60ms = 30s of pure process-spawn overhead on top of the
actual work, making sequential Barca slower than Dagster's in-process executor
which simply loops within a single Python process.

A future persistent-worker mode (long-lived Python process that receives work
over a pipe/socket instead of restarting each time) would eliminate this cost
and make Barca competitive even at `-j 1`.

## Individual scripts

Each benchmark can be run standalone:

```bash
# Barca (args: runs [concurrency])
cd barca_bench
python bench.py 3        # default concurrency (= nproc)
python bench.py 3 1      # sequential (-j 1)
python bench.py 3 64     # 64 concurrent
python bench_trivial.py 3
python bench_cold_start.py 5
python bench_pickup.py 5  # starts and stops the server
python bench_spaceflights.py 3  # 10-asset diamond DAG

# Prefect
cd prefect_bench
.venv/bin/python bench.py 3
.venv/bin/python bench_trivial.py 3
.venv/bin/python bench_cold_start.py 5
.venv/bin/python bench_spaceflights.py 3

# Dagster
cd dagster_bench
.venv/bin/python bench.py 3
.venv/bin/python bench_trivial.py 3
.venv/bin/python bench_cold_start.py 5
.venv/bin/python bench_spaceflights.py 3
```
