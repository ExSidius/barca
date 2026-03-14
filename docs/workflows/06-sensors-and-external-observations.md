# Sensors And External Observations

This document specifies how Barca should model uncontrolled external dependencies.

The core idea is to add sensors as first-class graph nodes:

- sensors pull external state into the graph
- assets transform graph values
- effects push graph values back out to external systems

This workflow assumes the Barca core constraints documented in [../core-constraints.md](../core-constraints.md).

## Summary

For the MVP, Barca should support a `@sensor` decorator.

Recommended shape:

```python
@sensor(
    name: str | None = None,
    schedule: ScheduleLike = "manual",
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Sensors should:

- be autodiscovered like assets and effects
- be renderable in the same DAG UI
- produce typed outputs that assets and effects can consume
- explicitly report whether they observed a meaningful update
- use the same `schedule` primitive as assets and effects
- record versioned observation history

## Example

```python
from barca import sensor, asset, effect, cron


@sensor(schedule=cron("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    return True, ["inbox/a.csv", "inbox/b.csv"]


@asset(inputs={"paths": inbox_files}, schedule="always")
def parse_inbox(paths: list[str]) -> list[dict[str, str]]:
    ...


@effect(inputs={"rows": parse_inbox}, schedule=cron("0 * * * *"))
def publish_rows(rows: list[dict[str, str]]) -> None:
    ...
```

This should render as one graph:

```text
inbox_files -> parse_inbox -> publish_rows
```

## Why sensors are useful

Without sensors, uncontrolled external state ends up being hidden inside assets.

That is a bad fit because it hides the real graph boundary.

Making sensors explicit gives Barca:

- honest graph structure
- visible external ingress points in the UI
- explicit scheduling for polling/checking external sources
- better stale-state reasoning

It also makes sensor-triggered updates explicit rather than burying polling logic inside assets.

## What sensors return

For the MVP, sensors should conceptually return:

- `updated_detected: bool`
- `output: SupportedValue`

The simplest user-facing syntax is a 2-tuple:

```python
@sensor(schedule=cron("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    return True, ["inbox/a.csv", "inbox/b.csv"]
```

Barca should interpret that as:

```python
updated_detected, output = inbox_files()
```

Where `output` may use the same supported value types as assets where practical:

- JSON-serializable values
- pandas dataframes
- polars dataframes
- explicit pickle values

This keeps downstream consumption simple while still letting the sensor say “I checked, but nothing meaningfully changed.”

### Recommended internal model

Even if the user-facing API starts as a tuple, Barca should normalize it to an internal record like:

```python
SensorResult(
    updated_detected: bool,
    output: SupportedValue,
)
```

That gives the system room to grow later without breaking the API shape.

## Sensor history

Sensors should be append-only like assets and effects.

That means Barca should keep:

- prior sensor definitions
- prior sensor observation records
- stale/historical status over time

This matters because external state can change independently of code.

## Sensor identity and execution records

Sensors need:

- a continuity key, same policy as assets
- a definition hash
- observation records keyed by sensor definition and observation payload metadata

Unlike pure assets, a sensor observation is not treated as deterministically reproducible from code.

So Barca should record sensor observations, not pretend they are derivable caches.

Observation records should include at least:

- sensor definition hash
- `updated_detected`
- output metadata or artifact reference
- observation timestamp
- status

## Freshness and staleness

Sensors should share the same broad stale-state machinery as assets and effects:

- `fresh`
- `stale_waiting_for_schedule`
- `runnable_stale`
- `running`
- `failed`
- `historical`

For a sensor, “fresh” means:

- there exists a successful observation record that satisfies the current schedule/reconciliation policy

When a sensor returns `updated_detected=True`, its downstream descendants become stale using the same graph propagation rules as assets.

When a sensor returns `updated_detected=False`:

- the sensor run is still recorded
- downstream descendants do not become stale solely because of the poll

## Inputs

For the MVP:

- assets may depend on assets or sensors
- effects may depend on assets or sensors
- sensors should not depend on other nodes in the first version
- effects should not be inputs to anything in the first version

That keeps the model directional and easier to reason about.

## Why sensors are like inverse effects

This is the right intuition.

- effects send graph state out into the world
- sensors bring world state into the graph

They are opposites operationally, but they can share:

- schedule semantics
- autodiscovery
- graph rendering
- history tracking

They should not share pure-asset cache semantics.

## UI implications

Sensors should be visually distinct in the UI and TUI.

The UI should make it obvious which nodes are:

- sensors
- assets
- effects

That matters because operators need to know where uncontrolled external state enters the graph.

Useful sensor UI details:

- last observed time
- last update-detected flag
- last successful payload hash or checksum
- current schedule
- downstream nodes affected by the latest observation

## Recommended implementation stance

For the MVP:

- add `@sensor`
- allow sensors as inputs to assets and effects
- reuse the same `schedule` primitive
- require sensors to return `updated_detected` plus payload
- record append-only observation history
- keep sensors as source nodes only
- render sensors distinctly in the DAG UI

This gives Barca an explicit and honest model for external dependencies without overcomplicating the core graph semantics.

## Acceptance criteria

- A sensor can be autodiscovered from decorator metadata.
- A sensor can feed an asset.
- A sensor can feed an effect.
- A sensor can return `(False, payload)` to indicate a successful poll with no meaningful update.
- Sensor observations are recorded historically rather than overwritten.
- A sensor observation with `updated_detected=True` can mark downstream assets stale.
- Sensors are rendered as first-class nodes in the UI/TUI graph.
