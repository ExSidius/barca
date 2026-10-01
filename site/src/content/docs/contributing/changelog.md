---
title: Changelog
description: Notable changes to barca, release by release.
---

## [Unreleased]

### Added

- **Sub-minute scheduling** — `Schedule(...)` accepts a 6-field cron with a leading
  seconds field (e.g. `*/15 * * * * *`); the scheduler evaluates at 1-second
  resolution. 5-field crons are unchanged.
- **Built-in cron scheduler** in `barca serve` — enforces `freshness=Schedule(...)`,
  timezone-aware (`--timezone`), durable catch-up on restart, self-overlap skip,
  and live status via `GET /schedule`; disable with `--no-schedule`.
- **Shared remote state** — `barca.toml` config, `--env` separation, and remote
  artifact storage (Azure/S3/GCS/R2 via fsspec) with the metadata DB as a blob.
- Real `freshness=` keyword parameters on the `@asset`/`@sensor`/`@task` Python
  stubs (IDE autocomplete + type checking).
- `examples/scheduler` — a minimal standalone task-scheduler example.
- Sensors as first-class nodes with observation history
- Sensor-aware CLI display (observation info instead of materialization info)

### Changed

- Async-native core: the async runtime is owned by the caller, with cancellable runs.
- Cron parsing centralized behind one `CronExpr::parse` helper shared by validation,
  the scheduler, and the `/schedule` handler.

## [0.1.0] - 2025

### Added

- Rust + Python hybrid architecture: a Cargo workspace (`barca-core`, `barca-cli`,
  `barca-server`) for parsing, planning, and dispatch, plus a Python package
  (`python/barca/`) for decorator stubs and the worker
- `@asset()` decorator with dependency tracking, partitions, and content-addressed caching
- `@sensor()` decorator for external state observation with `(update_detected, output)` return contract
- `@task()` decorator for side-effect operations (replaces earlier `@effect` concept)
- Schedule-driven reconciliation: `"manual"`, `"always"`, `cron("...")` schedules
- Single-pass and continuous (`--watch`) reconcile modes
- AST-based dependency tracing with per-function dependency cone hashing
- Partitioned assets with worker-pool parallelism
- Asset continuity via `@asset(name="stable_name")`
- SQLite metadata store with optional Turso/libSQL remote support
- CLI: `get`, `run`, `plan`, `history`, `stats`, `serve`, `version`
- HTTP server (axum) with background scheduler
- Test suite covering all features (Rust unit/integration tests + Python worker tests)
- Benchmark suite comparing Barca, Prefect, and Dagster
- Spaceflights benchmark (10-asset diamond DAG adapted from Kedro)

For the full project changelog (including this site's own history), see [CHANGELOG.md](https://github.com/ExSidius/barca/blob/main/CHANGELOG.md) in the repository root.
