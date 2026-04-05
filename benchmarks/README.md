# Benchmarks

Compares Barca, Prefect, and Dagster on orchestration overhead and parallel throughput.

## Quick start

```bash
# Prerequisites: uv, Python 3.13+ (free-threaded recommended)
# One-time setup:
cd benchmarks/barca_bench && uv sync && cd ..
cd prefect_bench && uv venv && uv pip install "prefect>=3.0" "scikit-learn" && cd ..
cd dagster_bench && uv venv && uv pip install "dagster>=1.9" "scikit-learn" && cd ..

# Run all benchmarks (3 iterations each):
bash run_all.sh 3
```

## Benchmarks

| # | Name | What it measures |
|---|------|-----------------|
| 1a | 500 trivial jobs | Pure framework overhead (no-op tasks) |
| 1b | 500 jobs x 50ms | Parallel throughput with simulated I/O |
| 2 | Cold start | Time to materialize 1 asset from scratch |
| 3 | Spaceflights | 10-asset diamond DAG with sklearn (adapted from Kedro) |
| 4a | Server startup | Time from process start to first successful API response |
| 4b | Server API latency | Time from HTTP request to materialization complete |

## Execution models

| Framework | Model | Parallelism |
|-----------|-------|-------------|
| Barca (`-j N`) | N concurrent threads (free-threaded Python) | True thread parallelism, no GIL |
| Barca (`-j 1`) | Sequential in-process | One at a time |
| Prefect | ThreadPoolTaskRunner (64 threads) | Thread parallelism in 1 process |
| Dagster | In-process executor | Sequential, no parallelism |

## Server comparison

| Framework | Server command | API style | Asset trigger |
|-----------|---------------|-----------|---------------|
| Barca | `barca serve` | REST (FastAPI) | `POST /assets/{id}/refresh` |
| Prefect | `prefect server start` | REST | Flow run via client SDK |
| Dagster | `dagster dev` | GraphQL | `launchRun` mutation |

### What the server benchmarks measure

**Benchmark 4a — Startup time**: Measures the wall-clock time from spawning the
server process to the first successful health/API response. This captures
framework initialization, database setup, and HTTP server startup.

**Benchmark 4b — API latency**: Measures the round-trip time from sending an
HTTP request to trigger a materialization until the materialization completes.
For Barca this is synchronous (the POST blocks until done). For Dagster and
Prefect this involves launching a run and polling until it reaches a terminal
state.

### Execution details

- **Barca**: FastAPI + uvicorn. `POST /assets/{id}/refresh` runs the
  materialization synchronously via `asyncio.to_thread()` and returns the
  result. `POST /reconcile` runs a full DAG walk. Background scheduler disabled
  during benchmarks (interval=3600s).

- **Dagster**: `dagster dev` starts a webserver with GraphQL API. Materialization
  triggered via `launchRun` mutation, polled via `runOrError` query.

- **Prefect**: `prefect server start` starts the API server. Flow runs triggered
  via the Prefect client SDK (which talks to the server). The server tracks run
  state while execution happens in the client process.

## Free-threaded Python

Barca defaults to Python 3.13t (free-threaded build, GIL disabled). This gives
`ThreadPoolExecutor` true parallelism without subprocess overhead. The `-j N`
flag controls concurrency (default: cpu_count).

To opt out and use regular Python 3.13+, change `.python-version` from `3.13t`
to `3.13`. Threads will still work for I/O-bound tasks (GIL is released during
I/O), but CPU-bound partitions won't get true parallelism.

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
python bench_spaceflights.py 3  # 10-asset diamond DAG
python bench_server.py 5        # all server benchmarks
python bench_server.py 5 startup    # startup only
python bench_server.py 5 refresh    # refresh latency only
python bench_server.py 5 reconcile  # reconcile latency only

# Prefect
cd prefect_bench
.venv/bin/python bench.py 3
.venv/bin/python bench_trivial.py 3
.venv/bin/python bench_cold_start.py 5
.venv/bin/python bench_spaceflights.py 3
.venv/bin/python bench_server.py 5          # all server benchmarks
.venv/bin/python bench_server.py 5 startup  # startup only
.venv/bin/python bench_server.py 5 flow     # flow run latency only

# Dagster
cd dagster_bench
.venv/bin/python bench.py 3
.venv/bin/python bench_trivial.py 3
.venv/bin/python bench_cold_start.py 5
.venv/bin/python bench_spaceflights.py 3
.venv/bin/python bench_server.py 5           # all server benchmarks
.venv/bin/python bench_server.py 5 startup   # startup only
.venv/bin/python bench_server.py 5 refresh   # materialization latency only
```
