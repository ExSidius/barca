# Backfill And Replay

This document specifies how Barca should handle rerunning assets against historical or explicitly selected inputs.

Backfill and replay are related but not identical:

- backfill means running many historical or partitioned targets
- replay means re-executing a node against a specific historical provenance choice

This workflow assumes the Barca core constraints documented in [../core-constraints.md](../core-constraints.md).

## Summary

Barca should support both:

- backfill over partition keys, time windows, or historical observations
- replay against explicit historical upstream selections

This matters because provenance is append-only and reusable, not just “latest only.”

## Why backfill matters

Even in an asset-first orchestrator without pipelines, operators still need:

- “run this asset for the last 30 daily partitions”
- “rerun downstream from these historical sensor observations”
- “recompute this subtree from a known past state”

Without backfill/replay, Barca would be good at current-state refresh but weak at historical workflows.

## Why replay matters

Replay is the manual counterpart to provenance-based caching.

If Barca can automatically reuse historical matching provenance, it should also let users intentionally target that provenance.

Examples:

- run a downstream asset against a specific upstream materialization
- compare current output to output from a previous sensor observation
- re-materialize a subtree from a historical partition source universe

## Backfill examples

### Partition backfill

```python
@asset(partitions={"day": partitions(["2026-03-01", "2026-03-02", "2026-03-03"])})
def daily_metric(day: str) -> dict[str, str]:
    ...
```

Barca should support:

- backfill one missing partition
- backfill a range of partitions
- backfill all stale partitions

### Sensor-observation backfill

If a sensor historically observed:

- observation `S1`
- observation `S2`
- observation `S3`

Barca should support replaying a downstream asset from `S2` specifically.

## Replay examples

### Replay against a historical upstream asset version

If asset `a` has materializations:

- `A1`
- `A2`
- `A3`

and downstream asset `b` depends on `a`, Barca should support a replay mode that says:

- materialize `b` against `A2`

That is not the default current-state refresh path, but it is a valuable operator/debugging workflow.

## Selection model

For the MVP, Barca should support a simple selector concept in internal APIs and CLI/UI even if the Python user-facing API stays small.

Useful selectors:

- specific materialization ID
- specific partition key
- specific sensor observation ID
- latest current
- latest historical matching selector

This is enough to make replay concrete without overbuilding a query language.

## How replay differs from normal refresh

Normal refresh:

- resolve inputs from the current selected provenance
- compute staleness against the current world

Replay:

- resolve inputs from an explicitly chosen historical provenance
- run against that selected historical state even if it is not the current one

That distinction should be visible in UI and execution records.

## UI implications

Barca should make it obvious whether a run is:

- normal current-state refresh
- backfill
- replay

Useful UI fields:

- run mode
- selected historical inputs
- partition range
- source observation/materialization IDs

## Recommended implementation stance

For the MVP:

- support historical selection internally and in operator-facing surfaces
- support partition-range backfill
- support replay against explicit upstream materialization or observation IDs
- keep the Python happy-path API small

This gives Barca real orchestration power without forcing a large extra DSL into the authoring surface.

## Acceptance criteria

- Barca can rerun a partitioned asset over a selected partition range.
- Barca can replay a downstream asset from a specific historical upstream materialization.
- Barca can replay a downstream asset from a specific historical sensor observation.
- Replay and backfill runs are distinguished from normal refresh runs in metadata and UI.
