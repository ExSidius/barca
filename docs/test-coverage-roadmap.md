# Test Coverage Roadmap

This document identifies gaps in test coverage and specifies tests needed to close them.

---

## Current state

| Area | Tests | Notes |
|------|-------|-------|
| Basic lifecycle | 5 | Reindex, refresh, caching, reset, idempotency |
| Dependencies | 5 | Input linkage, upstream triggers, artifact passing |
| Partitions | 6 | Fan-out, parallel execution, distinct hashes, caching |
| Reconciler | 4 | Always/manual schedules, sensor→asset→effect pipeline |
| Sensors | 10 | Decorator, discovery, observations, trigger, history, guards |
| Effects | 3 | Decorator, leaf-node validation, upstream execution |
| Schedules | 6 | Cron, manual, always; serialize/deserialize |
| Tracing | 5 | AST deps, cross-file, purity analysis |
| Codebase hash | 3 | Stability, invalidation |
| Server | 14 | All HTTP endpoints, sensor e2e, reconcile integration |

**Total: 61 tests.** Good coverage of happy paths and core features. Main gaps are edge cases and error handling.

---

## Phase 1: Hashing edge cases

**Priority**: Critical — cache correctness depends on these.
**Location**: `packages/barca-core/src/barca/_hashing.py`

### `compute_definition_hash()`

| # | Test | Assert |
|---|------|--------|
| 1.1 | Determinism — call twice with identical inputs | Hashes are equal |
| 1.2 | Source change — modify function source | Hash changes |
| 1.3 | Metadata change — modify decorator metadata | Hash changes |
| 1.4 | Dependency change — modify dependency cone hash | Hash changes |
| 1.5 | Protocol version change — modify version string | Hash changes |

### `compute_run_hash()`

| # | Test | Assert |
|---|------|--------|
| 2.1 | Determinism | Same inputs → same hash |
| 2.2 | Upstream order independence | Sorted mat IDs produce same hash regardless of input order |
| 2.3 | Different upstream versions | Different mat IDs → different hash |
| 2.4 | Partition key inclusion | Same definition + different partition key → different hash |

---

## Phase 2: Store edge cases

**Priority**: High — data integrity.
**Location**: `packages/barca-core/src/barca/_store.py`

| # | Test | Assert |
|---|------|--------|
| 3.1 | Duplicate continuity key | `upsert_indexed_asset` with same key updates, doesn't duplicate |
| 3.2 | Asset not found | `asset_detail(99999)` raises ValueError |
| 3.3 | Materialization not found | `get_materialization_with_asset(99999)` raises ValueError |
| 3.4 | Empty observation list | `list_sensor_observations` on asset with no observations returns `[]` |
| 3.5 | Multiple definitions | Reindex after code change creates new definition, old marked historical |

---

## Phase 3: Reconciler edge cases

**Priority**: High — correctness of schedule-driven execution.

| # | Test | Assert |
|---|------|--------|
| 4.1 | Cron schedule — not yet eligible | Asset with future cron tick is skipped |
| 4.2 | Failed upstream — downstream skipped | If upstream fails, downstream is not attempted |
| 4.3 | Multiple sensors — partial update | Only sensor with `update_detected=True` triggers downstream |
| 4.4 | Cycle detection | Circular dependencies raise error during reindex |
| 4.5 | Empty graph | Reconcile with no assets returns zero counts |

---

## Phase 4: Error handling

**Priority**: Medium — robustness.

| # | Test | Assert |
|---|------|--------|
| 5.1 | Asset function raises | Materialization marked failed with error message |
| 5.2 | Sensor returns wrong shape | Error recorded, not crash |
| 5.3 | Missing module | Reindex handles ImportError gracefully |
| 5.4 | Invalid barca.toml | Clear error message |
| 5.5 | Refresh non-existent asset | ValueError raised |

---

## Phase 5: Server error handling

**Priority**: Medium.

| # | Test | Assert |
|---|------|--------|
| 6.1 | Refresh sensor via assets endpoint | Returns 404 or appropriate error |
| 6.2 | Trigger non-sensor via sensors endpoint | Returns 404 |
| 6.3 | Concurrent reconcile requests | Lock prevents overlap |
| 6.4 | Health endpoint during reconcile | Still returns 200 |
