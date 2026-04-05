# Asset Continuity Across Rename And Move

This document specifies how Barca should preserve history when asset code changes location or name.

This is where the difference between:

- logical asset continuity
- definition history
- materialization history

becomes visible to users.

This workflow assumes the Barca core constraints documented in [../core-constraints.md](../core-constraints.md).

## Summary

For the MVP, Barca should use this continuity policy:

- if `@asset(name="...")` is provided, that explicit name is the continuity key
- otherwise, continuity is repo-relative file path + function name
- old definitions and materializations are never deleted
- rename/move detection from source similarity is advisory only, not automatic identity

This means:

- explicit names preserve identity across renames and moves
- implicit path/function identity does not

That tradeoff is intentional.

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

For the MVP, Barca should treat this as:

- one historical asset lineage for `my_project/assets.py:prices`
- one new live asset lineage for `my_project/pricing.py:fetch_prices`

Barca may optionally show an advisory hint like:

- “this new asset definition appears similar to a previously indexed asset”

But it should not automatically merge those histories.

## Why not merge automatically here

It is too easy to get this wrong.

Examples:

- a user copies a function into a new file as the starting point for a new asset
- two assets intentionally share nearly identical code
- a moved function also changes purpose or semantics

Silent merging would corrupt lineage more than it helps.

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

- continuity answers “is this the same logical asset lineage?”
- `definition_hash` answers “is this the exact code/config snapshot we planned to run?”

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

- Moving or renaming an unnamed asset creates a new logical asset lineage while preserving the old one as historical.
- Moving or renaming a named asset preserves one logical asset lineage with multiple definition snapshots.
- Old materializations remain queryable in both cases.
- Duplicate continuity keys fail indexing.
- UI/TUI can show definition history for a single logical asset when explicit `name` continuity is used.
