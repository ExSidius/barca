# Caching, artifacts and environments

## What is cached

Each asset step has a **run hash**: a hash of the function's definition, its inputs' run hashes,
for partitioned assets the partition key, and the values of any environment variables it declares
with `env=[...]`. If the hash matches a previous successful materialization, the artifact is
reused and the function does not run. Change the function's code, any upstream, or a declared
environment variable and the hash changes, so only the affected subgraph re-runs.

Environment variables a function reads **without** declaring them are not part of the hash:
changing one does not invalidate anything. Declare them with `@asset(env=["NAME"])`
(`barca docs assets`). Nodes that declare no env hash exactly as they did before `env=` existed,
so upgrading does not invalidate existing caches.

Tasks and sensors are never served from cache. Partitioned assets are cached per key (see
`barca docs partitions`).

A sensor's *output* is not part of its consumers' run hashes. A sensor has no inputs, so its run
hash depends only on its code; when it returns a new value (a new blob etag, say), an asset that
reads it keeps the same run hash and is served from cache with the old data. To pick up external
data that changed in place, refresh the asset that reads it: `--refresh bronze` (below).

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
| Run a task, refresh chosen upstream assets and everything downstream of them | `barca run task pipeline.py --refresh a,b` |
| Run a task, refresh only the chosen assets | `barca run task pipeline.py --refresh a,b --no-cascade` |
| Run a task, refresh all upstream assets | `barca run task pipeline.py --refresh-all` |

`barca run` previously refreshed every upstream asset by default and called the selective flag
`--burst`. The default is now cache-aware and the flag is `--refresh`.

Previously `--refresh` did not cascade: it re-ran only the named assets and left their downstream
assets cached. It now cascades by default; `--no-cascade` keeps the old behavior.

### Exactly what `--refresh` does

- `--refresh a,b` re-materializes the assets you name **and every asset downstream of them** in
  the task's cone (the cascade), so the refreshed data reaches the task. A step re-run by the
  cascade reports `reason: "refresh_cascade"` and a `detail` naming the asset it cascaded from.
- It takes one comma-separated list; `--refresh a b` is an error ("'b' is not a .py file"). A
  name that is not an upstream asset of the task is an error that lists the valid names, so a
  typo never silently does nothing.
- It does **not** rebuild the upstream of what you name, or assets in the cone that do not depend
  on it. Those keep serving from cache.
- `--no-cascade` re-materializes **only** the assets you name. Run hashes cover definitions and
  upstream hashes, not output contents, so a cached downstream asset still matches and the
  refreshed data never reaches it. Barca prints
  `warning: 'mid' was served from cache but depends on refreshed 'src' ...` when this happens.
  `--no-cascade` without `--refresh` is a usage error (exit 2).

Use `--refresh` when external data changed in place (a blob overwritten at the same path): name
the asset that reads it, and everything built from it re-runs.

## Seeing what will happen: `--dry-run`

`barca get` and `barca run` take `--dry-run`. It reports, for exactly that command and flags, which
steps would be served from cache and which would run, and why. It executes nothing and writes
nothing: no `.barca` directory is created and no run is recorded.

```bash
barca run report pipeline.py --dry-run --json          # JSON on one line
barca run report pipeline.py --dry-run --pretty        # a table for humans
barca run report pipeline.py --dry-run --refresh src   # preview a refresh and its cascade
barca get total pipeline.py --dry-run --no-cache
```

```json
{"dry_run": true, "command": "run", "target": "report",
 "steps": [{"id": "pipeline.py:src", "kind": "asset", "action": "cached",
            "run_hash": "…", "artifact": ".barca/artifacts/…"},
           {"id": "pipeline.py:report", "kind": "task", "action": "run",
            "reason": "task", "detail": "tasks always re-run"}],
 "summary": {"will_run": 1, "cached": 1, "unknown": 0}}
```

Each step has an `action`:

| `action` | Meaning |
|---|---|
| `cached` | Served from the cache (`artifact` is the file). |
| `run` | Will execute; `reason` says why (below). |
| `partial` | A partitioned asset where some keys are cached; `partitions` lists the counts and the keys that will run. |
| `unknown` | Cannot be known without running: a dynamic partition (`partitions_from`) whose source has to run first to produce its keys, and anything that depends on it. |

`reason` is one of `task` and `sensor` (always run), `no_cache` (`--no-cache`), `refresh` (named in
`--refresh`), `refresh_cascade` (downstream of an asset named in `--refresh`), `refresh_all`, or
`not_materialized` (no cached result for this code and these inputs: never run, or the code or an
upstream changed). Under `--no-cascade`, a cached step downstream of a refreshed asset carries a
`warning` (see the refresh notes above). `summary` counts steps, one per partition
key.

A dry run makes the same decisions a real run makes (it calls the same code), and the test suite
checks that `will_run` equals the real run's `steps_executed` across cold, warm, `--refresh`,
`--refresh --no-cascade` and `--refresh-all` runs.

## What a run reports

A real `barca get` / `barca run` returns the same per-step information in a `steps` array, with a
`status` of `ran`, `cached` or `partial` (and the same `reason` / `warning`). In `--agent` mode a
cached step also prints `[barca] step:<id> cached` on stderr, so a log shows what was served from
cache as well as what ran. A step whose node declares `env=[...]` also carries `env`, the values
it was hashed with (`null` when unset, `<redacted>` for secret-looking names), in both the JSON
`steps` entry and the `--agent` line (`... env SOURCE_CSV=b.csv`). `barca history --json` and `barca stats` show the same over time.

## Concurrent runs

Several barca processes can run in one project at once (parallel scripts or agents, `barca serve`
alongside the CLI). The metadata DB is a single-file database that one process opens at a time,
so each process holds a short lock on it (`.barca/metadata.db.lock`) only while it reads or
writes, and releases it while your Python runs. Processes queue instead of failing. If a
process waits more than 60 seconds for the lock you get an error that names the lock file;
an `... File is locked by another process` error means something outside barca (a DB browser, a
backup tool, an older barca) has `.barca/metadata.db` open.

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
