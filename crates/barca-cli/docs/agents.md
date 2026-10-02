# Using barca from scripts and AI agents

Conventions that make barca easy to drive programmatically. Everything here is stable CLI
behavior. When stdout is not a terminal (a pipe, a subprocess, an agent) every result is JSON
without any flag. List-shaped output is bounded by default (see "Bounded output" below).

## Output format: JSON unless stdout is a terminal

`get`, `run`, `list`, `history` and `stats` pick their stdout format by one rule, first match wins:

1. **A flag:** `--json` forces JSON, `--pretty` forces human output (tables, summaries).
   `get`/`run` also keep `-o json|value|pretty`; `-o value` prints only the final value.
   `--fields` implies JSON.
2. **`BARCA_OUTPUT=json` or `BARCA_OUTPUT=pretty`** in the environment (for CI or a shell
   profile). Any other value is a usage error (exit 2).
3. **The terminal:** stdout is a TTY → human output; anything else → JSON.

So `barca list pipeline.py` shows a table in your terminal, and `barca list pipeline.py | cat`
or a subprocess call gets JSON. Pass `--json` anyway in scripts: it states the intent
and survives someone setting `BARCA_OUTPUT=pretty`. `plan` always prints JSON and `docs` always
prints markdown (`barca docs --json` for JSON); neither follows the rule.

```bash
barca list pipeline.py --json        # JSON even in a terminal
barca history --pretty               # a table even when piped
BARCA_OUTPUT=json barca get total pipeline.py
```

## Output contract

- **stdout** carries the result: one JSON object for `get`/`run`, the plan JSON for `plan`, and
  JSON for `list`/`history`/`stats` (whenever the rule above picks JSON). It is safe to parse.
- **stderr** carries progress (`[barca] 2/2 steps done in 0.0s`), your own `print` output from
  steps, warnings and errors. The progress bar draws only when stderr is a terminal; barca
  writes no ANSI colour or cursor codes to a stream that is not one. Use `--agent` for plain
  progress lines (`[barca] 1/2 ...`) on stderr instead of a progress bar.
- **Exit codes:** one per kind of failure, so you can decide what to do from the code alone.
  On failure stderr explains (see Errors below). stdout is empty, except that a failed step in
  JSON mode still prints a result line with `"status": "failed"` (and a run with several targets
  prints its full result; see below).

| Code | `kind`        | Meaning                                                                         | What to do                    |
|------|---------------|---------------------------------------------------------------------------------|-------------------------------|
| 0    |               | success                                                                         |                               |
| 1    | `step_failed` | a step of yours raised (traceback included); the run is recorded as failed      | fix the code, re-run          |
| 2    | `usage`       | bad flags or arguments, unknown target, task/asset misuse, unreadable or invalid `.py` file, invalid `--env` or barca.toml | fix the command |
| 3    | `infra`       | barca or its environment failed: metadata DB, worker pool, remote state, I/O    | not your code; retrying may help |
| 130  | `cancelled`   | interrupted (Ctrl-C)                                                            | re-run                        |

```bash
barca get total pipeline.py --agent > result.json 2> progress.log
echo $?
```

## Errors

In JSON output mode (whenever the output rule above picks JSON: piped or captured stdout,
`--json`, `-o json` or `BARCA_OUTPUT=json`; `plan` always; `docs` with `--json`) an error is
**one JSON line**, the last line on stderr:

```
{"code":2,"error":"Asset 'nope' not found. Available: pipeline.py:src, pipeline.py:total, pipeline.py:clean","kind":"usage","remediation":"Run `barca list pipeline.py` to see every node and its kind."}
```

| Field          | Always | Meaning                                                              |
|----------------|--------|----------------------------------------------------------------------|
| `error`        | yes    | what went wrong                                                      |
| `code`         | yes    | the exit code (table above)                                          |
| `kind`         | yes    | `usage`, `step_failed`, `infra` or `cancelled`                       |
| `remediation`  | yes    | what to do next, often a command to run                              |
| `node`         | `step_failed` | the failing step's id, e.g. `pipeline.py:clean`               |
| `traceback`    | `step_failed` | the Python traceback of your code (barca frames removed), or null |
| `artifact_dir` | `step_failed` | where that step's artifacts are stored (a path or remote URI); it may not exist if the step never succeeded |

A failed step looks like this (one line; wrapped here):

```
{"artifact_dir":".barca/artifacts/pipeline.py--clean","code":1,
 "error":"step 'pipeline.py:clean' failed: ZeroDivisionError: division by zero","kind":"step_failed",
 "node":"pipeline.py:clean","remediation":"Fix the error in 'pipeline.py:clean' (see the traceback) and re-run the same command. Steps that succeeded are cached and will not re-run.",
 "traceback":"  File \"/abs/path/pipeline.py\", line 11, in clean\n    return x / 0\n           ~~^~~"}
```

Parse it from the last stderr line; earlier lines are progress and your steps' own output.
In human mode (a terminal, `--pretty`, `-o pretty` or `-o value`) the same error is plain prose with the
remediation on the last lines. Errors never go to stdout. From Python, `barca.BarcaError` carries
the envelope as attributes: `kind`, `code`, `remediation`, `node`, `traceback`, `artifact_dir`.

When a step fails in JSON mode, `get`/`run` still print one result line on stdout, so you can
read the outcome without parsing stderr: `{"status": "failed", "failed_node": ..., "error": ...,
"run_id", "steps", ...}`, where the failed step's `status` is `failed`. A successful result has
`"status": "success"`. Just before the error, stderr gets one greppable line:
`[barca] run failed: step 'pipeline.py:clean' failed (exit 1)`.

`get`/`run` JSON fields: `status` (`success`, or `failed` as above), `run_id`, `elapsed_seconds`,
`steps_executed` (0 means everything was a cache hit), `phases`, `steps` (what happened to each
step: `status` ran/cached/partial and why, plus `env`, the declared environment variable values
used, for nodes with `env=[...]`),
`final_output`. `final_output` is the value for json artifacts and
`{"_barca_artifact": {"path", "format", "size_bytes"}}` for parquet and pickle
(`barca docs types`).

### Several targets in one call

`barca run a,b pipeline.py` (or `barca get a,b ...`) runs every named target in one invocation:
one plan over the union of their cones, so shared upstream steps run once. Use it instead of N
calls. The output has the same run fields (`status` is `failed` when any target failed), but
`final_output` is replaced by `targets`, keyed by target name in the order given:

```json
{"status": "failed", "run_id": "...", "elapsed_seconds": 0.2, "steps_executed": 5, "phases": 2, "steps": [...],
 "targets": {"check_a": {"status": "success", "final_output": {"a_ok": true}},
             "boom": {"status": "failed", "failed_step": "pipeline.py:boom",
                      "error": "ValueError: check failed\n  File ..."}}}
```

- Every target runs even if another fails; a failure skips only the steps that depend on it
  (`"status": "skipped"`, reason `upstream_failed`, in `steps`). `failed_step` names the step that
  raised: the target itself or something upstream of it.
- Exit code 1 if any target failed. stdout carries the full JSON (so you see which targets
  passed); stderr has one `error: target '<name>' failed ...` line each, then the error envelope
  for the first failed target (`kind: "step_failed"`).
- One target (or a repeated name, `a,a`) gives exactly the single-target output.
- `--dry-run` with several targets reports the union once, with `"targets": [names]` in place of
  `"target"`. `-o value` prints `{target: value}` (null for a failed target).
- Every name is checked before anything runs: an unknown name, a task passed to `get`, or an
  empty name (`a,,b`) is a usage error, exit 2, and nothing runs.

```bash
barca run check_a,check_b pipeline.py --agent > result.json
barca run check_a,check_b pipeline.py --dry-run
```

## Parallel runs

It is safe to run several `barca` commands in one project at the same time: they queue briefly
on the metadata DB (see `barca docs cache`) instead of failing with a lock error.

## Long-running steps

Nothing is printed while a step is executing, so a slow step used to look hung. Any step that
stays in flight for 15 seconds or more is now reported on stderr, in every mode, and again on
each later interval:

```
[barca] still running (45s): pipeline.py:fetch_orders
```

Set `BARCA_PROGRESS_SECS` to change the interval (`0` turns it off). A completed step appears as
`[barca] step:<id> completed ...` in `--agent` mode. If neither a completion nor a "still running"
line has appeared for much longer than your slowest step, the process is genuinely stuck.

## Environment variables

A node that declares `env=["SOURCE_CSV"]` has those values in its cache key, and each `--agent`
step line ends with them so a log records which inputs a run used:

```
[barca] step:pipeline.py:raw completed 0.0s (1/2) env API_TOKEN=<unset> SOURCE_CSV=b.csv
[barca] step:pipeline.py:raw cached env API_TOKEN=<unset> SOURCE_CSV=b.csv
```

`<unset>` means the variable was not set; secret-looking names (`*_TOKEN`, `*_SECRET`, `*_KEY`,
`*_PASSWORD`) show `<redacted>`. Values with spaces are double-quoted. Undeclared variables are
invisible to barca (`barca docs assets`).

## Refreshing: syntax and pitfalls

- Several assets are one comma-separated list: `--refresh a,b`. Never `--refresh a b`.
- `--refresh` re-runs only what you name. Cached assets downstream of a refreshed one are not
  recomputed and barca warns on stderr (`... does not reflect the refresh`). Name the whole chain
  or use `--refresh-all`. Details: `barca docs cache`.
- An unknown name is an error (exit 2, `kind: usage`) listing the valid upstream assets.

## Inspect before you run

Preview any `get`/`run` with `--dry-run`: it says which steps would run, which come from cache,
and why, and writes nothing (`barca docs cache`):

```bash
barca run report pipeline.py --dry-run --refresh src
```

```bash
barca list pipeline.py --json       # {nodes: [{id, kind, freshness, inputs, env}], total, truncated}
barca plan pipeline.py              # phases and steps that would run, nothing executes
barca history --json                # {runs: [...], total, truncated}: the last 10 runs
barca stats total pipeline.py --json  # timings and cache hit rate for one asset
```

Planning is pure static analysis: it never imports your code and never runs a step.

## Bounded output: --limit, --all, --fields

List-shaped commands print a bounded number of items so a large project cannot flood your
context: `barca list` shows at most 100 nodes (in topological order) and `barca history` the 10
most recent runs. Their JSON is an envelope that says whether you saw everything:

```json
{"nodes": [...], "total": 312, "truncated": true,
 "hint": "pass --limit N for more, or --all for all 312 nodes"}
```

`truncated` and `total` are always present; `hint` only when `truncated` is true. The human table
prints the same hint as one line on stderr, so stdout stays just the table.

```bash
barca list pipeline.py --limit 20   # first 20 nodes
barca list pipeline.py --all        # every node
barca history -l 50 --json          # last 50 runs
barca history --all --json          # every recorded run
```

`--fields a,b` keeps only those keys on each item: `nodes` for `list`, `runs` for `history`,
`recent_runs` for `stats`, `steps` for `get`/`run` (including `--dry-run`), and `topics` for
`docs`. The rest of the JSON is unchanged. `--fields` implies JSON on every command, even in a
terminal; combining it with `--pretty`, `-o pretty` or `-o value` is a usage error. A key that is valid but absent on an item (for example
`next_fire` on an unscheduled node) is simply omitted. An unknown key is a usage error (exit 2)
that lists the valid keys, and nothing runs; `barca <command> --help` lists them too.

```bash
barca list pipeline.py --fields id,inputs
barca history --fields run_id,status,elapsed_seconds
barca get total pipeline.py --fields id,status,reason
```

Limits bound how many items are printed, never what an item says: error messages and the Python
traceback of a failed step are always complete.

Breaking change after 0.9.0: `list --json` and `history --json` used to print a bare array; they now print
the envelope above. Read `.nodes` / `.runs` (for example `jq '.nodes[].id'`).

## Getting values, not pointers

From Python, `barca.get` runs the command and deserializes the result:

```python
import barca

df = barca.get("orders", "pipeline.py")        # parquet artifact -> pandas DataFrame
total = barca.get("total", "pipeline.py")      # json artifact -> dict
barca.run("send_email", "pipeline.py", refresh=["report"])
```

`barca.plan`, `barca.history` and `barca.stats` return parsed dicts, and `barca.BarcaError` is
raised on failure; for `get`/`run`/`plan` its `kind`, `code`, `remediation` (and for a failed step
`node`, `traceback`, `artifact_dir`) come from the error envelope. Or read a parquet `path` directly with duckdb/pandas/polars.

## Targets and files

- `barca get file.py` gets every asset (final value is the last asset).
- `barca get name file.py [more.py ...]` gets one target; `name` can be the bare function name
  or the full id `file.py:name`. Cross-file inputs use `asset_ref("path.py:fn")`.
- `barca get a,b file.py` / `barca run a,b file.py` take several targets in one run (see above).
- `barca file.py` is shorthand for `barca get file.py`.
- `get` is for assets and `run` is for tasks; using the wrong one exits 2 and says which to use.
- The target comes before the files. If the first positional ends in `.py` and a later one does
  not, there is exactly one valid reading, so barca exits 2 and prints the corrected command
  (same files and flags) instead of running anything:

  ```
  $ barca run pipeline.py report
  error: the target comes before the files

    barca run report pipeline.py

  Run `barca list pipeline.py` to see available assets and tasks.
  ```

  With more than one non-`.py` name after a file it states the rule and does not guess. barca
  never offers fuzzy "did you mean" suggestions; run `barca list <files>` to find a name.

## Editing a barca project: a safe loop

1. Write or change the function with parameter annotations (`barca docs types`).
2. `barca list pipeline.py` — confirm the node, its kind and its dependencies were discovered.
3. `barca get <target> pipeline.py` — check exit code, `steps_executed`, and `final_output`.
4. Run it again — `steps_executed` should be 0 (cached). If not, something upstream changed.
5. `barca plan` / `barca history --json` when you need to explain what ran.

## Finding more

```bash
barca docs                    # topic index
barca docs <topic>            # one topic as markdown
barca docs --all              # the whole manual in one stream (paste into context)
barca docs --json             # topic index as JSON; add a topic for its full text
barca get --help               # flags and runnable examples (every command has --help)
```

Online: https://barca.sh/
