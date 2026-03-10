# barca

A modern, minimal orchestrator.

Rust (axum) backend that discovers Python functions decorated with `@asset()`, materializes them on demand, stores artifacts, and serves a reactive Datastar UI.

## Quick start

```bash
uv sync
cargo run -p barca-server
```

Then open `http://127.0.0.1:3000`.

### Example app

```bash
cd examples/basic_app
uv sync
cargo run -p barca-server
```

## Docs

- [Architecture](./docs/architecture.md)
- [Core constraints](./docs/core-constraints.md)
- [Datastar reference](./docs/datastar-reference.md)
- [Templates architecture](./docs/templates-architecture.md)
- [Testing checklist](./docs/testing.md)
- [Release plan](./docs/releases.md)
- [0.1.1 milestone](./docs/milestones/0.1.1.md)

### Workflow specs

- [Single asset, no inputs](./docs/workflows/01-single-asset-no-inputs.md)
- [Single asset with one input](./docs/workflows/02-single-asset-one-input.md)
- [Parametrized assets and partitions](./docs/workflows/03-parametrized-assets-and-partitions.md)
- [Asset continuity (rename/move)](./docs/workflows/04-asset-continuity-rename-and-move.md)
- [Schedule-driven reconciliation and effects](./docs/workflows/05-schedule-driven-reconciliation-and-effects.md)
- [Sensors and external observations](./docs/workflows/06-sensors-and-external-observations.md)
- [Notebook workflow](./docs/workflows/07-notebook-workflow.md)
- [Backfill and replay](./docs/workflows/08-backfill-and-replay.md)
- [Execution controls and ad hoc params](./docs/workflows/09-execution-controls-and-ad-hoc-params.md)
