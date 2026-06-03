# Benchmark Results

Last run: 2026-06-03 | Apple Silicon (M-series) | Rust release build | Multi-process dispatch

Architecture: Rust plans and spawns N Python worker processes per phase. Workers communicate via stdout (JSON lines). Rust owns all DB persistence. Consecutive single-stream phases merge into one worker.

## Summary

| Benchmark | barca | dagster | prefect | vs dagster |
|:---|---:|---:|---:|---:|
| Trivial (1 asset) | **29ms** | 535ms | 3.9s | 18.7x |
| Chain 100 (linear) | **37ms** | 1.1s | 4.3s | 29.8x |
| Fan-out 500 (independent) | **87ms** | 2.3s | 4.0s | 26.8x |
| Fan-out 500×50ms (parallel I/O) | **1.9s** | 33.6s | 33.5s | 17.7x |
| Spaceflights (10-asset diamond, sklearn) | **574ms** | 1.2s | 3.9s | 2.1x |
| Deep Diamond (18 assets, 5-wide) | **77ms** | 638ms | 3.9s | 8.3x |
| Wide Layers (63 assets, 3×20 layers) | **167ms** | 919ms | 3.9s | 5.5x |
| Mixed I/O+CPU (API calls + SHA compute) | **220ms** | 971ms | 3.9s | 4.4x |

## Detailed Results

### 1. Trivial: Single asset (zero work)

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 28.6 ms ± 0.5 ms | 1.00 |
| dagster | 534.7 ms ± 14.2 ms | 18.7x |
| prefect | 3912 ms ± 171 ms | 136.8x |

### 2. Chain 100: Linear chain of 100 assets

1 phase, 1 stream (entire chain bundled vertically in one process).

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 37.4 ms ± 1.2 ms | 1.00 |
| dagster | 1113 ms ± 20 ms | 29.8x |
| prefect | 4310 ms ± 840 ms | 115.2x |

### 3. Fan-out 500: 500 independent assets (zero work)

1 phase, 16 streams (500 assets distributed across 16 worker processes).

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 86.9 ms ± 3.4 ms | 1.00 |
| dagster | 2325 ms ± 19 ms | 26.8x |
| prefect | 3975 ms ± 75 ms | 45.7x |

### 4. Fan-out 500 × 50ms: Parallel throughput (simulated I/O)

500 independent assets each sleeping 50ms. Sequential minimum: 25.0s.

| Command | Wall time | Notes |
|:---|---:|:---|
| **barca** | 1.9s | 16 worker processes, ~31 steps each |
| dagster | 33.6s | Sequential in-process |
| prefect | 33.5s | Sequential task execution |

### 5. Spaceflights: 10-asset diamond DAG (sklearn)

2 phases (after merge optimization). Phase 1: 3 parallel [raw→prep] streams. Phase 2: 1 stream [master→split→train→eval].

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 573.5 ms ± 24.9 ms | 1.00 |
| dagster | 1194 ms ± 36 ms | 2.08x |
| prefect | 3872 ms ± 61 ms | 6.75x |

### 6. Deep Diamond: 18-asset diamond DAG

5 parallel 3-step pipelines → merge → 2-step post-processing. 2 phases, 6 streams.

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 77.2 ms ± 1.1 ms | 1.00 |
| dagster | 638.2 ms ± 9.5 ms | 8.27x |
| prefect | 3850 ms ± 44 ms | 49.9x |

### 7. Wide Layers: 63-asset layered DAG

3 layers of 20 independent assets + aggregation barriers. 6 phases, 20-wide parallelism.

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 167.3 ms ± 14.0 ms | 1.00 |
| dagster | 918.5 ms ± 24.2 ms | 5.49x |
| prefect | 3901 ms ± 6 ms | 23.3x |

### 8. Mixed I/O + CPU: Realistic pipeline

5 parallel API calls (50ms each) → merge → CPU-heavy SHA compute → summarize.

| Command | Mean | Relative |
|:---|---:|---:|
| **barca** | 220.1 ms ± 3.8 ms | 1.00 |
| dagster | 970.5 ms ± 7.6 ms | 4.41x |
| prefect | 3885 ms ± 48 ms | 17.7x |

## Reproducing

```bash
cargo build --release && maturin develop --release

# Individual benchmarks:
hyperfine --warmup 3 --runs 10 benchmarks/trivial/*/run.sh
hyperfine --warmup 2 --runs 5 benchmarks/chain_100/*/run.sh
hyperfine --warmup 1 --runs 3 benchmarks/fan_out_500/*/run.sh
hyperfine --warmup 1 --runs 3 benchmarks/spaceflights/*/run.sh
hyperfine --warmup 1 --runs 3 benchmarks/deep_diamond/*/run.sh
hyperfine --warmup 1 --runs 3 benchmarks/wide_layers/*/run.sh
hyperfine --warmup 1 --runs 3 benchmarks/mixed_io_cpu/*/run.sh
```
