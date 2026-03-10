# barca

A modern, minimal orchestrator.

## Planning

- [Release plan](./releases.md)
- [Architecture](./docs/architecture.md)
- [0.1.1 breakdown](./docs/milestones/0.1.1.md)

## Current 0.1.1 slice

- Rust orchestrator with `tokio`, `axum`, Turso-backed metadata, and a minimal Datastar UI
- Python SDK scaffolding with `@asset`, dynamic inspection, and an isolated worker entrypoint
- First workflow only: a single no-input asset returning JSON-serializable output

## Run Barca Itself

```bash
uv sync
cargo run
```

Then open `http://127.0.0.1:3000`.

## Run Against The Example App

```bash
cargo build
cd examples/basic_app
uv sync
../../target/debug/barca
```

Then open `http://127.0.0.1:3000`.

## Design docs

- [Core constraints](./docs/core-constraints.md)
- [Single asset, no inputs workflow](./docs/workflows/01-single-asset-no-inputs.md)
- [Single asset with one upstream input workflow](./docs/workflows/02-single-asset-one-input.md)
- [Parametrized assets and partitions workflow](./docs/workflows/03-parametrized-assets-and-partitions.md)
- [Asset continuity across rename and move](./docs/workflows/04-asset-continuity-rename-and-move.md)
- [Schedule-driven reconciliation and effects](./docs/workflows/05-schedule-driven-reconciliation-and-effects.md)
- [Sensors and external observations](./docs/workflows/06-sensors-and-external-observations.md)
- [Notebook workflow](./docs/workflows/07-notebook-workflow.md)
- [Backfill and replay](./docs/workflows/08-backfill-and-replay.md)
- [Execution controls and ad hoc params](./docs/workflows/09-execution-controls-and-ad-hoc-params.md)
