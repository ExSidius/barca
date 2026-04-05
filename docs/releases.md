# Releases

This file scopes Barca by release so the MVP does not quietly expand.

## 0.1

Goal:

- ship a usable local orchestrator with a strong Python authoring model

### 0.1.1

Goal:

- implement the first workflow end to end
- establish the Python package skeleton

Included:

- first workflow only:
  - one decorated asset
  - no inputs
  - JSON output path for simple scalar values
- basic Python package skeleton:
  - `@asset`
  - metadata attachment on decorated functions
  - inspection helper for no-input assets
  - JSON serializer for supported simple values
- indexing for the first workflow
- materialization for the first workflow
- append-only metadata records for:
  - assets
  - asset definitions
  - materializations
- `.barcafiles` layout for the first workflow
- preflight `definition_hash` consistency check
- cache reuse for the first workflow
- basic CLI with:
  - asset list
  - asset detail
  - trigger materialization

Explicitly out of scope for 0.1.1:

- upstream dependencies
- `load_inputs(...)`
- partitions
- sensors
- effects
- schedules/reconciler loop
- retries/timeouts/cancellation
- notebook helpers beyond plain import-and-call
- advanced artifact formats beyond JSON

### 0.1 full scope

Included:

- pure Python uv workspace (3 packages: core, CLI, server)
- asset autodiscovery from decorator semantics
- `@asset` workflow with dependency tracking
- `@sensor` workflow with observation history
- `@effect` workflow
- schedule-driven reconciliation with:
  - `manual`
  - `always`
  - `cron(...)`
- preflight definition hash consistency checks
- append-only history for definitions and materializations
- provenance-based cache reuse
- partitions with `partitions(iterable)`
- HTTP API (FastAPI) + background scheduler
- CLI for all operations (no server required)
- free-threaded Python (3.14t) for parallel partition execution

Planned for later in 0.1:

- notebook workflow with `load_inputs(...) -> dict`
- timeout support on assets, sensors, and effects
- fixed retry policy (3 attempts, exponential backoff)
- cancellation

Explicitly out of scope for 0.1:

- replay beyond what partitions already account for
- backfill beyond what partitions already account for
- TUI
- full job/pipeline DSL
- configurable retry policy
- distributed execution
- advanced effect idempotency semantics
- automatic rename/move continuity merging
- web UI (deferred — CLI and HTTP API cover operator needs)

## 0.2

Goal:

- add operator power and historical execution workflows without changing the core asset-first model

Planned additions:

- TUI
- explicit replay support
- explicit backfill support
- historical provenance selection
- richer historical navigation

## Notes

Release boundaries matter here.

Barca 0.1 should prove that:

- the asset-first API is pleasant
- provenance-based caching works
- schedule-driven reconciliation works
- the CLI and HTTP API are strong enough to operate the system

Once that is true, 0.2 can safely add historical/operator power without destabilizing the core model.
