# Benchmark Results

Last run: 2026-06-03 | Apple Silicon (M-series) | Rust release build | Multi-process dispatch

Architecture: Rust plans and spawns N Python worker processes per phase. Workers communicate via stdout (JSON lines). Rust owns all DB persistence.

## 1. Trivial: Single asset (zero work)

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 28.6 ms ± 0.5 ms | 1.00 |
| dagster | 534.7 ms ± 14.2 ms | 18.7x |
| prefect | 3912 ms ± 171 ms | 136.8x |

## 2. Chain 100: Linear chain of 100 assets

1 phase, 1 stream (entire chain bundled vertically in one process).

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 37.4 ms ± 1.2 ms | 1.00 |
| dagster | 1113 ms ± 20 ms | 29.8x |
| prefect | 4310 ms ± 840 ms | 115.2x |

## 3. Fan-out 500: 500 independent assets (zero work)

1 phase, 16 streams (500 assets distributed across 16 worker processes).

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 86.9 ms ± 3.4 ms | 1.00 |
| dagster | 2325 ms ± 19 ms | 26.8x |
| prefect | 3975 ms ± 75 ms | 45.7x |

## 4. Fan-out 500 × 50ms: Parallel throughput (simulated I/O)

500 independent assets each sleeping 50ms. Sequential minimum: 25.0s.

| Command | Wall time | Notes |
|:---|---:|:---|
| **barca** | 1.9s | 16 worker processes, ~31 steps each |
| dagster | 33.6s | Sequential in-process |
| prefect | 33.5s | Sequential task execution |

Barca achieves **17.7x speedup** over sequential via multi-process parallelism.

## 5. Spaceflights: 10-asset diamond DAG (sklearn)

4 phases, 6 streams. Phase 1: 3 parallel [raw→prep] chains. Phase 2+: [master→split→train→eval].

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 1087 ms ± 23 ms | 1.00 |
| dagster | 1241 ms ± 27 ms | 1.14x |
| prefect | 5294 ms ± 2201 ms | 4.87x |

## Key improvements (multi-process dispatch)

| Benchmark | Previous (ThreadPoolExecutor) | Current (multi-process) | Change |
|:---|---:|---:|---:|
| Trivial | 46ms | 29ms | **37% faster** |
| Chain 100 | 53ms | 37ms | **30% faster** |
| Fan-out 500 | 104ms | 87ms | **16% faster** |
| Fan-out 500×50ms | 1.5s | 1.9s | 27% slower (process spawn overhead vs threads) |
| Spaceflights | 551ms | 1087ms | Slower (4 phases × process spawn overhead) |

Multi-process is faster for trivial/chain cases (less Python import overhead with direct stdout), but adds latency for compute-heavy multi-phase DAGs where process spawn cost exceeds the parallelism benefit. The spaceflights regression is because sklearn compute dominates and we pay 4× process spawn cost.

## Reproducing

```bash
cargo build --release && maturin develop --release
hyperfine --warmup 3 --runs 10 benchmarks/trivial/*/run.sh
hyperfine --warmup 2 --runs 5 benchmarks/chain_100/*/run.sh
hyperfine --warmup 1 --runs 3 benchmarks/fan_out_500/*/run.sh
hyperfine --warmup 1 --runs 3 benchmarks/spaceflights/*/run.sh
```
