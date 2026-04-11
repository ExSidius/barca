# Freshness-Driven Run And Effects

This document specifies how Barca decides when assets, sensors, and side effects are eligible to run.

The goal is to keep the orchestration model small:

- assets define the graph
- sensors bring external state into the graph
- `barca run` continuously maintains each asset at its declared freshness level
- no pipeline DSL is required

This workflow assumes the Barca core constraints documented in [../core-constraints.md](../core-constraints.md).

## Summary

Every asset, sensor, and effect declares a `freshness` value that controls when Barca keeps it up to date during `barca run`.

```python
@asset()                                    # freshness=Always (default)
@asset(freshness=Manual)
@asset(freshness=Schedule("0 5 * * *"))

@sensor(freshness=Manual)                   # Always is not valid for sensors
@sensor(freshness=Schedule("*/5 * * * *"))

@effect()                                   # freshness=Always (default)
@effect(freshness=Manual)
@effect(freshness=Schedule("0 6 * * *"))
```

Where:

- `freshness` controls how eagerly Barca keeps a stale node up to date
- staleness is still computed from provenance
- `barca run` discovers stale nodes and materialises them when eligible

## Freshness kinds

### `Always`

The default for `@asset` and `@effect`. Barca keeps this asset fresh automatically — any upstream change cascades through and re-materialises it during `barca run`.

**`Manual` freshness blocks downstream**: if any transitive upstream asset has `Manual` freshness, downstream `Always` assets cannot auto-update. They remain stale until the `Manual` upstream is explicitly refreshed.

### `Manual`

Barca never auto-updates this asset, even when stale. Only refreshed via explicit `barca assets refresh`. Useful for source data or pinned inputs that should not change without deliberate action.

### `Schedule("cron_expr")`

Barca refreshes this asset when a cron tick has elapsed since its last run. Acceptable staleness between ticks.

```python
from barca import asset, Schedule

@asset(freshness=Schedule("0 5 * * *"))
def prices() -> dict[str, str]:
    return {"ok": "yes"}

@asset(inputs={"prices": prices}, freshness=Schedule("0 6 * * *"))
def daily_report(prices: dict[str, str]) -> dict[str, str]:
    return {"report": prices["ok"]}
```

Behavior:

- at 5:00, `prices` becomes eligible
- after `prices` updates, `daily_report` becomes stale
- before 6:00, `daily_report` is `stale_waiting_for_schedule`
- at 6:00, if upstream is settled, `daily_report` becomes `runnable_stale`

## Decorator APIs

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

```python
@sensor(
    name: str | None = None,
    freshness: Manual | Schedule = Manual,
    timeout_seconds: int = 300,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

```python
@effect(
    name: str | None = None,
    inputs: dict[str, NodeRefLike] | None = None,
    freshness: Freshness = Always,
    timeout_seconds: int = 300,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

## Why `Always` is the default

`Always` is the right default because most assets in a pipeline should stay up to date automatically. Users opt into `Manual` or `Schedule` when they need to control when updates happen. `Manual` assets are passive by design.

## Timeout policy

Assets, sensors, and effects all support:

```python
timeout_seconds: int = 300
```

This timeout applies per attempt. If an attempt exceeds the timeout, Barca terminates the worker and marks the attempt as timed out.

## barca run model

`barca run` is a long-running process. On each pass (topo order):

1. Reindex current definitions
2. Compute provenance-based staleness
3. Compute freshness-based eligibility
4. Enqueue runnable stale nodes
5. Dispatch workers
6. Record results and update descendant states

`Always` assets: materialise if stale and all upstreams fresh.
`Schedule` assets: materialise if a cron tick has elapsed since last run.
`Manual` assets: never auto-materialise.
Sensors: observe on each `Schedule` tick (or `Manual` trigger).
Effects/Sinks: run when upstream freshens.

If a pass is already running when the next tick arrives, the tick is skipped — passes do not overlap.

## State model

Recommended internal states:

- `fresh`
- `stale_waiting_for_schedule`
- `stale_waiting_for_upstream`
- `runnable_stale`
- `running`
- `failed`
- `historical`

## Sensors

Sensors are first-class ingress nodes for uncontrolled external state.

```python
from barca import sensor, asset, Schedule

@sensor(freshness=Schedule("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    return True, ["a.csv", "b.csv"]

@asset(inputs={"paths": inbox_files})
def ingest_paths(paths: tuple[bool, list[str]]) -> list[dict[str, str]]:
    ...
```

The full `(update_detected, output)` tuple is passed as input to downstream assets — downstream receives the complete tuple, not just the output payload.

Sensors return `(update_detected: bool, output)`. When `update_detected=True`, downstream assets become stale. When `update_detected=False`, downstream assets do not become stale (no meaningful change was detected).

## Effects

Effects are standalone side-effect functions — sending email, writing to a database, calling an external API. Use `@effect` for arbitrary side-effects. Use `@sink` for writing asset outputs to file paths.

```python
from barca import asset, effect, sensor, Schedule

@sensor(freshness=Schedule("0 5 * * *"))
def upstream_db_rows() -> tuple[bool, list[dict[str, str]]]:
    ...

@asset(inputs={"rows": upstream_db_rows})
def report_rows(rows: tuple[bool, list[dict[str, str]]]) -> list[dict[str, str]]:
    _, data = rows
    return data

@effect(inputs={"rows": report_rows}, freshness=Schedule("0 6 * * *"))
def send_report(rows: list[dict[str, str]]) -> None:
    ...
```

Effects are leaf nodes — no other asset may list an effect as an input.

## Sinks

`@sink` is a decorator stacked on `@asset` for writing outputs to file paths. Paths are fsspec-compatible (local, `s3://`, `gs://`, etc.).

```python
from barca import asset, sink, Always, JsonSerializer

@asset(freshness=Always)
@sink('./report.json', serializer=JsonSerializer)
@sink('s3://my-bucket/report.json', serializer=JsonSerializer)
def report() -> dict:
    return {"rows": 42}
```

When the parent asset materialises, all attached sinks write its output to their declared paths. Sink failures are non-blocking (leaf nodes) but surface prominently as failures in `assets list` and job logs.

## Partitioned assets and freshness

Freshness applies to partitioned assets per partition:

- staleness is determined per partition
- eligibility is determined per partition
- `barca run` may enqueue many runnable partitions at once

```python
@asset(
    partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])},
    freshness=Schedule("0 5 * * *"),
)
def fetch_prices(ticker: str) -> dict[str, str]:
    return {"ticker": ticker}
```

At 5:00, each stale partition becomes independently eligible.

## Acceptance criteria

- An asset with `freshness=Manual` is only refreshed when explicitly requested or when required by a targeted downstream refresh.
- An asset with `freshness=Always` becomes runnable immediately when stale and upstream-ready.
- A sensor with `freshness=Schedule(...)` records observations on its cron cadence.
- An asset with `freshness=Schedule(...)` becomes runnable only when its cron window opens.
- A downstream asset can be stale at 5:10 and still not runnable until its 6:00 schedule.
- Assets and effects may consume sensors as inputs (receiving the full tuple).
- Effects use the same `freshness` primitive as assets.
- Effects record successful executions against current upstream provenance.
- Partitioned assets evaluate staleness and eligibility per partition.
- `Manual` freshness blocks downstream `Always` assets from auto-updating.
- `Always` is not valid for sensors.
