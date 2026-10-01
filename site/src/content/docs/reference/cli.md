---
title: CLI Reference
description: All barca CLI commands — get, run, plan, history, stats, serve, list, version.
---

The `barca` binary is the entry point. Once installed (e.g. `uv add barca`), the `barca` command
is on your PATH.

## Commands

```
barca get [target] <file.py> [file.py ...]   Get asset value(s) — cache-aware
barca run <task> <file.py> [--refresh a,b | --refresh-all]  Run a task (always re-runs)
barca plan <file.py> [file.py ...]           Emit the execution plan as JSON
barca history [-l N]                          Show recent run history
barca stats <target> <file.py> [file.py ...]  Show timing/cache stats for an asset
barca serve [file.py ...] [--port N] [--watch] [--no-schedule] [--timezone TZ]
                                               Run the HTTP API server
barca list <file.py> [file.py ...]            List discovered definitions and their deps
barca docs [topic] [--all] [--json]           Built-in manual
barca version                                 Print version
barca --help                                  Show help
```

Shorthand: `barca pipeline.py` is rewritten to `barca get pipeline.py`.

## get

Execute the computation graph and return asset value(s). Cache-aware — only the needed subgraph
runs, and unchanged steps are served from cache.

Each completed step **fully materializes** its output to an artifact file under `.barca/artifacts/`
(json, pickle, or parquet). That write is the cache checkpoint — barca does not pass lazy
in-memory frames or query plans between workers. Downstream steps read the artifact back;
parameter type annotations (e.g. `data: pl.DataFrame`) select the parquet *reader* only.
To cache several outputs from one efficient computation, define multiple assets (or compute
them in one step and return the value you want cached).

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

Execute a task and its dependency cone. Tasks always re-run (they are never cached). Upstream
assets are cache-aware by default, exactly like `barca get`. Use `--refresh` to force
re-materialize only named upstream assets, or `--refresh-all` (alias `--no-cache`) to refresh every
upstream asset in the cone.

```bash
barca run deploy pipeline.py                          # run task, upstream assets from cache
barca run deploy pipeline.py --refresh fetch,transform  # re-materialize only named assets
barca run deploy pipeline.py --refresh-all            # re-materialize all upstream assets
barca run deploy pipeline.py --no-cache               # same as --refresh-all
```

Unlike `barca get`, which targets assets and respects the cache, `barca run` is for tasks that
produce side effects (deploys, notifications, reports). If a task must see fresh upstream data,
pass `--refresh-all`.

> **Behavior change:** `barca run` previously force-rerun every upstream asset by default and took
> `--burst`. Add `--refresh-all` to restore the old default; `--burst a,b` is now `--refresh a,b`.

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
barca history --json   # machine-readable array of runs
```

## stats

Show aggregated execution statistics for a single asset: total materializations, timing
percentiles (avg / median / p95 / max), cache hit rate, and recent runs.

```bash
barca stats summary pipeline.py
barca stats summary pipeline.py --json   # the same as one JSON object
```

## serve

Start a long-running HTTP server that exposes the orchestrator as a JSON API. Binds to
`127.0.0.1` (local only, no auth). See [Server API](/reference/server-api/) for the full endpoint
reference.

```bash
barca serve pipeline.py                 # default port 8274
barca serve pipeline.py --port 8400     # custom port
barca serve pipeline.py --watch         # dev mode: re-parse the DAG on file change
barca serve pipeline.py --no-schedule   # disable the cron scheduler
barca serve pipeline.py --timezone utc  # evaluate cron in UTC (default: local)
```

`--watch` is a local-development convenience and is off by default; a production deployment serves
a fixed set of files and does not need it.

`barca serve` does not yet support shared remote state — if `barca.toml` resolves to
`state = "optimistic"` with a state URI, `serve` refuses to start with an error telling you to set
`state = "off"` (or `BARCA_STATE=off`) to serve with a local metadata DB. See
[Configuration](/reference/config/).

## list

List all discovered definitions (assets, tasks, sensors) with their kind, freshness, and
dependencies. Scheduled definitions also show their next fire time in local time (to the
second, so sub-minute schedules are legible).

```bash
barca list pipeline.py
barca list pipeline.py --json   # array of {id, kind, freshness, inputs, next_fire?}
```

## docs

The manual, compiled into the binary: it works offline and always matches the installed version.
Every command's `--help` also ends with runnable examples.

```bash
barca docs                    # topic index with one-line summaries
barca docs types              # one topic as markdown (output formats, annotations, duckdb)
barca docs examples/duckdb    # a runnable example pipeline
barca docs --all              # every topic in one stream
barca docs --json             # topic index as JSON; add a topic for its full text
```

Topics: `overview`, `assets`, `types`, `tasks`, `cache`, `partitions`, `sinks`, `scheduling`,
`agents`, and `examples/*`. `barca docs agents` describes the output contract for scripts and AI
agents: JSON on stdout, progress and errors on stderr, exit code `0` success / `1` runtime
failure / `2` usage error.

## version

```bash
barca version
```

## --env

`get`, `run`, `plan`, `serve`, `history`, and `stats` accept `--env <name>`
(default: `BARCA_ENV`, then `default_env` in barca.toml, then `default`).
Environments fully separate cache, artifacts, and shared remote state — see
[Configuration](/reference/config/).
