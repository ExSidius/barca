# Test Coverage Roadmap

This document identifies gaps in unit and integration test coverage and specifies the tests needed to close them. It complements `testing.md`, which covers orchestration integration tests and Playwright E2E tests.

---

## Current state

| Crate | Unit tests | Integration tests | Notes |
|---|---|---|---|
| `barca-core` | 0 | 0 | Completely untested |
| `barca-server` | 4 (display.rs) | 71 (api_tests.rs) | Strong API coverage, weak unit coverage |
| `barca-cli` | 0 | 26 (5 test files) | Good CLI integration, no unit tests |
| `barca-py` | 0 | 0 | Tested implicitly via CLI integration tests |

**Total: ~94 tests**, almost entirely integration-level. Core logic is exercised only as a side effect of higher-level tests — failures are hard to diagnose and edge cases go unexercised.

---

## Phase 1: Core hashing functions (`barca-core`)

**Priority**: Critical — cache correctness depends on these.
**Location**: `crates/barca-core/src/hashing.rs`
**Effort**: Low (pure functions, no setup needed)

### `compute_definition_hash()`

| # | Test | Assert |
|---|---|---|
| 1.1 | Determinism — call twice with identical `DefinitionHashPayload` | Hashes are equal |
| 1.2 | Sensitivity to `source_text` — change one character | Hash changes |
| 1.3 | Sensitivity to `decorator_metadata_json` — change a value | Hash changes |
| 1.4 | Sensitivity to `uv_lock_hash` — `Some("a")` vs `Some("b")` | Hash changes |
| 1.5 | Sensitivity to `uv_lock_hash` — `Some("a")` vs `None` | Hash changes |
| 1.6 | Sensitivity to `protocol_version` — bump version string | Hash changes |
| 1.7 | Sensitivity to `codebase_hash` — different codebase hash | Hash changes |
| 1.8 | Field ordering independence — same logical payload, same hash | Hash is stable regardless of construction order |

### `compute_run_hash()`

| # | Test | Assert |
|---|---|---|
| 1.9 | Determinism — same inputs produce same hash | Hashes are equal |
| 1.10 | Sensitivity to `definition_hash` | Hash changes when definition hash changes |
| 1.11 | Sensitivity to upstream materialization IDs — add/remove/reorder | Hash changes |
| 1.12 | Sensitivity to partition values | Hash changes when partition key/value changes |
| 1.13 | Empty upstream list vs non-empty | Different hashes |
| 1.14 | Empty partition list vs non-empty | Different hashes |

### `compute_codebase_hash()`

| # | Test | Assert |
|---|---|---|
| 1.15 | Determinism — same file set, same hashes | Stable output |
| 1.16 | Sensitivity to file content hash — change one file's hash | Codebase hash changes |
| 1.17 | Sensitivity to file addition — add a file to the set | Codebase hash changes |
| 1.18 | Sensitivity to file removal — remove a file from the set | Codebase hash changes |
| 1.19 | Order independence — shuffled input produces same hash | Merkle tree is sorted |

### `slugify()`

| # | Test | Assert |
|---|---|---|
| 1.20 | Simple name — `"greeting"` | `"greeting"` |
| 1.21 | Spaces — `"my asset"` | `"my-asset"` or similar |
| 1.22 | Special characters — `"hello@world!"` | Only alphanumeric + hyphens/underscores |
| 1.23 | Leading/trailing whitespace | Trimmed |
| 1.24 | Empty string | Returns something valid (non-empty or errors) |
| 1.25 | Unicode — `"café"` | Handled without panic |

### `sha256_hex()`

| # | Test | Assert |
|---|---|---|
| 1.26 | Known input — `"hello"` | Matches known SHA-256 hex digest |
| 1.27 | Empty string | Valid 64-char hex digest |
| 1.28 | Binary-like content | Valid 64-char hex digest |

### `optional_file_hash()`

| # | Test | Assert |
|---|---|---|
| 1.29 | Existing file | Returns `Some(hex_string)` |
| 1.30 | Nonexistent file | Returns `None` (no panic) |
| 1.31 | Empty file | Returns `Some(hash_of_empty)` |

### `relative_path()`

| # | Test | Assert |
|---|---|---|
| 1.32 | Child of base — `/a/b/c` relative to `/a/b` | `"c"` |
| 1.33 | Deeply nested — `/a/b/c/d/e` relative to `/a/b` | `"c/d/e"` |
| 1.34 | Same path — `/a/b` relative to `/a/b` | `""` or `"."` |
| 1.35 | Unrelated paths | Reasonable behavior (full path or error) |

---

## Phase 2: Store / database operations (`barca-server`)

**Priority**: High — data integrity.
**Location**: `crates/barca-server/src/store.rs`
**Effort**: Medium (requires in-memory DB setup per test)

Each test opens a fresh in-memory libSQL database via `MetadataStore::open(":memory:")` (or equivalent).

### Schema & initialization

| # | Test | Assert |
|---|---|---|
| 2.1 | `open()` on fresh DB | All tables created, no error |
| 2.2 | `open()` twice on same DB | Idempotent, no schema conflict |

### Asset CRUD

| # | Test | Assert |
|---|---|---|
| 2.3 | `upsert_asset()` + `get_asset()` round-trip | All fields match |
| 2.4 | `upsert_asset()` twice with same continuity key | Updates in place, same `asset_id` |
| 2.5 | `upsert_asset()` with different continuity keys | Different `asset_id`s |
| 2.6 | `list_assets()` returns all upserted assets | Count and names match |
| 2.7 | `list_assets()` on empty DB | Returns empty vec |
| 2.8 | `asset_id_by_continuity_key()` — existing key | Returns correct ID |
| 2.9 | `asset_id_by_continuity_key()` — missing key | Returns `None` |
| 2.10 | `asset_id_by_logical_name()` — existing name | Returns correct ID |
| 2.11 | `asset_id_by_logical_name()` — missing name | Returns `None` |

### Definition CRUD

| # | Test | Assert |
|---|---|---|
| 2.12 | `upsert_definition()` + `get_definition()` round-trip | All fields match |
| 2.13 | `definition_id_by_hash()` — existing hash | Returns correct ID |
| 2.14 | `definition_id_by_hash()` — missing hash | Returns `None` |
| 2.15 | Upsert definition with same hash twice | Idempotent, same `definition_id` |

### Materialization lifecycle

| # | Test | Assert |
|---|---|---|
| 2.16 | `insert_materialization()` with status `queued` | Record inserted, retrievable |
| 2.17 | `claim_next_materialization()` | Returns oldest queued job, sets status to `running` |
| 2.18 | `claim_next_materialization()` on empty queue | Returns `None` |
| 2.19 | `mark_materialization_success()` | Status is `success`, artifact fields populated |
| 2.20 | `mark_materialization_failure()` | Status is `failed`, `last_error` populated |
| 2.21 | `active_materialization_for_run()` — queued job exists | Returns the job |
| 2.22 | `active_materialization_for_run()` — only completed jobs | Returns `None` |
| 2.23 | `requeue_materialization()` | Status reverts to `queued` |
| 2.24 | `update_materialization_run_hash()` | Run hash updated on the record |

### Batch operations

| # | Test | Assert |
|---|---|---|
| 2.25 | `batch_check_cached_materializations()` — all cached | Returns all as hits |
| 2.26 | `batch_check_cached_materializations()` — none cached | Returns empty |
| 2.27 | `batch_check_cached_materializations()` — mixed | Returns only the cached ones |
| 2.28 | `batch_mark_materialization_success()` | All records updated |

### Edge cases

| # | Test | Assert |
|---|---|---|
| 2.29 | Duplicate `continuity_key` across two assets | Rejected or handled gracefully |
| 2.30 | Very long strings (asset name, error message) | No truncation or DB error |
| 2.31 | `get_asset()` for nonexistent ID | Returns `None` or appropriate error |

---

## Phase 3: Python bridge (`barca-server`)

**Priority**: High — production reliability.
**Location**: `crates/barca-server/src/python_bridge.rs`
**Effort**: Medium (needs stub Python scripts for subprocess testing)

These tests exercise `UvPythonBridge` with real subprocesses but controlled Python scripts (not the full `barca` Python package). Each test uses a temp directory containing a small Python script that prints known JSON to stdout.

### `inspect_modules()`

| # | Test | Assert |
|---|---|---|
| 3.1 | Valid JSON output from subprocess | Parses into `Vec<InspectedAsset>` correctly |
| 3.2 | Empty module list (no assets) | Returns empty vec |
| 3.3 | Subprocess exits with non-zero code | Returns error, does not panic |
| 3.4 | Subprocess prints invalid JSON | Returns error with context |
| 3.5 | Subprocess prints to stderr then succeeds | Assets returned, stderr logged |

### `materialize_asset()`

| # | Test | Assert |
|---|---|---|
| 3.6 | Worker returns success JSON | Parsed into `WorkerResponse` with artifact info |
| 3.7 | Worker returns failure JSON | Parsed into `WorkerResponse` with error |
| 3.8 | Worker exits non-zero with no JSON | Error returned with exit code |
| 3.9 | Worker hangs past timeout | Process killed, timeout error returned |
| 3.10 | Worker writes large stdout | Handled without OOM (bounded buffering) |

### `materialize_batch()`

| # | Test | Assert |
|---|---|---|
| 3.11 | Batch of 3 assets, all succeed | All 3 results present |
| 3.12 | Batch with one failure | Failed asset returns error, others succeed |
| 3.13 | Empty batch | Returns empty results |

### Module discovery

| # | Test | Assert |
|---|---|---|
| 3.14 | `discover_barca_modules()` with valid Python package | Finds all `.py` files |
| 3.15 | `discover_barca_modules()` with empty directory | Returns empty list |
| 3.16 | `discover_barca_modules()` skips `__pycache__` | Cache dirs excluded |

---

## Phase 4: Snapshot management (`barca-server`)

**Priority**: Medium — filesystem edge cases.
**Location**: `crates/barca-server/src/snapshot.rs`
**Effort**: Low-Medium (temp directory fixtures)

### `copy_py_tree()`

| # | Test | Assert |
|---|---|---|
| 4.1 | Copies `.py` files preserving directory structure | Files exist at expected paths |
| 4.2 | Skips `__pycache__` directories | Not present in snapshot |
| 4.3 | Skips `.pyc` files | Not present in snapshot |
| 4.4 | Handles empty source directory | Creates empty snapshot dir |
| 4.5 | Handles nested directories | Full tree replicated |

### `SnapshotManager::ensure_snapshot()`

| # | Test | Assert |
|---|---|---|
| 4.6 | First call creates snapshot | Snapshot directory exists |
| 4.7 | Second call with same hash returns existing | No new directory created |
| 4.8 | Call with different hash creates new snapshot | Two snapshot dirs exist |
| 4.9 | `.venv` is symlinked, not copied | Symlink target matches original |
| 4.10 | Config files (`pyproject.toml`, `uv.lock`) are copied | Present in snapshot root |

### `SnapshotManager::cleanup()`

| # | Test | Assert |
|---|---|---|
| 4.11 | Removes snapshots not in keep-set | Old dirs deleted |
| 4.12 | Keeps snapshots in keep-set | Current dirs preserved |
| 4.13 | No-op when no old snapshots exist | No error |

---

## Phase 5: SSE streams (`barca-server`)

**Priority**: Medium — UI reliability.
**Location**: `crates/barca-server/src/server.rs`
**Effort**: Medium (requires test server with SSE client)

These are integration-level but test the SSE mechanism specifically, which the existing API tests skip.

| # | Test | Assert |
|---|---|---|
| 5.1 | `GET /stream` returns `text/event-stream` content type | Header correct |
| 5.2 | `main_stream` sends initial asset list on connect | First event contains asset HTML |
| 5.3 | `main_stream` pushes update when materialization completes | Patch event received with updated badge |
| 5.4 | `asset_panel_stream` sends asset detail on connect | First event contains panel HTML |
| 5.5 | `asset_panel_stream` pushes update on materialization | Updated panel content received |
| 5.6 | `job_panel_stream` sends job detail on connect | First event contains job panel HTML |
| 5.7 | `job_panel_stream` pushes update on job completion | Updated status received |
| 5.8 | Client disconnect does not crash server | No panic, no resource leak |
| 5.9 | Multiple concurrent SSE connections | Each receives independent events |

---

## Phase 6: Template rendering (`barca-server`)

**Priority**: Low-Medium — XSS prevention, structural correctness.
**Location**: `crates/barca-server/src/templates.rs`
**Effort**: Low (string assertions)

### `escape_html()`

| # | Test | Assert |
|---|---|---|
| 6.1 | Escapes `<` | `&lt;` |
| 6.2 | Escapes `>` | `&gt;` |
| 6.3 | Escapes `&` | `&amp;` |
| 6.4 | Escapes `"` | `&quot;` |
| 6.5 | Escapes `'` | `&#x27;` or `&#39;` |
| 6.6 | Passes through safe strings unchanged | No modification |
| 6.7 | Handles empty string | Returns empty string |
| 6.8 | Mixed content — `<script>alert("xss")</script>` | All dangerous chars escaped |

### Structural correctness (spot checks)

| # | Test | Assert |
|---|---|---|
| 6.9 | `asset_card()` output contains asset name | Substring match |
| 6.10 | `asset_card()` escapes user-provided asset name | XSS payload in name is escaped |
| 6.11 | `asset_panel()` output contains definition hash | Substring match |
| 6.12 | `job_row()` output contains status badge | Substring match |

---

## Phase 7: Server helper functions (`barca-server`)

**Priority**: Low-Medium.
**Location**: `crates/barca-server/src/server.rs`
**Effort**: Low

### `format_timestamp()` / `format_last_updated()`

| # | Test | Assert |
|---|---|---|
| 7.1 | Recent timestamp (seconds ago) | `"Xs ago"` format |
| 7.2 | Timestamp from hours ago | `"Xh ago"` format |
| 7.3 | Zero / epoch timestamp | Does not panic; returns reasonable string |
| 7.4 | Future timestamp | Does not panic |

### `classify_asset_state()`

| # | Test | Assert |
|---|---|---|
| 7.5 | No materialization | Returns "not materialized" state |
| 7.6 | Materialization with matching run hash | Returns "fresh" state |
| 7.7 | Materialization with mismatched run hash | Returns "stale" state |
| 7.8 | Failed materialization | Returns "failed" state |
| 7.9 | Running materialization | Returns "running" state |

---

## Phase 8: CLI unit tests (`barca-cli`)

**Priority**: Low.
**Location**: `crates/barca-cli/src/commands.rs`, `crates/barca-cli/src/lib.rs`
**Effort**: Low

| # | Test | Assert |
|---|---|---|
| 8.1 | Clap parser accepts `serve` subcommand | No parse error |
| 8.2 | Clap parser accepts `assets list` | No parse error |
| 8.3 | Clap parser accepts `assets show 1` | No parse error, ID = 1 |
| 8.4 | Clap parser accepts `assets refresh 1` | No parse error |
| 8.5 | Clap parser accepts `jobs list` | No parse error |
| 8.6 | Clap parser accepts `jobs show 1` | No parse error |
| 8.7 | Clap parser accepts `reindex` | No parse error |
| 8.8 | Clap parser accepts `reset --db --artifacts` | Flags parsed correctly |
| 8.9 | Unknown subcommand | Parse error |

---

## Implementation order

```
Phase 1 (core hashing)     ████████████  ← start here, highest value per effort
Phase 2 (store)             ████████████
Phase 3 (python bridge)     ████████████
Phase 4 (snapshot)          ████████
Phase 5 (SSE streams)       ████████
Phase 6 (templates)         ██████
Phase 7 (server helpers)    ██████
Phase 8 (CLI parsing)       ████
```

Phases 1-3 are critical for production confidence. Phases 4-8 are hardening and polish.

---

## Running the new tests

All new unit tests should run with the existing command:

```bash
cargo test -p barca-server -p barca-cli -p barca-core
```

No new test infrastructure required for phases 1, 2, 6, 7, 8. Phases 3-5 need:

- **Phase 3**: A `tests/fixtures/` directory with small Python scripts that simulate worker output.
- **Phase 4**: Temp directory setup (use `tempfile` crate, already available).
- **Phase 5**: Test server harness (extend the existing `Scenario` builder to support SSE connections).
