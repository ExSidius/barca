# Decorators

Core decorators for defining assets, sensors, tasks, sinks, and related primitives.

## @asset

```python
@asset(
    name: str | None = None,
    inputs: dict[str, AssetRefLike] | None = None,
    partitions: dict[str, PartitionSpecLike] | None = None,
    serializer: SerializerKind | None = None,
    freshness: Freshness = Always,
    timeout_seconds: int = 300,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Declares a cacheable, provenance-tracked asset. The default freshness is `Always` — the asset is kept up to date automatically during `barca run`.

```python
from barca import asset, Always, Manual, Schedule

@asset()                                    # freshness=Always (default)
def my_asset() -> dict:
    return {"x": 1}

@asset(freshness=Manual)                    # only via explicit refresh
def pinned_data() -> dict:
    return {"x": 1}

@asset(freshness=Schedule("0 5 * * *"))     # daily at 05:00
def daily_report() -> dict:
    return {"x": 1}
```

`Manual` freshness blocks downstream `Always` assets from auto-updating — a downstream asset cannot be fresher than its most-upstream `Manual` dependency.

::: barca.asset
    options:
      show_source: false

## AssetWrapper

::: barca.AssetWrapper

## @sink

```python
@sink(
    path: str,
)
```

Stacked on an `@asset` to write the asset's output to a path when it materialises. Paths are fsspec-compatible (local, `s3://`, `gs://`, etc.). Multiple `@sink` decorators may be stacked on the same asset.

```python
from barca import asset, sink, Always

@asset(freshness=Always)
@sink('./output.json')
@sink('s3://my-bucket/output.json')
def banana() -> dict:
    return {'a': 1}
```

Sinks are leaf nodes — no other asset may list a sink as an input. A sink failure does not fail the parent asset, but is surfaced prominently in logs.

::: barca.sink
    options:
      show_source: false

## @sensor

```python
@sensor(
    name: str | None = None,
    freshness: Manual | Schedule = Manual,
    timeout_seconds: int = 300,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Declares an external-state observer. Sensors must use `Manual` or `Schedule` freshness — `Always` is not valid for sensors (polling frequency must be declared explicitly).

Sensors return `(update_detected: bool, output)` tuples. The full tuple is passed as input to downstream assets.

```python
from barca import sensor, Schedule

@sensor(freshness=Schedule("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    files = list(Path("inbox").glob("*.csv"))
    return len(files) > 0, [str(f) for f in files]
```

Sensors are source nodes only — they have no upstream inputs.

::: barca.sensor
    options:
      show_source: false

## SensorWrapper

::: barca.SensorWrapper

## @task

```python
@task(
    name: str | None = None,
    inputs: dict[str, NodeRefLike] | None = None,
    freshness: Freshness = Always,
    timeout_seconds: int = 300,
    retries: int = 1,
    retry_backoff: float = 0.0,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Declares a **task** — a workflow-management step such as a deploy, notification,
migration, or cache warm. Tasks always re-run and are never cached, so they're
the right home for "do something" operations that don't produce cacheable data.

- They may appear **anywhere** in the graph (not just at the leaves).
- They may depend on assets, sensors, or other tasks (via `inputs=`).
- For ordering-only dependencies (no data needed), use the `_` prefix convention:
  `inputs={"_dep": some_node}`. The `_` prefix tells barca to skip artifact
  deserialization — the parameter receives `None`.
- They must **not** be an input to an asset or sensor (a task always re-runs, so
  feeding its output into a cacheable node would keep that node perpetually
  stale).

Run a task with [`barca run`](../cli.md). By default `barca run` force-reruns
every upstream asset; `--burst a,b` re-runs only the named assets.

```python
from barca import asset, task

@asset()
def report() -> dict:
    return {"rows": 42}

# Asset -> task: a task consuming an upstream asset.
@task(inputs={"data": report})
def send_email(data: dict) -> None:
    print(f"Sending report: {data}")

# Ordering-only: migrate runs first, notify doesn't need its data.
@task()
def migrate() -> None:
    run_migration()

@task(inputs={"_migrate": migrate})
def notify(_migrate) -> None:
    send_slack("migration done")
```

::: barca.task
    options:
      show_source: false

## @unsafe

```python
@unsafe
def my_asset() -> str:
    return global_config["value"]
```

Marks a function as unsafe — it references globals, performs I/O, or otherwise cannot be tracked by AST analysis. `@unsafe` silences purity warnings; caching behaviour is unchanged. Barca makes no correctness guarantee for unsafe assets.

::: barca.unsafe
    options:
      show_source: false

## Schedule

```python
Schedule("0 5 * * *")   # cron expression
```

Constructs a schedule freshness value. Use inside `freshness=` on any decorator.

::: barca.Schedule

## partitions

::: barca.partitions

## Partitions

::: barca.Partitions
