# Benchmarks

Barca ships with a comprehensive benchmark suite that compares execution overhead against Dagster and Prefect. All benchmarks use [hyperfine](https://github.com/sharkdp/hyperfine) for measurement.

## Running benchmarks

```bash
cd benchmarks/<name>
./bench.sh 10    # 10 measured runs (3 warmup runs)
```

Each benchmark directory contains:
- `barca/` -- barca implementation + run script
- `dagster/` -- equivalent Dagster implementation
- `prefect/` -- equivalent Prefect implementation
- `bench.sh` -- hyperfine wrapper that runs all three
- `results.md` -- markdown table with latest results

## Results

### Trivial (1 asset, zero work)

Measures pure framework overhead.

| Framework | Mean | Min | Max | Relative |
|-----------|------|-----|-----|----------|
| **barca (Rust+Python)** | **38.0 ms** | 37.4 ms | 38.9 ms | **1.00x** |
| dagster | 538.1 ms | 517.8 ms | 556.1 ms | 14.2x |
| prefect | 3977.7 ms | 3899.2 ms | 4244.4 ms | 104.7x |

## Benchmark suite

### Overhead & scaling

| Benchmark | Assets | Topology | What it tests |
|-----------|--------|----------|--------------|
| `trivial` | 1 | single node | Pure framework startup + teardown cost |
| `chain_100` | 100 | linear chain (A→B→C→...→Z) | Sequential phasing, 100 dependency hops |
| `fan_out_500` | 500 | 500 independent nodes | Wide parallelism, worker spawning overhead |
| `fan_out_500_50ms` | 500 | 500 independent + 50ms sleep each | Parallelism gains under I/O-bound workloads |

### DAG topologies

| Benchmark | Assets | Topology | What it tests |
|-----------|--------|----------|--------------|
| `deep_diamond` | 18 | 5 parallel 3-step pipelines → merge → 2-step tail | Fan-out/fan-in with real compute (filter, normalize, hash) |
| `wide_layers` | varies | parallel tiers | Tier-based parallel execution |
| `map_reduce` | varies | scatter → gather | Map-reduce pattern |

### Real workloads

| Benchmark | Assets | Topology | What it tests |
|-----------|--------|----------|--------------|
| `spaceflights` | 10 | 3-wide source → prep → merge → train → evaluate | Full ML pipeline (adapted from Kedro spaceflights) |
| `iris_pipeline` | varies | linear ML chain | Iris dataset classification pipeline |
| `mixed_io_cpu` | varies | mixed | Interleaved I/O-bound and CPU-bound assets |

### Edge cases

| Benchmark | Assets | Topology | What it tests |
|-----------|--------|----------|--------------|
| `large_payloads` | varies | varied | JSON serialization overhead with large return values |
| `multi_file_discovery` | varies | multi-file | Parsing and DAG construction across multiple Python files |

## Spaceflights topology

Adapted from [Kedro spaceflights](https://github.com/kedro-org/kedro-starters):

```
raw_shuttles ──→ prep_shuttles ──┐
raw_companies ─→ prep_companies ─├→ master_table → split → train → evaluate
raw_reviews ───→ prep_reviews ──┘
```

6 levels deep, 3 wide at the source layer. Uses `random` for data generation and `sklearn` for model training.

## Deep diamond topology

```
src_0 → prep_0 → feat_0 ──┐
src_1 → prep_1 → feat_1 ──┤
src_2 → prep_2 → feat_2 ──├→ merge → transform → output
src_3 → prep_3 → feat_3 ──┤
src_4 → prep_4 → feat_4 ──┘
```

18 assets, 5-wide parallelism, 6 levels deep. Each step does real compute (list filtering, normalization, hashing).
