# CLI Reference

All commands use the `uv run` prefix. If you've activated your virtualenv, you can omit it.

## Commands

```
uv run barca reindex                          Re-inspect Python modules, update DB
uv run barca assets list                      List all indexed assets
uv run barca assets show <id>                 Show asset detail
uv run barca assets refresh <id>              Trigger materialization
uv run barca assets refresh <id> -j <N>       Parallel partition workers
uv run barca sensors list                     List all sensors
uv run barca sensors show <id>                Show sensor detail + observation history
uv run barca sensors trigger <id>             Manually trigger a sensor
uv run barca jobs list                        List recent materializations
uv run barca jobs show <id>                   Show job detail
uv run barca reconcile                        Single-pass reconcile
uv run barca reconcile --watch                Continuous reconcile loop
uv run barca reconcile --watch --interval 30  Custom interval (seconds)
uv run barca serve                            HTTP API + background scheduler
uv run barca serve --port 8400                Custom port
uv run barca serve --interval 60              Custom reconcile interval
uv run barca serve --log-level info           Log level (debug, info, warning, error)
uv run barca reset [--db] [--artifacts] [--tmp]  Clean generated files
```

## reindex

Discover all `@asset()`, `@sensor()`, and `@effect()` decorated functions in your project. Computes definition hashes and upserts into the metadata database.

```bash
uv run barca reindex
```

## assets

### list

Show all indexed assets with their current status.

```bash
uv run barca assets list
```

### show

Show detailed information for a single asset, including its latest materialization.

```bash
uv run barca assets show 1
```

### refresh

Materialize an asset and all its upstream dependencies. For partitioned assets, use `-j` to control parallelism.

```bash
uv run barca assets refresh 1        # default: cpu_count workers
uv run barca assets refresh 1 -j 1   # sequential
uv run barca assets refresh 1 -j 8   # 8 parallel workers
```

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

## reconcile

Run a single reconciliation pass: reindex, topological sort, check staleness and schedule eligibility, execute ready nodes.

```bash
uv run barca reconcile                      # single pass
uv run barca reconcile --watch              # continuous loop (default 60s)
uv run barca reconcile --watch --interval 30
```

## serve

Start the FastAPI HTTP server with a background reconciliation scheduler.

```bash
uv run barca serve
uv run barca serve --port 8400 --interval 60 --log-level info
```

## reset

Remove generated files and caches.

```bash
uv run barca reset --db          # remove .barca/ (metadata database)
uv run barca reset --artifacts   # remove .barcafiles/ (artifacts)
uv run barca reset --tmp         # remove tmp/
uv run barca reset --db --artifacts --tmp  # everything
```
