# Architecture

This document describes the 0.1 Barca architecture at a high level.

The main principle is:

- Python owns authoring and runtime values
- Rust owns orchestration, state, scheduling, API, and operator UX

That split keeps the user-facing API very Pythonic while still giving Barca a strong local runtime and UI.

## High-level shape

Barca is two cooperating systems:

1. a Python SDK used by end users to define assets, sensors, and effects
2. a Rust orchestrator that discovers those definitions, plans work, executes workers, tracks state, and serves the UI/API

At runtime, the Rust side launches isolated Python worker processes via `uv run`.

Those workers import and execute the user’s real code from its original location.

## Why this split

Python should own:

- decorators
- function authoring
- runtime types like pandas and polars
- notebook ergonomics
- serializers/deserializers close to Python values

Rust should own:

- DAG discovery coordination
- persistent metadata
- scheduling and reconciliation
- concurrency and cancellation
- execution supervision
- web API
- Datastar-driven web UI

This is the cleanest way to avoid rebuilding Python ergonomics in Rust while also avoiding a Python-based orchestrator control plane.

## Rust responsibilities

The Rust side is the product core.

For 0.1 it should implement:

- indexing and discovery orchestration
- metadata persistence in the local Turso-backed store
- DAG validation and cycle detection
- staleness computation
- schedule eligibility computation
- reconciler loop
- run claiming and coordination
- worker process launching via `uv run`
- timeout enforcement
- retry and backoff behavior
- cancellation
- real-time event streaming to the UI
- HTTP/API server
- Datastar web UI

### Rust subsystems

Recommended internal modules:

- `indexer`
  - discover configured Python modules
  - ask Python inspection code to describe nodes
  - validate graph shape
- `metadata`
  - read/write definitions, materializations, observations, executions, run attempts
- `planner`
  - compute stale state
  - compute runnable state
  - expand partitions
  - resolve ad hoc params
- `executor`
  - spawn and monitor Python worker processes
  - enforce timeout and cancellation
  - publish success/failure/cancelled results
- `reconciler`
  - periodic manager loop for schedule-driven operation
- `api`
  - JSON endpoints and Datastar fragment endpoints
- `ui`
  - server-rendered views and streaming updates

## Python responsibilities

The Python side should stay intentionally small and author-focused.

For 0.1 it should implement:

- `@asset`
- `@sensor`
- `@effect`
- `asset_ref(...)`
- `partitions(...)`
- `partitions_from(...)`
- `cron(...)` helper
- decorator metadata capture
- inspection helpers used by indexing
- worker entrypoint used by Rust
- serializer/deserializer layer
- notebook helpers such as:
  - `load_inputs(...)`
  - `materialize(...)`
  - `read_asset(...)`
  - `list_versions(...)`

### Python package structure

Recommended conceptual modules:

- `barca.decorators`
  - decorators and metadata containers
- `barca.refs`
  - asset refs, partition refs, schedule helpers
- `barca.inspect`
  - module/function inspection for indexing
- `barca.worker`
  - CLI/entrypoint called by `uv run`
- `barca.io`
  - serializers and deserializers
- `barca.notebook`
  - `load_inputs`, `read_asset`, helper APIs
- `barca.types`
  - Pydantic models and supported type declarations

## Boundary between Rust and Python

The boundary should be narrow and explicit.

Rust should not try to understand Python runtime values directly.

Python should not own orchestration state or scheduling.

The contract between them should be a small manifest/protocol.

### Rust -> Python worker input

For each execution attempt, Rust should pass a manifest describing:

- node kind: asset / sensor / effect
- module path
- function name
- expected `definition_hash`
- selected upstream artifact references
- selected params
- selected partition key
- serializer expectations
- output staging directory

### Python worker -> Rust result

The Python worker should return structured result data describing:

- success / failure / cancelled / timed out
- produced artifact manifest, if any
- output metadata
- logs or error payload
- sensor `updated_detected` when applicable

This should be JSON-based so the boundary stays simple.

## Discovery flow

Discovery should be coordinated by Rust but executed through Python-aware inspection.

Recommended flow:

1. Rust loads Barca config and Python source roots.
2. Rust launches a Python inspection command via `uv run`.
3. Python imports modules, discovers decorated nodes, and returns structured metadata.
4. Rust validates:
   - duplicate continuity keys
   - dependency refs
   - partition metadata
   - DAG acyclicity
5. Rust writes definitions into metadata storage.

This keeps Python introspection in Python, where it belongs.

## Execution flow

For a normal asset execution:

1. Rust selects a runnable stale node.
2. Rust resolves upstream provenance, params, and partition key.
3. Rust computes the intended `run_hash`.
4. Rust checks for an existing successful matching record.
5. If matched, Rust reuses it without spawning Python.
6. If not matched, Rust spawns a Python worker with the execution manifest.
7. Python imports the real user module and calls the real function.
8. Python serializes outputs to the staging directory.
9. Rust validates and publishes the result into durable metadata and `.barcafiles`.

This keeps cache reuse and state transitions in Rust, while actual user code execution stays in Python.

## Storage architecture

Barca uses two storage layers:

### Metadata store

Local Turso-backed store for:

- assets
- sensors
- effects
- definitions
- materializations
- sensor observations
- effect executions
- run attempts
- state transitions
- provenance edges

This is append-only for historical records.

### Artifact store

Filesystem-backed `.barcafiles` tree for:

- JSON outputs
- Parquet outputs
- pickle outputs
- `code.txt`
- per-materialization metadata snapshots

Turso is the source of truth for identity and lineage.

The filesystem is the durable artifact payload layer.

## UI architecture

The 0.1 UI should be Rust-first and server-driven.

Recommended stack:

- `axum` for HTTP
- Datastar for incremental updates
- server-rendered HTML fragments

The UI should consume the same state model the reconciler uses:

- DAG structure from autodiscovered decorator metadata
- node states from reconciler state
- run attempts from executor metadata
- provenance history from the metadata store

This avoids a split between “control-plane truth” and “UI truth.”

## Real-time behavior

Rust should own:

- worker lifecycle
- live run state
- cancellation
- timeout deadlines
- retry scheduling

The UI should subscribe to those state changes through server-side streaming or Datastar event updates.

That means users can:

- watch running nodes live
- cancel them live
- see retries and timeouts
- see stale/fresh transitions as they happen

## Web UI vs Python helper APIs

The web UI is for operators.

The Python helper APIs are for authors and notebook users.

They should share the same metadata and selection semantics, but serve different workflows:

- web UI
  - inspect graph
  - inspect stale/runnable state
  - run/cancel nodes
  - inspect lineage and history
- Python helpers
  - `load_inputs(...)`
  - manual materialization
  - notebook-friendly experimentation

## Why not put more in Python

A Python-first control plane would make some things easier initially, but it would weaken:

- process supervision
- cancellation and timeout handling
- UI/server concurrency
- metadata/service structure

Rust is the better place for the long-running orchestrator and operator surface.

## Why not put more in Rust

Trying to move authoring, value typing, or serialization too far into Rust would make the Python UX worse.

In particular, Rust should not own:

- dataframe semantics
- Python type inspection logic
- notebook helper ergonomics
- reconstruction of user code behavior

That would produce a more complicated system with a worse API.

## 0.1 architecture summary

For 0.1, the clean balance is:

- Python package:
  - thin, ergonomic, author-facing
  - decorators, inspection, worker runtime, serializer logic, notebook helpers
- Rust application:
  - authoritative orchestrator
  - metadata, scheduling, execution control, API, web UI

This keeps the MVP narrow while still producing a real orchestrator rather than a library with a thin wrapper.
