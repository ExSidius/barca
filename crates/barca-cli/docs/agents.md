# Using barca from scripts and AI agents

Conventions that make barca easy to drive programmatically. Everything here is stable CLI
behavior; `barca docs --json` and `--json` on `list`/`history`/`stats` give structured output.

## Output contract

- **stdout** carries the result: one JSON object for `get`/`run` (default `-o json`), the plan
  JSON for `plan`, or JSON for `list`/`history`/`stats` with `--json`. It is safe to parse.
- **stderr** carries progress (`[barca] 2/2 steps done in 0.0s`), your own `print` output from
  steps, warnings and errors. Use `--agent` for plain progress lines instead of a progress bar.
- **Exit codes:** `0` success; `1` runtime failure (a step raised, unknown target, task/asset
  misuse, no files); `2` usage error (bad flags or missing arguments, from the argument parser).
  On failure stdout is empty and stderr explains, including the Python traceback of a failed step.

```bash
barca get total pipeline.py --agent > result.json 2> progress.log
echo $?
```

`get`/`run` JSON fields: `run_id`, `elapsed_seconds`, `steps_executed` (0 means everything was
a cache hit), `phases`, `final_output`. `final_output` is the value for json artifacts and
`{"_barca_artifact": {"path", "format", "size_bytes"}}` for parquet and pickle
(`barca docs types`).

## Inspect before you run

```bash
barca list pipeline.py --json       # every node: id, kind, freshness, inputs
barca plan pipeline.py              # phases and steps that would run, nothing executes
barca history --json                # recent runs
barca stats total pipeline.py --json  # timings and cache hit rate for one asset
```

Planning is pure static analysis: it never imports your code and never runs a step.

## Getting values, not pointers

From Python, `barca.get` runs the command and deserializes the result:

```python
import barca

df = barca.get("orders", "pipeline.py")        # parquet artifact -> pandas DataFrame
total = barca.get("total", "pipeline.py")      # json artifact -> dict
barca.run("send_email", "pipeline.py", refresh=["report"])
```

`barca.plan`, `barca.history` and `barca.stats` return parsed dicts, and `barca.BarcaError` is
raised with stderr text on failure. Or read a parquet `path` directly with duckdb/pandas/polars.

## Targets and files

- `barca get file.py` gets every asset (final value is the last asset).
- `barca get name file.py [more.py ...]` gets one target; `name` can be the bare function name
  or the full id `file.py:name`. Cross-file inputs use `asset_ref("path.py:fn")`.
- `barca file.py` is shorthand for `barca get file.py`.
- `get` is for assets and `run` is for tasks; using the wrong one exits 1 and says which to use.

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
