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
barca serve pipeline.py --no-schedule     # disable the cron scheduler
barca serve pipeline.py --timezone utc    # evaluate cron in UTC (default: local)
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
| `POST` | `/run/{target}` | Trigger a task run. Returns a `run_id`. |
| `POST` | `/get/{target}` | Trigger a run scoped to one target asset. Returns a `run_id`. |
| `GET`  | `/status/{run_id}` | Poll the status and result of a run. |
| `GET`  | `/schedule` | List scheduled jobs with next fire time and last run status. |

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
{ "status": "ok", "version": "0.5.0" }
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

## Scheduling

`barca serve` runs a **cron scheduler** — the piece that gives
`@asset(freshness=Schedule("..."))` teeth. It is **on by default**; pass `--no-schedule`
to turn it off.

At startup the server enumerates every node whose freshness is `Schedule(cron)`, parses
each 5-field cron expression, and logs the schedule (invalid or empty cron strings are
logged and skipped, not fatal). A background task then wakes at each minute boundary and,
for every job whose cron matches the current minute, triggers a run through the same path
as `POST /run` / `POST /run/{target}`:

- **Assets and sensors** are materialized via the `get` path.
- **Tasks** are executed via the `run` path.

Each scheduled run gets a normal `run_id`, is visible via `GET /status/{run_id}`, and is
persisted to `.barca/metadata.db` (`barca history`) — identical to a manually triggered run.
Inspect the live schedule with `GET /schedule` or, statically, with `barca list <files>`
(scheduled definitions show their next fire time).

Behavior:

- **Timezone.** Cron is evaluated in the machine's local time by default. Pass
  `--timezone utc` or `--timezone America/New_York` (any IANA name) to change it.
- **Catch-up.** The scheduler persists the last fire time of each job. On startup, if a
  scheduled tick elapsed while the daemon was down, the job fires **once** to catch up
  (jobs never seen before are anchored to "now" — no first-launch stampede). Individual
  ticks missed during a long outage are *not* replayed one-for-one.
- **Concurrency.** Independent runs execute in parallel (bounded by a run pool sized to the
  machine's CPUs); their writes to the shared `metadata.db` are serialized by a process-wide
  DB lock. A scheduled job never overlaps *itself*: if its previous run is still
  pending/running when the next tick arrives, that tick is skipped.
- **Reload.** Under `--watch`, editing a source file re-reads the schedule live (within a
  minute). Without `--watch` the job set is fixed for the process lifetime.

### `GET /schedule`

```
GET /schedule → [ScheduleEntry, ...]
```

Each `ScheduleEntry` is:

```json
{
  "id": "pipeline.py:daily_report",
  "cron": "0 5 * * *",
  "kind": "asset",
  "next_fire": 1780740000,
  "last_fired": 1780653600,
  "last_run": "1b942bb33182",
  "last_status": "complete"
}
```

`next_fire`/`last_fired` are unix epoch seconds (`last_fired` is `null` until the first
fire); `last_run` is the most recent scheduled `run_id` and `last_status` its state
(`pending`/`running`/`complete`/`failed`, or `null` if none yet).

## Python client

The `barca.Client` SDK (standard-library only) wraps this API:

```python
from barca import Client

c = Client("http://127.0.0.1:8274")
run = c.get("daily_report")       # POST /get/{target}, returns immediately
result = run.wait(timeout=30)     # poll /status until complete/failed
print(result["status"])

for job in c.schedules():         # GET /schedule
    print(job["id"], job["cron"], job["next_fire"])
```

`Client` methods map to the endpoints above: `health()`, `assets()`, `asset(name)`,
`plan()`, `schedules()`, `status(run_id)`, plus the two trigger verbs that mirror the CLI —
`get(target=None)` (`barca get [TARGET]`; omit the target for a full-DAG run) and
`run(target)` (`barca run TARGET`). The trigger methods return a `Run` whose `.wait()` blocks
until the run reaches a terminal state. This complements `barca.api` (`barca.get`/`run`/…),
which shells out to the binary for one-shot commands rather than talking to a server.

## Errors

Errors return a JSON body `{ "error": "..." }` with an appropriate status code: `404` for an
unknown asset or run, `400` for parse/DAG errors, `500` for execution or database failures.

## Not in v1

No authentication, no WebSocket/SSE streaming (poll `/status`), no web UI, no distributed
execution, and no persistence of the in-memory run queue across restarts. A future UI is a
separate package that consumes this API; it could later be served from the same server via a
static-file route.
