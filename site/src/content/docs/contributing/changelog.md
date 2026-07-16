---
title: Changelog
description: Notable changes to barca, release by release.
---

## [Unreleased]

### Added

- Sensors as first-class nodes with observation history
- Sensor-aware CLI display (observation info instead of materialization info)

## [0.1.0] - 2025

### Added

- Pure Python uv workspace architecture (3 packages: barca-core, barca-cli, barca-server)
- `@asset()` decorator with dependency tracking, partitions, and content-addressed caching
- `@sensor()` decorator for external state observation with `(update_detected, output)` return contract
- `@task()` decorator for side-effect operations (replaces earlier `@effect` concept)
- Schedule-driven reconciliation: `"manual"`, `"always"`, `cron("...")` schedules
- Single-pass and continuous (`--watch`) reconcile modes
- AST-based dependency tracing with per-function dependency cone hashing
- Partitioned assets with `ThreadPoolExecutor` parallelism (free-threaded Python 3.14t)
- Asset continuity via `@asset(name="stable_name")`
- SQLite metadata store with optional Turso/libSQL remote support
- CLI: `get`, `run`, `plan`, `history`, `stats`, `serve`, `version`
- FastAPI HTTP server with background scheduler
- 61 pytest tests covering all features
- Benchmark suite comparing Barca, Prefect, and Dagster
- Spaceflights benchmark (10-asset diamond DAG adapted from Kedro)

For the full project changelog (including this site's own history), see [CHANGELOG.md](https://github.com/ExSidius/barca/blob/main/CHANGELOG.md) in the repository root.
