# Changelog

## [Unreleased]

### Added

- Notebook helpers: `load_inputs()`, `materialize()`, `read_asset()`, `list_versions()` for interactive/notebook workflows (workflow 07)
- `_notebook.py` module with project bootstrapping and upstream value resolution
- 14 notebook workflow tests (shared upstream reuse, sensor upstream, effects, error cases, list_versions)
- Sensors as first-class nodes: dedicated `barca sensors list/show/trigger` CLI commands, `GET /sensors`, `GET /sensors/{id}/observations`, `POST /sensors/{id}/trigger` API endpoints
- `trigger_sensor()` engine function for manual sensor execution
- `list_sensor_observations()` for observation history retrieval
- `AssetDetail.latest_observation` populated for sensor nodes
- Sensor-aware CLI display (observation info instead of materialization info)
- 10 new tests (5 sensor unit + 5 server integration including e2e)

## [0.1.0] - 2025

### Added

- Pure Python uv workspace architecture (3 packages: barca-core, barca-cli, barca-server)
- `@asset()` decorator with dependency tracking, partitions, and content-addressed caching
- `@sensor()` decorator for external state observation with `(update_detected, output)` return contract
- `@effect()` decorator for side-effect leaf nodes
- Schedule-driven reconciliation: `"manual"`, `"always"`, `cron("...")` schedules
- Single-pass and continuous (`--watch`) reconcile modes
- AST-based dependency tracing with per-function dependency cone hashing
- Partitioned assets with `ThreadPoolExecutor` parallelism (free-threaded Python 3.13t)
- Asset continuity via `@asset(name="stable_name")`
- SQLite metadata store with optional Turso/libSQL remote support
- Typer CLI: `reindex`, `assets`, `sensors`, `jobs`, `reconcile`, `serve`, `reset`
- FastAPI HTTP server with background scheduler
- 61 pytest tests covering all features
- Benchmark suite comparing Barca, Prefect, and Dagster
- Spaceflights benchmark (10-asset diamond DAG adapted from Kedro)
