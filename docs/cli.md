# CLI Reference

All commands use the `uv run` prefix. If you've activated your virtualenv, you can omit it.

## Commands

```
uv run barca reindex                          Re-inspect Python modules, update DB
uv run barca assets list                      List all indexed assets
uv run barca assets show <id>                 Show asset detail
uv run barca assets refresh <id>              Trigger materialization
uv run barca assets refresh <id> -j <N>       Parallel partition workers
uv run barca assets refresh <id> --stale-policy error|warn|pass
uv run barca sensors list                     List all sensors
uv run barca sensors show <id>                Show sensor detail + observation history
uv run barca sensors trigger <id>             Manually trigger a sensor
uv run barca jobs list                        List recent materializations
uv run barca jobs show <id>                   Show job detail
uv run barca run                              Run production mode (long-running)
uv run barca dev                              Development mode — file watcher, live staleness
uv run barca serve                            HTTP API + background scheduler
uv run barca serve --port 8400                Custom port
uv run barca serve --log-level info           Log level (debug, info, warning, error)
uv run barca prune                            Remove history/artifacts unreachable from current DAG
uv run barca reset [--db] [--artifacts] [--tmp]  Clean generated files
```

## reindex

Discover all `@asset()`, `@sensor()`, `@effect()`, and `@sink()` decorated functions in your project. Computes definition hashes and upserts into the metadata database. Shows a three-way diff of what changed: added assets, removed assets, and renamed/moved assets.

```bash
uv run barca reindex
```

Rename detection uses two signals in priority order:
1. AST match — a removed and an added asset have identical function bodies (covers file reorganisation).
2. `name=` match — same explicit `name=` at a different location.

Removed assets are pruned from the active DAG but their history is preserved.

## assets

### list

Show all indexed assets with their current status. Unsafe assets are visually distinguished. For dynamically partitioned assets whose partition-defining upstream has not yet been materialised, the partition set is shown as "pending".

```bash
uv run barca assets list
```

### show

Show detailed information for a single asset, including its latest materialization.

```bash
uv run barca assets show 1
```

### refresh

Materialize an asset. For partitioned assets, use `-j` to control parallelism.

By default, all upstream inputs must be fresh (`--stale-policy error`). Use `warn` to proceed with stale inputs (records `stale_inputs_used=true` and emits a warning), or `pass` to proceed silently.

```bash
uv run barca assets refresh 1                             # default: error on stale inputs
uv run barca assets refresh 1 --stale-policy warn         # proceed with warning
uv run barca assets refresh 1 --stale-policy pass         # proceed silently
uv run barca assets refresh 1 -j 1                        # sequential
uv run barca assets refresh 1 -j 8                        # 8 parallel workers
```

`refresh` does not cascade to downstream assets. Downstream becomes implicitly stale and is picked up on the next `barca run` pass or explicit refresh.

## sensors

### list

Show all indexed sensors.

```bash
uv run barca sensors list
```

### show

Show sensor detail and observation history.

```bash
uv run barca sensors show 1
```

### trigger

Manually trigger a sensor and record an observation.

```bash
uv run barca sensors trigger 1
```

## jobs

### list

Show recent materialization jobs.

```bash
uv run barca jobs list
```

### show

Show detail for a specific job.

```bash
uv run barca jobs show 1
```

## run

Long-running production mode. Continuously maintains the DAG at each asset's declared freshness level. On each pass (topo order):

- `Always` assets: materialise if stale and all upstreams fresh
- `Schedule` assets: materialise if a cron tick has elapsed since last run
- `Manual` assets: never auto-materialise
- Sensors: observe on each `Schedule` tick (or `Manual` trigger)
- Effects/Sinks: run when upstream freshens

`Manual` freshness blocks downstream `Always` assets from auto-updating.

If a pass is already running when the next tick arrives, the tick is skipped — passes do not overlap.

```bash
uv run barca run
```

## dev

Development mode. Watches for file changes and updates asset staleness state in the UI in real time. Does not materialise anything — use `barca assets refresh` to materialise individual assets during development.

```bash
uv run barca dev
```

## serve

Start the FastAPI HTTP server with a background scheduler. The scheduler uses the same semantics as `barca run`.

```bash
uv run barca serve
uv run barca serve --port 8400 --log-level info
```

## prune

Remove all artifacts, materialisations, and DB records that are not reachable from the current active DAG. This includes removed assets, removed partition values, and old definition hash versions no longer referenced by any current asset. This is the only operation that permanently deletes history.

```bash
uv run barca prune
```

## reset

Remove generated files and caches.

```bash
uv run barca reset --db          # remove .barca/ (metadata database)
uv run barca reset --artifacts   # remove .barcafiles/ (artifacts)
uv run barca reset --tmp         # remove tmp/
uv run barca reset --db --artifacts --tmp  # everything
```
