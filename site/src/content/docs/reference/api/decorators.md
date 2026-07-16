---
title: Decorators API
description: Reference for @asset, @sink, @sensor, @task, @unsafe, Schedule, partitions, and parallel().
---

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
    retries: int = 1,
    retry_backoff: float = 0.0,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Declares a cacheable, provenance-tracked asset. The default freshness is `Always` — the asset is kept up to date automatically during `barca run`.

`retries` is the total number of attempts on failure (1 = no retry). `retry_backoff` is the base
delay in seconds between attempts (delay grows linearly: `retry_backoff * attempt`).

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

## Partitions

```python
partitions(values: list[str | int])          # static partition values
partitions_from(source: AssetLike)            # derive partitions from an upstream asset
collect(source: AssetLike)                    # fan-in: aggregate all partitions of an upstream asset
asset_ref(canonical_name: str)                # reference a node by canonical id, not Python import
```

Use `partitions=` on `@asset` to split an asset's work across a set of keys, executed as
independent steps:

```python
from barca import asset, partitions, partitions_from, collect

@asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
def price(ticker: str) -> dict:
    return fetch_price(ticker)

@asset(partitions={"ticker": partitions_from(price)})   # same partition keys as `price`
def signal(ticker: str, price: dict) -> dict:
    return compute_signal(price)

@asset(inputs={"prices": collect(price)})                # fan-in: all partitions as a list
def summary(prices: list[dict]) -> dict:
    return aggregate(prices)
```

`partitions(...)` accepts a literal list (extracted statically at parse time) or any other Python
expression — e.g. a list comprehension or function call — which is evaluated by the Python runtime
at plan time. `partitions_from(...)` derives an asset's partition keys from an upstream asset's own
partitions, rather than declaring them again. `collect(...)`, used inside `inputs=`, aggregates
every partition of an upstream asset into a single list delivered to the parameter.

`asset_ref("path/to/file.py:function_name")`, used inside `inputs=`, references a node by its
canonical id (source file path + function name, or its explicit `name=`) instead of importing the
Python function directly — useful for cross-file references:

```python
from barca import asset, asset_ref

@asset(inputs={"data": asset_ref("other_module/assets.py:raw_data")})
def process(data: dict) -> dict:
    return data
```

## @sink

```python
@sink(
    path: str,
    serializer: str | None = None,
)
```

Stacked on an `@asset` to write the asset's output to a path when it materialises. Paths are fsspec-compatible (local, `abfss://`, `s3://`, `gs://`, etc. — remote schemes need the matching extra, see [Remote storage](/reference/remote-storage/)). Multiple `@sink` decorators may be stacked on the same asset.

```python
from barca import asset, sink, Always

@asset(freshness=Always)
@sink('./output.json')
@sink('abfss://exports@myacct.dfs.core.windows.net/output.parquet', serializer='parquet')
def banana() -> dict:
    return {'a': 1}
```

The serialization format for each sink is chosen by precedence: the `serializer=` kwarg (`json`, `pickle`, `parquet`) → the sink path's extension (`.json`, `.pkl`, `.pickle`, `.parquet`) → the parent asset's artifact format. Writes are staged through a local temp file and uploaded/renamed atomically, so a crash never leaves a partial file at the destination.

Sinks are leaf nodes — no other asset may list a sink as an input. A sink failure does not fail the parent asset, but is surfaced prominently in logs (`[barca] SINK FAILED: ...`).

For partitioned assets, each partition writes its own sink file with the partition key injected before the extension: `@sink('out.parquet')` on partitions `ticker=AAPL, ticker=MSFT` produces `out_ticker_AAPL.parquet` and `out_ticker_MSFT.parquet`.

## @sensor

```python
@sensor(
    name: str | None = None,
    freshness: Manual | Schedule = Manual,
    timeout_seconds: int = 300,
    retries: int = 1,
    retry_backoff: float = 0.0,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Declares an external-state observer. Sensors must use `Manual` or `Schedule` freshness — `Always` is not valid for sensors (polling frequency must be declared explicitly). See `@asset` above for `retries` / `retry_backoff` semantics.

Sensors return `(update_detected: bool, output)` tuples. The full tuple is passed as input to downstream assets.

```python
from barca import sensor, Schedule

@sensor(freshness=Schedule("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    files = list(Path("inbox").glob("*.csv"))
    return len(files) > 0, [str(f) for f in files]
```

Sensors are source nodes only — they have no upstream inputs.

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

Run a task with [`barca run`](/reference/cli/). By default `barca run` force-reruns
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

## parallel

```python
parallel(*callables) -> list
parallel_map(fn, items, **kwargs) -> list
```

Fan out work from inside a `@task` body across worker processes. `parallel()` takes
`functools.partial`-wrapped calls to other `@task`-decorated functions and returns their results
(or `ParallelError` objects for failed branches) in argument order. `parallel_map(fn, items)` is
sugar for `parallel(*(partial(fn, item) for item in items))`.

```python
from functools import partial
from barca import task, parallel, parallel_map

@task()
def deploy_us(model) -> str: ...

@task()
def deploy_eu(model) -> str: ...

@task()
def deploy_all(model) -> None:
    results = parallel(partial(deploy_us, model), partial(deploy_eu, model))
    # or: results = parallel_map(deploy, ["us", "eu"])
```

When running inside a barca worker, `parallel()` dispatches each branch to a separate worker
process via the coordinator (the calling worker is frozen for the duration and resumed on
completion); called standalone (outside a worker), it runs the callables sequentially. Barca's
static analysis recognizes `partial(fn, ...)` arguments (including inside a starred generator or
list comprehension, e.g. `parallel(*(partial(deploy, r) for r in regions))`) to build the
dependency graph; fully dynamic call sets (e.g. `parallel(*work_items)`) are supported at runtime
but can't be resolved statically. `parallel()`/`parallel_map()` calls are only recognized inside
`@task` bodies, not `@asset` bodies.

A failed branch is returned as a `ParallelError` (with `.error` holding the message) rather than
raising — inspect each result to detect failures.

## @unsafe

```python
@unsafe
def my_asset() -> str:
    return global_config["value"]
```

Marks a function as unsafe — it references globals, performs I/O, or otherwise cannot be tracked by AST analysis. `@unsafe` silences purity warnings; caching behaviour is unchanged. Barca makes no correctness guarantee for unsafe assets.

## Schedule

```python
Schedule("0 5 * * *")   # cron expression
```

Constructs a schedule freshness value. Use inside `freshness=` on any decorator.
