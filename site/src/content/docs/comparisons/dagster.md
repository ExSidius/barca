---
title: "Barca vs Dagster"
description: New-user experience comparison — install footprint, steps to first output, error handling.
---

Tested 2026-06-10. Barca 0.2.0, Dagster 1.13.8 (latest at time of test).
Both installed from PyPI via `uv` on macOS (Apple Silicon), Python 3.14.

## Install footprint

| Metric | barca | dagster | Ratio |
|--------|-------|---------|-------|
| Packages installed | 1 | 100 | 100x |
| Venv size | 16 MB | 301 MB | 19x |
| Lockfile entries | 2 | 109 | 55x |
| Wheel download | 6.9 MB | 30+ MB | ~4x |

Barca's only dependency is itself — the wheel contains a Rust binary and Python stubs.
Dagster pulls in cryptography, uvloop, sqlalchemy, grpcio, graphql, starlette, pydantic,
protobuf, and ~90 transitive dependencies for an empty scaffold.

## Steps to first output

### Barca (3 steps)

```bash
uv add barca                    # install
cat > pipeline.py << 'PY'       # write code
from barca import asset

@asset()
def raw_data() -> list[dict]:
    return [{"x": 1}, {"x": 2}, {"x": 3}]

@asset(inputs={"data": raw_data})
def summary(data: list[dict]) -> dict:
    return {"count": len(data), "total": sum(d["x"] for d in data)}
PY
barca get pipeline.py            # run
```

### Dagster (6–7 steps)

```bash
uvx create-dagster@latest project myproj   # scaffold (interactive prompt)
cd myproj && source .venv/bin/activate     # enter project
dg scaffold defs dagster.asset assets.py   # generate boilerplate
# edit src/myproj/defs/assets.py           # write code
uv add pandas                              # install deps for the quickstart
dg launch --assets my_asset                # run
```

The dagster quickstart also requires creating a data directory and CSV file before
the asset can run, bringing the real count to 7–8 steps.

## Running the same 2-asset pipeline

| Metric | barca | dagster |
|--------|-------|---------|
| Command | `barca get pipeline.py` | `dg launch --assets raw_data,summary` |
| Total time | 240ms | 1,550ms |
| Output lines | 2 (progress + JSON) | 29 (all DEBUG) |
| Result access | JSON on stdout | Pickled to temp dir |
| Error output | 1 line | 25+ lines with internal frames |

### Barca output

```
[barca] 2/2 steps done in 0.0s
{"elapsed_seconds":0.241,"final_output":{"count":3,"total":6},"phases":1,"run_id":"...","steps_executed":2}
```

### Dagster output (abridged — actual is 29 lines)

```
2026-06-10 ... - dagster - DEBUG - RUN_START - Started execution of run for "__ASSET_JOB".
2026-06-10 ... - dagster - DEBUG - ENGINE_EVENT - Executing steps using multiprocess executor
2026-06-10 ... - dagster - DEBUG - raw_data - STEP_WORKER_STARTING - Launching subprocess
... (12 more lines for raw_data) ...
2026-06-10 ... - dagster - DEBUG - summary - STEP_WORKER_STARTING - Launching subprocess
... (12 more lines for summary) ...
2026-06-10 ... - dagster - DEBUG - ENGINE_EVENT - parent process exiting after 1.55s
2026-06-10 ... - dagster - DEBUG - RUN_SUCCESS - Finished execution of run for "__ASSET_JOB".
```

## Error handling comparison

### Barca

```
[barca] 0/1 steps done in 0.0s
Worker failed: something went wrong
```

### Dagster

```
dagster._core.errors.DagsterExecutionStepExecutionError: Error occurred while executing op "oops"::
ValueError: something went wrong

Stack Trace:
  File ".../dagster/_core/execution/plan/utils.py", line 57, in op_execution_error_boundary
    yield
  File ".../dagster/_utils/__init__.py", line 394, in iterate_with_context
    next_output = next(iterator)
  File ".../dagster/_core/execution/plan/compute_generator.py", line 136, in _coerce_op_compute_fn_to_iterator
    ...
  File "broken.py", line 5, in oops
    raise ValueError("something went wrong")
```

The user's error is 6 frames deep in dagster internals.

## What dagster does well (steal these)

### `dg list defs` — discoverability command

Shows a clean table of every asset, deps, group, kind. Answers "what does the framework
see?" without running anything. Barca's `barca list` command (added in 0.2.1) fills this gap.

### Scaffolded `.gitignore`

Dagster's scaffold includes a `.gitignore` covering framework artifacts. Barca silently
creates `.barca/` (with a 480KB WAL file) and doesn't mention it.

### Auto-discovery in `defs/` folder

Drop a `.py` file in the defs folder and it's picked up automatically. No registration.
(Barca already does this with explicit file arguments, but multi-file discovery from a
project root is a good future direction.)

## What dagster gets wrong (avoid these)

### Framework internals in error output

25 lines of dagster stack frames for a simple ValueError. Default should show only
the user's error. Framework traces belong behind `--verbose`.

### Two CLIs

`dagster` (old, deprecated) and `dg` (new). Every Google result and Stack Overflow answer
points to the old one. Never split the CLI — evolve subcommands within one binary.

### Mandatory project structure

Dagster requires pyproject.toml, src layout, definitions.py, defs/ folder before anything
runs. `barca get any_file.py` is the killer feature. Never gate it behind project structure.

### Verbose by default

14 DEBUG lines per asset per run with no quiet mode. The right default is what barca does:
progress + result. Lifecycle events belong behind `--verbose` or `--log-level debug`.

### Interactive prompts in non-interactive contexts

`create-dagster` prompts "Run uv sync? (y/n)" with no `--yes` flag. Blocks CI, scripts,
and automated workflows.

### Venv mismatch warning spam

Every `dg` command prints a 5-line warning unless the venv is activated. Running
`.venv/bin/dg` should just work without warnings.

## The meta-lesson

Dagster's UX problems stem from one design choice: the framework assumes it's the center
of your world. It wants you in its project structure, its web UI, its process model, its
logging format.

Barca's thesis is the opposite — the orchestrator is invisible. As features are added
(scheduling, server, integrations), each one will push toward making the framework more
visible. The discipline is to keep asking: "can this feature work without the user knowing
barca is there?"
