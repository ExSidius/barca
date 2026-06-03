# Benchmark Results

Last run: 2026-06-02 | Apple Silicon (M-series) | Rust release build

All benchmarks measure **cold-start, end-to-end wall time** — from process spawn to result persisted in local database. Each framework writes materialization results to its own local DB (Turso for barca, SQLite for dagster/prefect).

## Trivial: Single asset (zero work)

Measures pure framework overhead. The asset function returns `{"status": "ok"}`.

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| **barca** | 46.0 ms ± 1.8 ms | 44.0 ms | 49.1 ms | 1.00 |
| dagster | 552.6 ms ± 24.1 ms | 530.9 ms | 607.8 ms | 12.03 ± 0.70 |
| prefect | 3902 ms ± 38 ms | 3851 ms | 3965 ms | 84.91 ± 3.36 |

## Chain 100: Linear chain of 100 assets

Each asset consumes the previous one's output. Measures sequential DAG execution overhead. `asset_099` produces `{"step": 99, "value": 4950}`.

| Command | Mean | Min | Max | Relative |
|:---|---:|---:|---:|---:|
| **barca** | 52.8 ms ± 1.6 ms | 51.3 ms | 55.0 ms | 1.00 |
| dagster | 1130 ms ± 23 ms | 1100 ms | 1161 ms | 21.42 ± 0.79 |
| prefect | 3952 ms ± 87 ms | 3873 ms | 4086 ms | 74.87 ± 2.83 |

## Key observations

- **barca adds ~7ms per 99 extra assets** (46ms → 53ms). Per-asset overhead is ~70μs.
- **dagster adds ~580ms per 99 extra assets** (553ms → 1130ms). Per-asset overhead is ~5.9ms.
- **prefect adds ~50ms per 99 extra assets** (3902ms → 3952ms). Most time is import/startup, not per-asset.
- barca's total time is dominated by Python interpreter startup (~40ms), not orchestration.

## Reproducing

```bash
# Prerequisites: Rust toolchain, uv, hyperfine
# One-time setup:
cargo build --release && maturin develop --release
cd benchmarks/trivial/dagster && uv venv --python 3.12 && uv pip install dagster
cd benchmarks/trivial/prefect && uv venv --python 3.12 && uv pip install prefect
cd benchmarks/chain_100/dagster && uv venv --python 3.12 && uv pip install dagster
cd benchmarks/chain_100/prefect && uv venv --python 3.12 && uv pip install prefect

# Run:
benchmarks/trivial/bench.sh 10
# or manually:
hyperfine --warmup 3 --runs 10 \
  'benchmarks/trivial/barca/run.sh' \
  'benchmarks/trivial/dagster/run.sh' \
  'benchmarks/trivial/prefect/run.sh'
```
