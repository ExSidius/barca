# Server API

`barca serve` starts a long-running HTTP server (the `barca-server` crate, built on
[axum](https://github.com/tokio-rs/axum)) that exposes the orchestrator as a JSON API.
It is the foundation for programmatic triggering, scheduling, and a future web UI.

The server reuses `barca-core` directly — no subprocess, no separate daemon. Each request
that needs engine work runs the core command on a blocking task; runs execute in background
tasks and are tracked in memory.

## Starting the server

```bash
barca serve pipeline.py                   # serve a DAG, default port 8274
barca serve pipeline.py --port 8400       # custom port
barca serve pipeline.py --watch           # dev mode: re-parse DAG on file change
barca serve a.py b.py                      # multiple source files
```

The server binds to `127.0.0.1` (local only). There is no authentication in v1 — do not
expose it to untrusted networks.

`--watch` is a **local development convenience**: it re-parses the DAG when a source file
changes so `/assets` and `/plan` reflect edits without a restart. It is off by default and
has no effect on the production serving path.

## Endpoints (v1)

All responses are JSON.

| Method | Path | Description |
|--------|------|-------------|
| `GET`  | `/health` | Liveness + version. |
| `GET`  | `/assets` | List every node with kind, freshness, and upstream inputs. |
| `GET`  | `/assets/{name}` | One asset's summary joined with timing/cache stats. |
| `GET`  | `/plan` | Execution plan (phases and streams) as JSON. |
| `POST` | `/run` | Trigger a full run. Returns a `run_id` immediately. |
| `POST` | `/get/{target}` | Trigger a run scoped to one target asset. Returns a `run_id`. |
| `GET`  | `/status/{run_id}` | Poll the status and result of a run. |

### Async runs

Runs are asynchronous. `POST /run` and `POST /get/{target}` return immediately with a
server-side polling handle:

```json
{ "run_id": "1b942bb33182" }
```

Poll `GET /status/{run_id}` until `status` is `complete` or `failed`:

```json
{
  "handle": "1b942bb33182",
  "status": "complete",
  "result": {
    "run_id": "1b9422cf12f3",
    "elapsed_seconds": 0.115,
    "steps_executed": 2,
    "phases": 1,
    "final_output": { "path": ".barca/artifacts/…", "format": "json", "size_bytes": 8 }
  },
  "error": null,
  "started_at": 1780721263.05,
  "finished_at": 1780721263.17
}
```

`status` is one of `pending`, `running`, `complete`, `failed`. The `handle` is the server's
polling id; `result.run_id` is the persisted database run id (the run is also written to
`.barca/metadata.db`, same as a CLI run).

In-flight run state is held in memory and is not persisted across a server restart. The run
history in the database persists regardless.

### Health

```
GET /health
```

```json
{ "status": "ok", "version": "0.1.5" }
```

### Assets

```
GET /assets            → [AssetSummary, ...]
GET /assets/{name}     → { "asset": AssetSummary | null, "stats": AssetStats }
```

`AssetSummary` is `{ id, kind, freshness, inputs }`. `{name}` matches by asset name or full
node id; an unknown name returns `404`. `AssetStats` carries run counts, timing percentiles,
and cache hit rate.

### Plan

```
GET /plan              → { total_steps, phases: [{ reason, streams: [{ stream_id, steps }] }] }
```

## Errors

Errors return a JSON body `{ "error": "..." }` with an appropriate status code: `404` for an
unknown asset or run, `400` for parse/DAG errors, `500` for execution or database failures.

## Not in v1

No authentication, no WebSocket/SSE streaming (poll `/status`), no web UI, no distributed
execution, and no persistence of the in-memory run queue across restarts. A future UI is a
separate package that consumes this API; it could later be served from the same server via a
static-file route.
