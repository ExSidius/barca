# Changelog

## [0.1.0] - 2025

Complete rewrite from Python to Rust. The orchestrator is now a compiled binary
shipped as a maturin-built wheel with thin Python decorator stubs.

### Added

- Rust workspace: `barca-core` (parse, DAG, planner, cone analysis, hashing) and
  `barca-cli` (commands, dispatch, cache, DB)
- `barca run <file.py>` — parse, plan, and execute an asset graph
- `barca get <target> <file.py>` — execute a single asset's subgraph with caching
- `barca plan <file.py>` — emit the execution plan as JSON
- `@asset()` decorator with dependency tracking via `inputs={}` parameter
- `@sensor()` for external state observation with `(updated, data)` return contract
- `@effect()` for side-effect leaf nodes
- `@sink(path, serializer=)` for output file declaration
- `@asset(serializer="json"|"pickle"|"parquet")` for explicit artifact format control
- Static partitions via `partitions()`, dynamic via expressions, derived via `partitions_from()`
- `collect()` fan-in: aggregate all partitions of an upstream asset
- Freshness markers: `Always`, `Manual`, `Schedule(cron="...")`
- Content-addressed caching: definition hash (function source + dependency cone) and
  run hash (definition + partition + upstream hashes) persisted to `.barca/metadata.db`
- Dependency cone analysis with same-file and cross-file helper/constant tracking
- File-based artifact persistence: outputs serialize to `.barca/artifacts/` as JSON,
  pickle, or parquet (auto-detected from value type)
- Protocol v2: workers emit lightweight artifact receipts on stderr, Rust records metadata
- Parallel worker dispatch with per-phase streaming and partition-aligned execution
- Python stubs: no-op decorators for IDE autocomplete and standalone execution
- Integration test suite (CLI, cache, staleness, cross-file, sensor, partitions)
- Benchmark suite comparing against Dagster, Prefect, and Airflow
