---
title: "Workflow: Asset Continuity Across Rename and Move"
description: How Barca preserves history when asset code changes location or name.
---

This document specifies how Barca should preserve history when asset code changes location or name.

This is where the difference between:

- logical asset continuity
- definition history
- materialization history

becomes visible to users.

This workflow assumes the Barca core constraints documented in [Core Constraints](/core-constraints/).

## Summary

Barca uses this continuity policy:

- if `@asset(name="...")` is provided, that explicit name is the continuity key
- otherwise, continuity is repo-relative file path + function name (`DagNode::continuity_key()`)
- materializations are keyed by continuity key + `run_hash` in an append-only table and are never deleted by normal operation
- **there is no rename/move detection.** Continuity is decided purely by whether the continuity key is unchanged. Without an explicit `name=`, changing the file path or function name produces a different continuity key, which Barca treats as an unrelated new node — there is no AST-similarity heuristic that reconnects it to the old one

This means:

- explicit names preserve identity across renames and moves
- implicit path/function identity does not — not even when the function body is untouched

That tradeoff is intentional: it keeps identity a pure function of declared metadata, with no heuristic matching that could silently merge two different functions or split history on a trivial refactor.

## No reindex diff today

Barca does not currently print an added/removed/renamed diff on `barca get`/`barca run`/`barca list`. Each invocation just parses the current source files into a DAG; there is no comparison against a previous index. `barca list` shows the live set of definitions only.

The subsections below describe what continuity looks like across a rename in the two supported cases — with and without an explicit `name=` — not a diffing feature.

## Case 1: rename or move without explicit asset name

Start with:

```python
from barca import asset


@asset()
def prices() -> str:
    return "v1"
```

in:

```text
my_project/assets.py
```

The continuity key is:

```text
my_project/assets.py:prices
```

Later, the user moves and renames it:

```python
from barca import asset


@asset()
def fetch_prices() -> str:
    return "v2"
```

in:

```text
my_project/pricing.py
```

The new continuity key is:

```text
my_project/pricing.py:fetch_prices
```

Regardless of whether the function body changed, Barca treats this as two unrelated lineages, because the continuity key changed:

- `my_project/assets.py:prices` — no longer produced by any source file; its old materializations remain in the local metadata DB (keyed by that continuity key) but are no longer reachable through `barca get`/`barca stats` once the file no longer defines it
- `my_project/pricing.py:fetch_prices` — a brand-new node with no prior materializations, so its first run cannot be a cache hit

This is true even when the function body is byte-for-byte identical — Barca does not compare function bodies across nodes to infer a rename. Only an explicit `name=` carries identity across a move.

## Why continuity requires an explicit name

Matching renames by function-body similarity would mean two independently-authored functions with the same body get silently treated as "the same asset," and a one-line edit made during a move would silently split history instead. Requiring `name=` for continuity makes identity an explicit, auditable decision rather than a heuristic guess.

## Case 2: rename or move with explicit asset name

Start with:

```python
from barca import asset


@asset(name="prices")
def prices() -> str:
    return "v1"
```

in:

```text
my_project/assets.py
```

Later, the user moves and renames it:

```python
from barca import asset


@asset(name="prices")
def fetch_prices() -> str:
    return "v2"
```

in:

```text
my_project/pricing.py
```

The continuity key remains:

```text
prices
```

For the MVP, Barca should treat this as:

- one logical asset lineage
- two definition snapshots
- old materializations retained as historical
- new materializations attached to the same logical asset identity

This is the recommended way for users to preserve asset history across refactors.

## What gets stored

For the explicit-name case, Turso should have:

### `assets`

- one `asset_id`
- `logical_name = "prices"`
- latest live module path / function location metadata

### `asset_definitions`

- one definition row for the old code location
- one definition row for the new code location
- both linked to the same `asset_id`

### `materializations`

- old materializations linked to the old definition
- new materializations linked to the new definition
- all preserved

## UI and TUI behavior

The UI should make this legible.

For a single logical asset, users should be able to see:

- current live definition
- prior definitions
- prior materializations
- stale or superseded runs
- source location changes over time

For assets without explicit continuity names, the UI should show them as distinct logical assets even if Barca has advisory similarity hints.

## Duplicate handling

If two currently indexed assets resolve to the same continuity key, Barca should fail indexing.

### Example: duplicate explicit name

```python
@asset(name="prices")
def prices_one() -> str:
    return "one"


@asset(name="prices")
def prices_two() -> str:
    return "two"
```

This should fail with a duplicate continuity key error.

### Example: duplicate implicit key

If Barca somehow discovers two live assets that both resolve to the same repo-relative file path + function name, that should also fail.

The MVP should prefer loud failure over ambiguous identity.

## Implementation behavior during indexing

When indexing an asset:

1. Compute the continuity key:
   - explicit `name` if present
   - otherwise repo-relative file path + function name
2. Look up an existing live asset with that continuity key.
3. If none exists:
   - create a new `asset_id`
4. If one exists:
   - attach the new definition snapshot to that `asset_id`
5. If multiple live assets exist with the same continuity key:
   - fail indexing
6. Insert a new `asset_definitions` row for the current definition snapshot.
7. Mark prior definitions for that asset as historical or superseded in UI-facing queries, but do not delete them.

## Interaction with preflight consistency

Preflight consistency still operates on `definition_hash`, not only on continuity key.

That means:

- continuity answers "is this the same logical asset lineage?"
- `definition_hash` answers "is this the exact code/config snapshot we planned to run?"

Both are required.

## Recommended guidance to users

If users want continuity across refactors, Barca should recommend:

```python
@asset(name="stable_asset_name")
```

This should be documented as the preferred way to preserve history when:

- moving files
- renaming functions
- reorganizing modules

Without an explicit name, Barca will preserve old history, but it will treat the moved/renamed asset as a new lineage.

## Acceptance criteria

- Moving or renaming an unnamed asset — even with an identical function body — creates a new logical asset lineage; Barca has no rename-detection heuristic, so this is indistinguishable from deleting the old asset and adding a new one.
- Moving or renaming a named asset (same `name=` at new location) preserves one logical asset lineage with multiple definition snapshots.
- Old materializations remain in the local metadata DB in both cases (never deleted by normal operation), though only the named-continuity case keeps them reachable under the current asset's identity.
- Duplicate continuity keys fail indexing.
- UI/TUI can show definition history for a single logical asset when explicit `name` continuity is used.
- There is no added/removed/renamed diff output today — this is a possible future feature, not current behavior.
