# Server API

The Barca server is an optional FastAPI application that provides HTTP endpoints and a background reconciliation scheduler.

## Starting the Server

```bash
uv run barca serve
uv run barca serve --port 8400 --interval 60 --log-level info
```

The server runs a background reconcile loop at the configured interval.

## Endpoints

### Health

```
GET /health
```

Returns server health and scheduler status.

```json
{"status": "ok", "scheduler_running": true}
```

### Assets

```
GET  /assets                → [AssetSummary, ...]
GET  /assets/{id}           → AssetDetail
POST /assets/{id}/refresh   → AssetDetail
```

- **List assets**: Returns all indexed assets with their latest materialization status.
- **Show asset**: Returns full detail for a single asset including definition, latest materialization, and latest observation (for sensors).
- **Refresh asset**: Reindexes, then materializes the asset and its upstream dependencies. Returns the updated detail.

### Sensors

```
GET  /sensors                          → [AssetSummary, ...]
GET  /sensors/{id}/observations        → [SensorObservation, ...]
POST /sensors/{id}/trigger             → SensorObservation
```

- **List sensors**: Returns all sensor nodes (filtered by `kind=sensor`).
- **Observations**: Returns observation history for a sensor.
- **Trigger**: Manually executes a sensor and records the observation.

### Reconcile

```
POST /reconcile → ReconcileResult
```

Runs a single reconciliation pass: reindex, topological sort, staleness check, and execution of eligible nodes. Returns counts of executed, stale, fresh, and failed nodes.

### Jobs

```
GET /jobs        → [JobDetail, ...]
GET /jobs/{id}   → JobDetail
```

- **List jobs**: Returns recent materializations with their associated asset summaries.
- **Show job**: Returns detail for a single materialization job.

## Response Models

See the [Models reference](api/models.md) for full schemas of `AssetSummary`, `AssetDetail`, `MaterializationRecord`, `SensorObservation`, `ReconcileResult`, and other response types.
