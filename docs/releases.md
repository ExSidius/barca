# Releases

This file scopes barca by release so the scope does not quietly expand.

## 0.1.x (shipped)

The Rust rewrite. Replaced the pure-Python prototype with a hybrid Rust+Python
architecture: Rust parses, plans, and dispatches; Python executes.

Shipped:

- `@asset` with dependency tracking (`inputs=`), caching (`run_hash`), partitions
- `@sensor` for external state observation
- `@task` for side-effect operations (always re-runs, never cached)
- `@sink` for writing outputs to disk
- `partitions()`, `partitions_from()`, `collect()` for partition workflows
- Static analysis via ruff (never imports user code)
- Turso/libSQL for persistence (`.barca/metadata.db`)
- CLI: `barca get`, `barca run`, `barca plan`, `barca history`, `barca stats`
- `barca serve` HTTP API (axum)
- Retries with backoff (`retries=`, `retry_backoff=`)
- Benchmarks: 13-97x faster than Dagster/Prefect across all workloads

## 0.2.0 (shipped)

The execution engine rewrite. Replaced the old per-thread dispatch system with
a stateless worker pool coordinated via Unix domain sockets.

Shipped:

- **UDS coordination protocol**: length-prefixed JSON over Unix domain sockets,
  290K msg/s at 128 workers
- **Stateless workers**: receive one task at a time from Rust via a global ready queue.
  No pre-assigned queues, no head-of-line blocking.
- **`parallel()` and `parallel_map()`**: dynamic fan-out at runtime via
  SIGSTOP/SIGCONT. Scales to 1000+ items, nested parallel works recursively.
- **Type-safe coordinator**: `load_phase()` consumes the planner's Phase directly.
  StepId on every Item. Step accounting invariant (assert on count mismatch).
- **orjson** optional dependency for faster Python serialization
- **-4000 lines net**: removed executor.rs, scheduler.rs, work_plan.rs, old dispatch loop

Issues closed: [#70](https://github.com/ExSidius/barca/issues/70) (UDS communication),
[#58](https://github.com/ExSidius/barca/issues/58) (asset vs task model)

## 0.3.0 (current)

Goal: scheduling, remote I/O, observability.

Delivered:

- **Cron scheduling enforcement** — `barca serve` fires `Schedule("0 5 * * *")`
  assets, sensors, and tasks on their cron tick
  ([#54](https://github.com/ExSidius/barca/issues/54)). Includes:
  - **Durable catch-up** — last-fire times persist; a job whose tick passed
    during downtime fires once on restart.
  - **Configurable timezone** — `--timezone local|utc|<IANA>`.
  - **Parallel runs** — independent runs execute concurrently (bounded pool),
    with DB writes serialized; a job never overlaps itself.
  - **Observability** — `GET /schedule` and `barca list <files>` (next fire times);
    live reload under `--watch`.
- **Python server client** — `barca.Client` (stdlib-only) to trigger runs, poll
  status, and inspect schedules over the HTTP API.

Planned:

- **Reproducible Docker benchmarks** — containerized cross-framework comparisons
  with proper timeouts and resource constraints
  ([#65](https://github.com/ExSidius/barca/issues/65))
- **Remote storage for @sink** — S3, R2, GCS, ADLS via pluggable backends
  ([#55](https://github.com/ExSidius/barca/issues/55))
- **Backend abstraction** — pluggable DB + storage, enabling Docker and shared
  deployments ([#56](https://github.com/ExSidius/barca/issues/56))
- **APM integration** — Datadog + Sentry for observability
  ([#59](https://github.com/ExSidius/barca/issues/59))
- **Alerting hooks** — Slack webhooks, email notifications
  ([#52](https://github.com/ExSidius/barca/issues/52))

## Future (unscoped)

Not yet assigned to a release:

- Partition filtering on CLI (`--partition` flag)
  ([#57](https://github.com/ExSidius/barca/issues/57))
- File versioning for artifacts
  ([#61](https://github.com/ExSidius/barca/issues/61))
- Notebook integration
- TUI
- Distributed execution (multi-machine)
- Web UI
