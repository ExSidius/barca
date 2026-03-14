# Schedule-Driven Reconciliation And Effects

This document specifies how Barca should decide when assets, sensors, and side effects are eligible to run.

The goal is to keep the orchestration model small:

- assets define the graph
- sensors bring external state into the graph
- one reconciler loop computes staleness and eligibility
- no pipeline DSL is required for the MVP

This workflow assumes the Barca core constraints documented in [../core-constraints.md](../core-constraints.md).

## Summary

For the MVP, Barca should attach a `schedule` policy directly to assets, sensors, and effects.

Recommended shape:

```python
@asset(schedule="manual")
@asset(schedule="always")
@asset(schedule=cron("0 5 * * *"))

@sensor(schedule="manual")
@sensor(schedule="always")
@sensor(schedule=cron("*/5 * * * *"))

@effect(schedule="manual")
@effect(schedule="always")
@effect(schedule=cron("0 6 * * *"))
```

Where:

- `schedule` controls when a stale node is eligible to run
- staleness is still computed from provenance
- the reconciler loop is responsible for discovering stale nodes and enqueueing runnable ones

This deliberately replaces a more complex pipeline/job architecture for the MVP.

## Why `schedule`, not `freshness`

The important distinction is:

- `staleness`: whether the current state is behind the desired provenance
- `schedule`: when Barca is allowed to do something about it

Those are not the same thing.

Example:

- parent assets are scheduled for `0 5 * * *`
- a downstream asset is scheduled for `0 6 * * *`

At 5:15, the downstream asset may already be stale in principle, but it is not yet runnable because its schedule does not allow it until 6:00.

That means the runtime model should be:

- provenance-based staleness
- schedule-based eligibility

## Recommended decorator APIs

For the MVP:

```python
@asset(
    name: str | None = None,
    inputs: dict[str, AssetRefLike] | None = None,
    partitions: dict[str, PartitionSpecLike] | None = None,
    serializer: SerializerKind | None = None,
    schedule: ScheduleLike = "manual",
    timeout_seconds: int = 300,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

```python
@sensor(
    name: str | None = None,
    schedule: ScheduleLike = "manual",
    timeout_seconds: int = 300,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

```python
@effect(
    name: str | None = None,
    inputs: dict[str, NodeRefLike] | None = None,
    schedule: ScheduleLike = "manual",
    timeout_seconds: int = 300,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Where:

```python
ScheduleLike = Literal["manual", "always"] | CronSchedule
NodeRefLike = AssetRefLike | SensorRefLike
```

Helper:

```python
from barca import cron

cron("0 5 * * *")
```

## Why `manual` should be the default

`always` is useful, but it should not be the default.

If `always` were the default, every asset would become an eager reconciler target, which is noisy and expensive.

`manual` is the safer default because:

- plain asset definitions remain passive unless selected
- plain sensor definitions remain passive unless selected
- downstream refresh still works when a user explicitly targets an asset
- users opt into autonomous background behavior intentionally

## Timeout policy

For the MVP, assets, sensors, and effects should all support:

```python
timeout_seconds: int = 300
```

This timeout should apply per attempt.

If an attempt exceeds the timeout:

- Barca should terminate the worker
- mark that attempt as timed out
- apply the standard retry policy

## Reconciler model

Barca should have one manager loop that:

1. indexes current definitions
2. computes provenance-based staleness
3. computes schedule-based eligibility
4. enqueues runnable stale nodes
5. dispatches workers
6. records results and updates descendant states

This loop replaces the need for a separate pipeline engine in the MVP.

## State model

The engine should keep the internal state model deterministic.

Recommended states:

- `fresh`
- `stale_waiting_for_schedule`
- `stale_waiting_for_upstream`
- `runnable_stale`
- `running`
- `failed`
- `historical`

For UI purposes, deeper descendants of a changed node may be rendered as “likely affected” or similar, but the engine should still use deterministic states internally.

## Example: staggered schedules

```python
from barca import asset, cron


@asset(schedule=cron("0 5 * * *"))
def prices() -> dict[str, str]:
    return {"ok": "yes"}


@asset(inputs={"prices": prices}, schedule=cron("0 6 * * *"))
def daily_report(prices: dict[str, str]) -> dict[str, str]:
    return {"report": prices["ok"]}
```

Behavior:

- at 5:00, `prices` becomes eligible
- after `prices` updates, `daily_report` becomes stale
- before 6:00, `daily_report` is `stale_waiting_for_schedule`
- at 6:00, if upstream is settled, `daily_report` becomes `runnable_stale`

This is the behavior you want from “update parents at 5AM, child at 6AM.”

## Why this removes most need for pipelines

In this model:

- the asset graph already exists
- “refresh X” means “materialize X and what it needs upstream”
- periodic execution comes from per-node schedules

That means the main historical use of pipelines, namely “define the graph and when it runs,” mostly disappears.

For the MVP, that is good.

Barca can avoid:

- a second graph-definition DSL
- pipeline-specific dependency semantics
- duplicated concepts between asset graph and pipeline graph

## Why this makes the UI cleaner

Because Barca discovers the graph directly from decorator semantics:

- the graph the user writes is the graph Barca runs
- the graph the reconciler reasons about is the graph the UI renders
- there is no second pipeline definition layer to reconcile against

That has a few concrete advantages:

- DAG views can be generated directly from indexed asset and effect metadata
- node state can be rendered from the same stale/eligibility model the reconciler uses
- provenance edges can be shown without translation through a separate pipeline abstraction
- partition fan-out can be visualized as expansions of one logical asset, not as unrelated jobs

This is one of the strongest arguments for keeping the orchestration model asset-first.

## What still may be needed later

Even if pipelines are unnecessary, a thin job or run-selection layer may still be useful later for:

- selecting groups of roots
- backfills over partition ranges
- operator-facing run grouping
- temporary ad hoc execution

But that should be an operational convenience layer, not the core graph model.

## Sensors

Sensors should be first-class ingress nodes for uncontrolled external state.

Recommended mental model:

- `@sensor`: observes external state and emits an update signal plus a value into the graph
- `@asset`: transforms deterministic inputs into deterministic outputs
- `@effect`: pushes data from the graph into an external system

Example:

```python
from barca import sensor, asset, cron


@sensor(schedule=cron("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    return True, ["a.csv", "b.csv"]


@asset(inputs={"paths": inbox_files}, schedule="always")
def ingest_paths(paths: list[str]) -> list[dict[str, str]]:
    ...
```

Sensors and assets should both be allowed as inputs to assets and effects.

For the MVP, effects do not become inputs to anything.

## Why sensors are not just assets

Sensors are not deterministic cacheable transforms.

They depend on uncontrolled external state, so Barca cannot infer correctness from code and upstream provenance alone.

That means sensors should:

- share the same `schedule` primitive
- participate in the same graph rendering
- produce versioned observation records
- explicitly indicate whether they detected a meaningful update
- not be treated as pure cacheable assets

## Sensor freshness

For a sensor, “fresh” should mean:

- there exists a successful sensor observation record that satisfies the current schedule/reconciliation policy

For the MVP, that is enough.

Barca does not need to verify the external world continuously outside the configured schedule.

## Sensor result contract

For the MVP, sensors should report both:

- whether an update was detected
- the current observed payload

The simplest user-facing form is:

```python
@sensor(schedule=cron("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    return True, ["a.csv", "b.csv"]
```

Semantically, this means:

```python
updated_detected, output = inbox_files()
```

Where:

- `updated_detected=True` means downstream nodes should treat this observation as a change candidate
- `updated_detected=False` means the sensor checked successfully but did not observe a meaningful update

### Why this is useful

This gives Barca something pure assets do not need:

- a clean distinction between “sensor ran” and “sensor observed a change”

That is important for uncontrolled external dependencies, because polling an external source successfully is not the same as finding something new.

### Recommended internal model

A raw positional tuple is acceptable as MVP syntax, but Barca should normalize it immediately into an internal structured record such as:

```python
SensorResult(
    updated_detected: bool,
    output: SupportedValue,
)
```

That keeps the persisted and internal model extensible even if the user-facing syntax starts as a tuple.

### What happens when `updated_detected=False`

When a sensor run returns `updated_detected=False`:

- Barca should still record a successful sensor check
- Barca should not mark downstream nodes stale solely because the sensor ran
- the returned payload can still be stored with the observation record if useful for audit/debugging

The important point is that downstream invalidation should be driven by detected change, not by polling cadence alone.

## Effects

Effects should be first-class, but separate from pure assets.

Recommended mental model:

- `@asset`: pure, cacheable, provenance-addressed
- `@sensor`: external observation node
- `@effect`: side-effecting leaf that consumes asset outputs

Example:

```python
from barca import asset, effect, sensor, cron


@sensor(schedule=cron("0 5 * * *"))
def upstream_db_rows() -> list[dict[str, str]]:
    ...


@asset(inputs={"rows": upstream_db_rows}, schedule="always")
def report_rows() -> list[dict[str, str]]:
    return rows


@effect(inputs={"rows": report_rows}, schedule=cron("0 6 * * *"))
def write_rows_to_db(rows: list[dict[str, str]]) -> None:
    ...
```

## Why effects can share the same `schedule` primitive

This is a good simplification for the MVP.

There is no strong reason to invent a separate scheduling model for effects right away.

Assets, sensors, and effects all need:

- manual runs
- immediate eligibility once stale
- cron eligibility

So Barca should use the same `schedule` primitive for all three.

## Important difference between assets and effects

They should share scheduling semantics, but not caching semantics.

Assets:

- are deterministically cacheable by provenance
- can become fresh again by matching an old `run_hash`

Effects:

- are not assumed to be deterministically cacheable
- represent external state changes
- need execution records, not artifact reuse in the same sense

For the MVP, Barca should still record effect provenance:

- effect definition hash
- upstream materialization IDs
- target config
- execution status
- execution timestamp

But Barca should not pretend an effect is equivalent to a reusable pure asset materialization.

Sensors:

- produce observation records
- can be consumed downstream like assets
- are not assumed to be reproducible from code provenance alone

## Effect freshness

To keep the model simple, effects should use the same stale/eligible states as assets.

The difference is in what “fresh” means.

For an effect, “fresh” should mean:

- there exists a successful effect execution record for the current effect definition and current upstream provenance

That is enough for the MVP.

Later, Barca can add richer semantics such as:

- explicit idempotency keys
- forced re-application
- operator approval
- external state verification

## Scheduling and upstream changes

When an upstream asset changes:

- immediate children should be marked stale
- if their own schedule permits and upstream is settled, they become `runnable_stale`
- otherwise they become `stale_waiting_for_schedule` or `stale_waiting_for_upstream`

The same logic applies whether the child is an asset, sensor-dependent asset, or an effect.

## Partitioned assets and schedules

Schedules should apply to partitioned assets too.

That means:

- staleness is determined per partition
- eligibility is determined per partition
- the reconciler may enqueue many runnable partitions at once

Example:

```python
@asset(
    partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])},
    schedule=cron("0 5 * * *"),
)
def fetch_prices(ticker: str) -> dict[str, str]:
    return {"ticker": ticker}
```

At 5:00, each stale partition can become independently runnable.

## Recommended implementation stance

For the MVP:

- add `schedule=` to both `@asset` and `@effect`
- add `schedule=` to `@sensor`
- support `manual`, `always`, and `cron(...)`
- keep one reconciler loop
- compute staleness from provenance
- compute eligibility from schedule plus upstream state
- use deterministic stale states internally
- avoid a pipeline DSL

This is the smallest architecture that still supports autonomous refresh behavior.

## Acceptance criteria

- An asset with `schedule="manual"` is only refreshed when explicitly requested or when required by a targeted downstream refresh.
- An asset with `schedule="always"` becomes runnable immediately when stale and upstream-ready.
- A sensor with `schedule=cron(...)` records observations on its cron cadence.
- An asset with `schedule=cron(...)` becomes runnable only when its cron window opens.
- A downstream asset can be stale at 5:10 and still not runnable until its 6:00 schedule.
- Assets and effects may consume sensors as inputs.
- Effects use the same `schedule` primitive as assets.
- Effects record successful executions against current upstream provenance.
- Partitioned assets evaluate staleness and eligibility per partition.
