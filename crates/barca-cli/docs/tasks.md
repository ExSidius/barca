# Tasks and `barca run`

A task is a workflow step that *does* something (deploy, notify, migrate, warm a cache). Tasks
always re-run and are never cached.

```python
from barca import asset, task


@asset()
def report() -> dict:
    return {"rows": 42}


@task(inputs={"data": report})            # asset -> task: receives the report
def send_email(data: dict) -> None:
    print(f"sending report with {data['rows']} rows")


@task()
def migrate() -> None:
    print("migrating")


@task(inputs={"_migrate": migrate})       # leading "_": ordering only, receives None
def notify(_migrate) -> None:
    print("migration done")
```

## Rules

- A task may depend on assets, sensors or other tasks, and may sit anywhere in the graph.
- A task must **not** be an input to an asset or sensor (its output is never cached, so a
  cacheable node downstream of it would be permanently stale).
- An `inputs` key starting with `_` means "run after, but do not load the data": the
  parameter receives `None` and no artifact is deserialized.
- `@task(freshness=Schedule("<cron>"))` runs on a timer under `barca serve`.

## Running tasks

```bash
barca run send_email pipeline.py                      # task runs; upstream assets come from cache
barca run send_email pipeline.py --refresh report     # also re-materialize these upstream assets
barca run send_email pipeline.py --refresh-all        # re-materialize every upstream asset
barca run send_email pipeline.py --no-cache           # same as --refresh-all
```

`barca run` is cache-aware for upstream assets, exactly like `barca get`; only the task always
executes. A task must be the target: `barca get` on a task is an error, and `barca run` on an
asset is an error.

## Fan-out from inside a task

`parallel(partial(f, x), ...)` and `parallel_map(f, items)` run other `@task` functions in
parallel worker processes and return results in argument order. A failed branch comes back as
a `ParallelError` instead of raising. They are recognized inside `@task` bodies only.

## When a task fails

A task (or asset) that raises, or calls `sys.exit()` with any code, fails the run: barca prints
the traceback on stderr and exits 1, and nothing downstream runs. To fail on purpose, for example
on a validation error, raise an exception. A failed `parallel()` branch fails the run only if the
parent task raises. Exit codes: `barca docs agents`.

See also: `barca docs cache`, `barca docs examples/deploy-task`.
