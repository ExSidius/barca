# CLI Reference

The `barca` binary is the entry point. Once installed (e.g. `uv add barca`), the `barca` command
is on your PATH.

## Commands

```
barca get [target] <file.py> [file.py ...]   Get asset value(s) — cache-aware
barca run <task> <file.py> [--burst a,b]     Run a task (always re-runs)
barca plan <file.py> [file.py ...]           Emit the execution plan as JSON
barca history [-l N]                          Show recent run history
barca stats <target> <file.py> [file.py ...]  Show timing/cache stats for an asset
barca serve [file.py ...] [--port N] [--watch] Run the HTTP API server
barca version                                 Print version
barca --help                                  Show help
```

Shorthand: `barca pipeline.py` is rewritten to `barca get pipeline.py`.

## get

Execute the computation graph and return asset value(s). Cache-aware — only the needed subgraph
runs, and unchanged steps are served from cache.

If the first positional argument ends in `.py`, all arguments are treated as files (gets all
assets, returning the final asset's value). Otherwise the first argument is the target asset name
and the rest are files.

```bash
barca get pipeline.py                 # all assets
barca get summary pipeline.py         # a specific target
barca get pipeline.py --no-cache      # execute everything fresh
barca get pipeline.py --agent         # plain progress lines instead of a progress bar
barca get pipeline.py -o value        # print just the final value (also: json | pretty)
```

## run

Execute a task and its dependency cone. Tasks always re-run (they are never cached). By default,
all upstream assets in the cone are also force-rerun (cache-busted). Use `--burst` to selectively
re-run only named upstream assets while leaving others cache-aware.

```bash
barca run deploy pipeline.py             # run task + bust all upstream caches
barca run deploy pipeline.py --burst fetch,transform  # only bust named assets
```

Unlike `barca get`, which targets assets and respects the cache, `barca run` is for tasks that
produce side effects (deploys, notifications, reports). The task's upstream assets are re-run by
default to ensure the task sees fresh data.

## plan

Parse the source files and emit the tiered execution plan as JSON, without running anything.

```bash
barca plan pipeline.py
```

## history

Show recent runs from `.barca/metadata.db` — run id, command, status, step counts, and timing.

```bash
barca history          # last 10 runs
barca history -l 25    # last 25
```

## stats

Show aggregated execution statistics for a single asset: total materializations, timing
percentiles (avg / median / p95 / max), cache hit rate, and recent runs.

```bash
barca stats summary pipeline.py
```

## serve

Start a long-running HTTP server that exposes the orchestrator as a JSON API. Binds to
`127.0.0.1` (local only, no auth). See [Server API](server-api.md) for the full endpoint
reference.

```bash
barca serve pipeline.py                 # default port 8274
barca serve pipeline.py --port 8400     # custom port
barca serve pipeline.py --watch         # dev mode: re-parse the DAG on file change
```

`--watch` is a local-development convenience and is off by default; a production deployment serves
a fixed set of files and does not need it.

## version

```bash
barca version
```
