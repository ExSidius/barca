# Caching, artifacts and environments

## What is cached

Each asset step has a **run hash**: a hash of the function's definition, its inputs' run hashes
and, for partitioned assets, the partition key. If the hash matches a previous successful
materialization, the artifact is reused and the function does not run. Change the function's
code or any upstream and the hash changes, so only the affected subgraph re-runs.

Tasks and sensors are never served from cache. Partitioned steps are not cache-checked yet,
so every partition re-runs each time (see `barca docs partitions`).

## Where things live

```
.barca/metadata.db                          run history and materialization records (local DB)
.barca/artifacts/<node>/<run_hash>.<ext>    one immutable file per materialization
```

`<ext>` is `.json`, `.pkl` or `.parquet` (see `barca docs types`). Artifacts are
content-addressed, so they can be shared between machines when remote state is configured
(`barca.toml`; see https://barca.sh/reference/config/).

## Controlling the cache

| Goal | Command |
|---|---|
| Normal, cache-aware | `barca get target pipeline.py` |
| Recompute everything in the cone | `barca get target pipeline.py --no-cache` |
| Run a task, cached upstream | `barca run task pipeline.py` |
| Run a task, refresh chosen upstream assets | `barca run task pipeline.py --refresh a,b` |
| Run a task, refresh all upstream assets | `barca run task pipeline.py --refresh-all` |

`barca run` previously refreshed every upstream asset by default and called the selective flag
`--burst`. The default is now cache-aware and the flag is `--refresh`.

## Environments

`--env <name>` (or `BARCA_ENV`, or `default_env` in `barca.toml`, else `default`) fully
separates cache, artifacts and shared state. Use it for dev/staging/prod isolation.

## Seeing what happened

```bash
barca history --json            # recent runs: status, steps executed, steps cached
barca stats total pipeline.py --json   # timing percentiles and cache hit rate for one asset
barca plan pipeline.py          # what would run, in phases, without running it
```

`steps_executed` in `barca get`'s JSON output is the number of steps that actually ran; a fully
cached second run reports 0 for assets.
