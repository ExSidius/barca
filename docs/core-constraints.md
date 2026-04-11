# Core Constraints

These are deliberate MVP constraints for Barca.

They are not incidental implementation details. They define the shape of the product.

## Directed acyclic graphs only

Barca only supports DAGs.

Cycles are explicitly disallowed.

That means:

- no self-dependencies
- no mutual recursion across assets
- no longer dependency loops anywhere in the asset graph

If a cycle is detected during indexing or job planning, Barca should fail immediately with a clear graph error.

### Why this is the right constraint

- it keeps scheduling and cache reuse understandable
- it keeps staleness propagation simple
- it avoids inventing vague semantics for cyclic materialization
- it matches the dominant orchestrator model for asset graphs

For the MVP, Barca should not attempt fixed-point computation, iterative cycles, or special loop semantics.

If users need iteration, they should write it inside a single asset function.

## `uv` is required

Barca should require `uv` for Python execution and environment management.

This is a hard product decision, not an optional adapter.

That means:

- indexing happens against a `uv`-managed project environment
- execution happens within the `uv`-managed virtualenv
- dependency state is tracked via the project's dependency graph

### Why this is the right constraint

- it narrows environment behavior
- it makes dependency hashing concrete
- it avoids supporting multiple Python launch models
- it reduces packaging ambiguity between indexing and execution

For the MVP, Barca should not support:

- raw `python`
- `poetry run`
- `pipenv`
- conda
- arbitrary user-defined execution shims

Those can be reconsidered later, but supporting them now would add a lot of complexity with little product value.

## Preflight consistency is required

Before executing a planned asset step, Barca should verify that the currently importable asset definition still matches the indexed definition.

At minimum, that means checking:

- module path resolves
- function name resolves
- current `definition_hash` matches the planned `definition_hash`

If not, execution should fail fast and require re-indexing.

This prevents the orchestrator from running stale plans against changed source.

## History is append-only

Barca should not delete old asset definitions or old materializations as part of normal operation.

Instead, Barca should:

- keep prior definitions
- keep prior materializations
- mark old states as stale, superseded, inactive, or historical as appropriate
- render history in the UI/TUI rather than hiding it

This is a core part of the product.

The point of Barca is not only to run assets, but to preserve approximate lineage over time.

### What this means in practice

- if code changes, create a new asset definition record
- if a partition disappears, keep its historical materializations
- if a run becomes stale, mark it stale rather than deleting it
- if an asset is removed from the current codebase, keep its prior history and mark it inactive

The storage model should therefore be append-only for definitions and materializations, with status flags rather than destructive updates.

### Pruning

History accumulates over time. `barca prune` is the only operation that permanently removes history — it deletes all artifacts, materialisations, and DB records not reachable from the current active DAG (removed assets, removed partition values, old definition hash versions no longer referenced by any current asset). It is explicit and destructive; run it before production deployments or to recover disk space.

## Freshness declarations

Every asset, sensor, and effect declares how eagerly Barca keeps its output up to date. The `freshness` parameter is the core primitive — not `schedule`.

Three freshness kinds exist:

- `Always` (default for `@asset` and `@effect`): Barca keeps this asset fresh automatically. Any upstream change cascades through and re-materialises it during `barca run`.
- `Manual`: Barca never auto-updates this asset, even when stale. Only refreshed via explicit `barca assets refresh`. **`Manual` freshness blocks downstream**: a downstream `Always` asset cannot be auto-materialised if any of its transitive upstream assets has `Manual` freshness.
- `Schedule("cron_expr")`: Barca refreshes this asset when a cron tick has elapsed since last run.

Sensors use `Manual` or `Schedule` only — `Always` is not valid for sensors (polling frequency must be declared explicitly).

## Freshness is provenance-based, not recency-based

Barca should decide freshness from provenance identity, not from whether something was run most recently.

That means:

- if the current `definition_hash` matches an older definition snapshot, that older snapshot becomes current again
- if the full `run_hash` matches an older successful materialization, that materialization is fresh again immediately
- Barca should reuse that prior materialization without recomputing it

This is an important invariant.

If code changes from version A to version B and later returns to version A, Barca should be able to reuse the original A outputs as long as the full provenance matches.

### Example

- `a` at definition hash `H1`
- `b` computed from `a@H1`
- later `a` changes to definition hash `H2`
- later `a` changes back to definition hash `H1`

If Barca already has a successful `b` materialization whose `run_hash` corresponds to `a@H1`, that `b` materialization should be considered fresh again without recomputation.

This is why append-only history matters: old valid provenance states remain reusable.

## Asset continuity is approximate but explicit

Barca needs a notion of “this is probably the same asset as before” so that code history remains legible across changes.

For the MVP, use this continuity policy:

- primary continuity key: explicit asset `name` if provided
- otherwise: repo-relative file path + function name
- if two live assets resolve to the same continuity key during indexing, fail with a duplicate-asset error

That is the safe default.

### Why not use `filepath/function name OR function definition` as identity

That rule is too loose for automatic identity because:

- two distinct assets can share nearly identical function definitions
- a copied function in another file may be new work, not the same asset
- a renamed or moved asset should usually keep history, but naive source matching can merge unrelated code

So for the MVP:

- use the continuity key above for canonical live identity
- use source similarity only as a non-authoritative history hint

Barca can later surface probable rename/move suggestions such as:

- “this definition looks similar to a previously indexed asset”

But it should not silently merge those histories automatically.

## User code is the execution source of truth

Barca stores inspected source snapshots for provenance, debugging, and reproducibility metadata.

Barca does not execute those stored snapshots directly.

Instead, the runner imports the real module from the user’s codebase and executes the real function in the `uv` environment.

That keeps:

- imports honest
- tracebacks accurate
- helper/module semantics intact
- environment problems correctly attributed to user code

## Practical implications

These constraints imply:

- asset discovery requires importable, source-backed Python modules
- notebook-defined assets are not first-class indexed assets in v1
- graph validation is a mandatory indexing step
- the execution engine must verify the planned `definition_hash`
- dependency cone hashing is part of cache invalidation and provenance
- definitions and materializations are append-only records
- duplicate live continuity keys should fail indexing
- old matching provenance can become current again without rerunning
