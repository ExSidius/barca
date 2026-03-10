# Releases

This file scopes Barca by release so the MVP does not quietly expand.

## 0.1

Goal:

- ship a usable local orchestrator with a strong Python authoring model and a real web UI

### 0.1.1

Goal:

- implement only the first workflow end to end
- establish the Rust application skeleton
- establish the Python package skeleton
- establish a minimal Datastar web UI

Included:

- first workflow only:
  - one decorated asset
  - no inputs
  - JSON output path for simple scalar values
- basic Rust app skeleton:
  - `tokio` runtime
  - HTTP server
  - Datastar frontend shell
  - local metadata service abstraction
  - basic executor abstraction
- basic Python package skeleton:
  - `@asset`
  - inspection helper for no-input assets
  - worker entrypoint for no-input asset execution
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
- basic web UI with:
  - asset list
  - asset detail page
  - latest materialization status
  - trigger materialization action
  - live status update for a running materialization

Explicitly out of scope for 0.1.1:

- upstream dependencies
- `load_inputs(...)`
- partitions
- sensors
- effects
- schedules/reconciler loop
- retries/timeouts/cancellation
- notebook helpers beyond plain import-and-call
- advanced artifact formats beyond JSON for the first happy path

Exit criteria:

- a user can define the first workflow asset
- Barca can index it
- Barca can materialize it through the Rust orchestrator
- Barca writes the expected `.barcafiles/...` layout
- Barca reuses the prior materialization when the definition has not changed
- the web UI shows the asset and lets the user trigger the run

Included:

- Rust runtime with `tokio`
- HTTP/API server and main web UI
- asset autodiscovery from decorator semantics
- asset graph rendering in the web UI
- pure `@asset` workflow
- `@sensor` workflow
- `@effect` workflow
- schedule-driven reconciliation with:
  - `manual`
  - `always`
  - `cron(...)`
- preflight definition hash consistency checks
- append-only history for definitions and materializations
- provenance-based cache reuse
- notebook workflow with `load_inputs(...) -> dict`
- partitions with:
  - `partitions(iterable)`
  - `partitions_from(...)`
- ad hoc runtime params included in cache identity
- timeout support on assets, sensors, and effects:
  - `timeout_seconds`, default `300`
- fixed retry policy:
  - 3 attempts total
  - exponential backoff
- real-time running state in the web UI
- real-time cancellation
- cancelled and timed-out runs recorded as incomplete and not published as current

Explicitly out of scope for 0.1:

- replay beyond what partitions already account for
- backfill beyond what partitions already account for
- TUI
- full job/pipeline DSL
- configurable retry policy
- distributed execution
- advanced effect idempotency semantics
- automatic rename/move continuity merging

Interpretation of the backfill/replay cut:

- users can still run specific partitions
- users can still rerun stale assets
- users cannot yet target arbitrary historical upstream materializations or sensor observations as a first-class replay feature

## 0.2

Goal:

- add operator power and historical execution workflows without changing the core asset-first model

Planned additions:

- TUI
- explicit replay support
- explicit backfill support
- historical provenance selection in UI/operator flows
- replay/backfill run modes distinct from normal refresh runs
- richer historical navigation in the UI/TUI

Likely candidates for 0.2, but not locked:

- thin job/run-selection layer
- partition inheritance ergonomics
- richer effect controls
- rename/move advisory matching in the UI

## Notes

Release boundaries matter here.

Barca 0.1 should prove that:

- the asset-first API is pleasant
- provenance-based caching works
- schedule-driven reconciliation works
- the web UI is strong enough to operate the system

Once that is true, 0.2 can safely add historical/operator power without destabilizing the core model.
