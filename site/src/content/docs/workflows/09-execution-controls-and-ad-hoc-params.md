---
title: "Workflow: Execution Controls and Ad Hoc Params"
description: Retries, timeouts, cancellation, and ad hoc runtime parameters.
---

This document specifies retries, timeouts, cancellation, and ad hoc runtime parameters.

This workflow assumes the Barca core constraints documented in [Core Constraints](/core-constraints/).

## Summary

For the MVP:

- assets, sensors, and tasks all support `timeout_seconds`, defaulting to `300`
- failed attempts retry per-node `retries`/`retry_backoff`, defaulting to `retries=1` (no retry)
- running work must be visible in real time in UI and TUI
- users can cancel running work in real time
- cancelled work is marked `cancelled` and treated as incomplete
- ad hoc runtime params are a planned concept — the cache-identity hash already reserves a slot for them, but no CLI or API surface sets them yet

## Timeout

Recommended decorator shape additions:

```python
@asset(..., timeout_seconds=300)
@sensor(..., timeout_seconds=300)
@task(..., timeout_seconds=300)
```

Timeouts are expressed in seconds.

The timeout applies per attempt.

If a node exceeds its timeout:

- the worker is terminated
- the attempt is marked timed out
- Barca applies the standard retry policy

## Retry policy

Retry policy is per-node and configurable, declared as decorator kwargs:

```python
@asset(retries=3, retry_backoff=1.0)
@sensor(retries=3, retry_backoff=1.0)
@task(retries=3, retry_backoff=1.0)
```

- `retries` — total attempts on failure (`1` = no retry, the default).
- `retry_backoff` — base delay in seconds. Delay before attempt N is `retry_backoff * N` (linear, not exponential).

Rust owns the retry loop; each retry is a fresh worker invocation. Retries do not cascade — a parent's retry does not re-run its children, and a child's retries do not trigger its parent.

### Why per-node configuration

For pure assets, repeated failure usually means:

- bad input
- bad logic

A fixed global policy would either retry cheap flaky steps too little or retry expensive deterministic failures too much. Letting each node declare its own `retries`/`retry_backoff` (with a conservative `retries=1` default) keeps the common case simple while leaving room for flaky I/O-bound steps to opt into retries explicitly.

If all retries fail, the node remains failed/stale until the user fixes the issue and reruns it.

## Cancellation

Users should be able to see running work and cancel it in real time.

That means UI and TUI should show:

- currently running nodes
- start time
- attempt number
- timeout deadline

When a user cancels a running node:

- Barca should send termination to the worker
- mark the run record as `cancelled`
- treat the execution as incomplete or partial
- avoid publishing partial outputs as current outputs

Cancellation is an operator action, not a successful completion.

## State model additions

The execution state model should include:

- `running`
- `failed`
- `timed_out`
- `cancelled`
- `fresh`
- `stale_waiting_for_schedule`
- `stale_waiting_for_upstream`
- `runnable_stale`
- `historical`

For the MVP, `cancelled` should mean:

- this run did not complete successfully
- any temp outputs should be discarded or left unpublished
- the node may still be stale and runnable later

## Why partial state should not be published

If a run times out or is cancelled, its outputs are not trustworthy as current materializations.

So Barca should:

- allow logs and attempt metadata to remain visible
- keep temp or debug artifacts if useful
- never publish partial outputs as the selected current result

## Ad hoc runtime parameters

**Not yet implemented.** `barca get`/`barca run` take no `--param` flag today, and nothing in the CLI or `barca.api` sets per-run parameters. `run_hash` already reserves a slot for them (`hash::run_hash(definition_hash, partition_key, upstream_ids, params)`), but every call site passes `None` — the plumbing exists at the hashing layer only. The design below is the intended shape once it ships.

Barca should support explicit runtime parameters that are not durable partition keys.

This is a separate concept from partitions.

Examples:

```python
@asset()
def square(x: int) -> int:
    return x**2
```

Users should be able to run this with ad hoc params like:

```bash
# Proposed — not implemented today.
barca get square pipeline.py --param x=7
barca get square pipeline.py --param x=14
```

## Why ad hoc params matter

Not every repeated input belongs in the partition universe.

Some inputs are just runtime arguments.

Barca should still cache them.

That means cache identity must include:

- definition hash
- resolved upstream materialization IDs
- explicit runtime params

## Example: cyclical values and cache reuse

Consider:

```python
@asset()
def day_number(day_name: str) -> int:
    mapping = {
        "monday": 1,
        "tuesday": 2,
        "wednesday": 3,
        "thursday": 4,
        "friday": 5,
        "saturday": 6,
        "sunday": 7,
    }
    return mapping[day_name]
```

If Barca has already materialized:

- `day_number(day_name="monday")`

and later the same function definition and same input recur, Barca should reuse the cached result immediately.

This is true even if the call occurs much later in wall-clock time.

The cache key is provenance plus params, not recency.

## Params vs partitions

Use partitions when the input space is:

- durable
- enumerable
- operator-visible as a managed set

Use ad hoc params when the input is:

- one-off or user-provided at call time
- not part of the durable partition universe

Both should participate in cache identity, but only partitions define managed sub-assets in the graph.

## Recommended API stance

For the MVP:

- allow ad hoc params in CLI and execution helpers
- include params in `run_hash`
- do not force every repeated input into partitions

Useful shapes:

```bash
# Proposed — not implemented today.
barca get square pipeline.py --param x=7
```

## Failure model for pure assets

For pure assets, failure should be treated straightforwardly.

If an asset fails repeatedly, the expectation is:

- the logic is wrong
- the input is wrong
- the user fixes it and reruns

Barca does not need to add more complicated automatic healing behavior in the MVP.

## Acceptance criteria

- Assets, sensors, and tasks all support `timeout_seconds=300` by default.
- Failed attempts retry per the node's `retries`/`retry_backoff` (default `retries=1`, i.e. no retry).
- Running nodes are visible in UI and TUI.
- Users can cancel running nodes in real time.
- Cancelled runs are marked `cancelled` and remain incomplete.
- Ad hoc params are supported in CLI invocations. (Not yet implemented.)
- Ad hoc params are included in cache identity. (Reserved in `run_hash`, but unset — not yet wired to any input.)
- Repeated calls with the same code, upstream provenance, and params reuse cache immediately.
