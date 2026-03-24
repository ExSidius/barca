# Benchmarks

Compares Barca, Prefect, and Dagster on orchestration overhead and parallel throughput.

## Quick start

```bash
# Prerequisites: cargo, uv, Python 3.11+
# One-time setup:
cd benchmarks/barca_bench && uv sync && uv pip install maturin && \
  uv run maturin develop --manifest-path ../../crates/barca-py/Cargo.toml --release && cd ..
cd prefect_bench && uv venv && uv pip install "prefect>=3.0" && cd ..
cd dagster_bench && uv venv && uv pip install "dagster>=1.9" && cd ..
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

## Execution models

| Framework | Model | Parallelism |
|-----------|-------|-------------|
| Barca (`-j N`) | N concurrent Python subprocesses | True process parallelism, configurable |
| Barca (`-j 1`) | Sequential subprocesses | One at a time |
| Prefect | ThreadPoolTaskRunner (64 threads) | Thread parallelism in 1 process |
| Dagster | In-process executor | Sequential, no parallelism |

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

# Prefect
cd prefect_bench
.venv/bin/python bench.py 3
.venv/bin/python bench_trivial.py 3
.venv/bin/python bench_cold_start.py 5

# Dagster
cd dagster_bench
.venv/bin/python bench.py 3
.venv/bin/python bench_trivial.py 3
.venv/bin/python bench_cold_start.py 5
```
