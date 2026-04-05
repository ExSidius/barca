# Testing

## Overview

All tests are Python pytest tests in the `tests/` directory. They run against temporary project fixtures created per test.

```bash
uv run pytest tests/ -v
```

## Test suites

| Suite | File | Tests | What it covers |
|-------|------|-------|---------------|
| Basic | `test_basic.py` | 5 | Reindex, refresh, caching, reset, idempotency |
| Dependencies | `test_deps.py` | 5 | Input linkage, upstream triggers, artifact passing, staleness |
| Partitions | `test_partitions.py` | 6 | Fan-out, parallel execution, distinct run hashes, caching |
| Reconciler | `test_reconciler.py` | 4 | Always/manual schedules, sensor→asset→effect pipeline |
| Sensors | `test_sensor.py` | 10 | Decorator, discovery, observations, trigger, history, guards |
| Effects | `test_effect.py` | 3 | Decorator, leaf-node validation, upstream execution |
| Schedules | `test_schedule.py` | 6 | Cron, manual, always; serialize/deserialize roundtrip |
| Tracing | `test_trace.py` | 5 | AST dependency tracing, cross-file deps, purity analysis |
| Codebase hash | `test_codebase_hash.py` | 3 | Hash stability, invalidation on change |
| Server | `test_server.py` | 14 | All HTTP endpoints, sensor trigger/observations, e2e reconcile |

**Total: 61 tests.**

## Test patterns

### Fixture projects

Each test creates a temporary project directory with:
- A Python module containing decorated functions
- A `barca.toml` with `[project] modules = [...]`
- Module cleanup in `sys.modules` to avoid cross-test pollution

### Server tests

Server tests (`test_server.py`) start a real uvicorn server in a background thread on a random port, exercise HTTP endpoints via `niquests`, and tear down after each test.

## State & output reference

### Materialization status values

Status transitions: `queued` → `running` → `success` | `failed`

| Status | `artifact_path` | `artifact_checksum` | `last_error` |
|--------|----------------|--------------------:|-------------|
| `queued` | null | null | null |
| `success` | `.barcafiles/{slug}/{def_hash}/value.json` | SHA-256 hex | null |
| `failed` | null | null | error message |

### Artifact directory structure

```
.barcafiles/{asset_slug}/{definition_hash}/
├── value.json     # serialized return value
└── code.txt       # source text at time of materialization
```

### Definition hash identity

The `definition_hash` is SHA-256 derived from: protocol version + function source + decorator metadata + dependency cone hash + serializer kind + Python version. Changing any input produces a new hash.

The `run_hash` is derived from the definition hash + upstream materialization IDs + partition key. A materialization is **fresh** when its `run_hash` matches; otherwise **stale**.

### API shapes

See the [server API endpoints](../README.md#server-api-endpoints) section in the README.

`AssetSummary` includes `kind` (`"asset"`, `"sensor"`, `"effect"`), `schedule`, and `materialization_status`.

`AssetDetail` includes `latest_observation` for sensors (with `update_detected`, `output_json`, `created_at`).
