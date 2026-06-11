# Parallel Tasks Benchmark

## What's being measured

Pure dispatch overhead for parallel fan-out of N tasks. Each task does zero
meaningful work — it just returns a dict — so the wall time reflects only the
framework's scheduling, serialization, and coordination costs.

## Metrics

- **Wall time**: end-to-end execution time for the full fan-out
- **Per-branch overhead**: wall time / N, showing amortized cost per dispatched unit

## Frameworks compared

| Framework | Mechanism |
|-----------|-----------|
| barca | `parallel()` with `partial(work, i)` |
| Dagster | `@op` invocations within a `@job` |
| Prefect | `@task.map()` with futures |

## Running

```bash
# Fan-out 10 tasks (default), 5 runs
./bench.sh

# Fan-out 100 tasks, 10 runs
./bench.sh 100 10
```

Requires `hyperfine` installed (`brew install hyperfine` / `cargo install hyperfine`).
