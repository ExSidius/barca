# Testing

## Overview

Two categories of automated tests:

1. **Orchestration integration tests** — spin up barca against the example project, exercise the API and CLI, verify asset lifecycle (indexing, materialization, staleness, re-materialization).
2. **Playwright E2E tests** — drive the browser UI, verify panels, SSE streaming, page updates, response times.

Both test suites run against `examples/basic_app/` as the fixture project.

---

## State & Output Reference

### Materialization status values

Status transitions: `queued` → `running` → `success` | `failed`

| Status | `artifact_path` | `artifact_format` | `artifact_checksum` | `last_error` |
|---|---|---|---|---|
| `queued` | null | null | null | null |
| `running` | null | null | null | null |
| `success` | `.barcafiles/{slug}/{def_hash}/value.json` | `"json"` | SHA-256 hex of file bytes | null |
| `failed` | null | null | null | error message string |

### Artifact directory structure (per successful materialization)

```
.barcafiles/{asset_slug}/{definition_hash}/
├── value.json         # serialized return value from the Python function
├── code.txt           # source text of the asset function at time of materialization
└── metadata.json      # ArtifactMetadata (see below)
```

### `metadata.json` shape

```json
{
  "asset_name": "greeting",
  "module_path": "example_project.assets",
  "file_path": "example_project/assets.py",
  "function_name": "greeting",
  "definition_hash": "<sha256>",
  "run_hash": "<sha256>",
  "serializer_kind": "json",
  "python_version": "3.x.x",
  "return_type": "str",       // or null
  "inputs": [],
  "barca_version": "<protocol version>"
}
```

### `GET /api/assets` — `AssetSummary` shape

```json
[
  {
    "asset_id": 1,
    "logical_name": "greeting",
    "module_path": "example_project.assets",
    "file_path": "example_project/assets.py",
    "function_name": "greeting",
    "definition_hash": "<sha256>",
    "materialization_status": null | "queued" | "running" | "success" | "failed",
    "materialization_run_hash": null | "<sha256>",
    "materialization_created_at": null | <unix_timestamp>
  }
]
```

### `GET /api/assets/{id}` — `AssetDetail` shape

```json
{
  "asset": {
    "asset_id": 1,
    "logical_name": "greeting",
    "continuity_key": "example_project/assets.py:greeting",
    "module_path": "example_project.assets",
    "file_path": "example_project/assets.py",
    "function_name": "greeting",
    "asset_slug": "greeting",
    "definition_id": 1,
    "definition_hash": "<sha256>",
    "run_hash": "<sha256>",
    "source_text": "@asset()\ndef greeting() -> str:\n    return \"Hello from Barca!\"",
    "module_source_text": "<full module source>",
    "decorator_metadata_json": "{}",
    "return_type": "str",
    "serializer_kind": "json",
    "python_version": "3.x.x",
    "uv_lock_hash": "<sha256>" | null
  },
  "latest_materialization": null | {
    "materialization_id": 1,
    "asset_id": 1,
    "definition_id": 1,
    "run_hash": "<sha256>",
    "status": "success",
    "artifact_path": ".barcafiles/greeting/<hash>/value.json",
    "artifact_format": "json",
    "artifact_checksum": "<sha256>",
    "last_error": null,
    "created_at": <unix_timestamp>
  }
}
```

### Definition hash identity

The `definition_hash` is a SHA-256 derived from: protocol version + function source + decorator metadata + uv.lock hash. Changing any of these inputs produces a new hash, which is how barca detects staleness.

The `run_hash` is derived from the definition hash and is used to match materializations to the exact definition that produced them. A materialization is **fresh** when its `run_hash` matches the current definition's `run_hash`; otherwise it's **stale**.

### Expected outputs for `examples/basic_app/`

| Asset | Return type | `value.json` content |
|---|---|---|
| `greeting` | `str` | `"Hello from Barca!"` |
| `slow_computation` | `dict` | `{"status": "ok", "count": 42}` |

---

## 1. Orchestration Integration Tests

**Runner**: Rust integration tests (`tests/` directory) or a dedicated test harness script.
**Setup**: Start a barca-server instance on a random port against `examples/basic_app/`, wait for readiness, run tests, tear down.

### Test cases

#### Indexing

- **Assets discovered on startup** — `GET /api/assets` returns exactly 2 assets. Assert:
  - `logical_name` values are `"greeting"` and `"slow_computation"`
  - `module_path` is `"example_project.assets"` for both
  - `function_name` matches `logical_name`
  - `definition_hash` is a 64-char hex string
  - `materialization_status` is `null` (never materialized)
  - `materialization_run_hash` is `null`
  - `materialization_created_at` is `null`

- **Reindex picks up changes** — Modify `greeting`'s return value, call `POST /api/reindex`. Assert:
  - `GET /api/assets` still returns 2 assets
  - `greeting`'s `definition_hash` has changed from the pre-reindex value
  - `slow_computation`'s `definition_hash` is unchanged
  - `greeting`'s `asset_id` is unchanged (same continuity key)

- **Reindex is idempotent** — Call `POST /api/reindex` twice without changes. Assert:
  - Both responses return identical asset counts
  - All `asset_id`, `definition_hash`, and `logical_name` values are identical across both calls

- **New asset discovered** — Add a new `@asset()` function to the module, reindex. Assert:
  - `GET /api/assets` now returns 3 assets
  - The new asset has `materialization_status: null`
  - Existing assets retain their `asset_id` and `definition_hash`

#### Materialization — state transitions

- **Queued state** — `POST /api/assets/{id}/materialize` for `greeting`. Immediately `GET /api/assets/{id}`. Assert:
  - `latest_materialization.status` is `"queued"` or `"running"` (depending on timing)
  - `artifact_path`, `artifact_format`, `artifact_checksum` are all `null`
  - `last_error` is `null`
  - `run_hash` is a non-empty string matching the current definition's `run_hash`

- **Running state** — Materialize `slow_computation` (3s sleep). Poll `GET /api/assets/{id}` during execution. Assert:
  - At some point `latest_materialization.status` == `"running"`
  - All artifact fields are still `null` while running

- **Success state** — Wait for `greeting` materialization to complete. Assert on `GET /api/assets/{id}`:
  - `latest_materialization.status` == `"success"`
  - `artifact_path` matches pattern `.barcafiles/greeting/<hash>/value.json`
  - `artifact_format` == `"json"`
  - `artifact_checksum` is a 64-char hex string
  - `last_error` is `null`
  - `materialization_created_at` is a recent unix timestamp

- **Failed state** — Materialize an asset that raises an exception. Assert:
  - `latest_materialization.status` == `"failed"`
  - `last_error` is a non-empty string containing the exception info
  - `artifact_path`, `artifact_format`, `artifact_checksum` are all `null`

#### Materialization — output content

- **`greeting` artifact content** — Read `.barcafiles/greeting/<hash>/value.json`. Assert:
  - File contents deserialize to `"Hello from Barca!"` (a JSON string)

- **`slow_computation` artifact content** — Read the value.json. Assert:
  - Deserializes to `{"status": "ok", "count": 42}`

- **`code.txt` matches source** — Read `.barcafiles/{slug}/{hash}/code.txt`. Assert:
  - Contains the function source text (the decorated function body)

- **`metadata.json` correctness** — Read and parse. Assert:
  - `asset_name` matches `logical_name`
  - `module_path` == `"example_project.assets"`
  - `function_name` matches
  - `definition_hash` matches the hash used in the directory path
  - `run_hash` is a non-empty string
  - `serializer_kind` == `"json"`
  - `python_version` matches `3.\d+\.\d+`
  - `inputs` is `[]` (no-arg assets)
  - `barca_version` is a non-empty string

- **Artifact checksum verification** — Compute SHA-256 of `value.json` bytes. Assert:
  - Matches `artifact_checksum` from the materialization record

#### Materialization — guards and deduplication

- **Duplicate materialization guard** — Fire `POST /api/assets/{id}/materialize` twice rapidly. Assert:
  - Only one materialization record is created (or second returns the existing job ID)
  - After completion, `GET /api/assets/{id}` shows exactly one latest materialization

- **Already-materialized asset** — Materialize `greeting`, wait for success, materialize again without changes. Assert:
  - The second request either returns the existing successful materialization or is a no-op
  - No new job is queued (definition hasn't changed)

#### Staleness & re-materialization

- **Stale detection after code change** — Materialize `greeting`, then change its return value, reindex. Assert:
  - `definition_hash` has changed
  - `GET /api/assets/{id}` shows `materialization_status` reflecting that the latest materialization was for a previous definition (stale)
  - The old artifact directory still exists at `.barcafiles/greeting/<old_hash>/`

- **Re-materialize produces new output** — After making `greeting` stale (above), materialize again. Assert:
  - New artifact directory created at `.barcafiles/greeting/<new_hash>/`
  - `value.json` contains the updated return value
  - `metadata.json` has the new `definition_hash`
  - Old artifact directory (`.barcafiles/greeting/<old_hash>/`) still exists (artifacts are immutable)
  - `latest_materialization.artifact_path` points to the new directory

- **Unchanged asset stays fresh** — Materialize, reindex without changes. Assert:
  - `definition_hash` is unchanged
  - `materialization_status` remains `"success"`
  - `run_hash` on the materialization matches the current definition's `run_hash`

#### Error handling

- **Materialize nonexistent asset** — `POST /api/assets/99999/materialize`. Assert:
  - HTTP 404 response
  - Response body contains an error message

- **Invalid module in barca.toml** — Configure a nonexistent module, start server. Assert:
  - Server starts (does not crash)
  - Error is logged or returned via reindex

#### JSON API contracts

- **Content-Type headers** — All `/api/` endpoints return `Content-Type: application/json`.
- **List endpoint is sorted** — `GET /api/assets` returns assets sorted by `logical_name` ascending.
- **Timestamps are unix integers** — All `created_at` fields are integer unix timestamps, not strings or ISO dates.

---

## 2. Playwright E2E Tests

**Runner**: Playwright (TypeScript or Python — TBD, likely TypeScript since we already have npm for Tailwind).
**Setup**: Same as integration tests — start barca-server, run Playwright against it, tear down.

### Test cases

#### Page load & layout

- **Index page renders all assets** — Navigate to `/`. Assert:
  - Both `greeting` and `slow_computation` appear as cards/rows
  - Each shows the asset name, module path, and a status indicator
  - Status indicators show "not materialized" state (no prior runs)

- **View toggle works** — Switch between `?view=assets` and `?view=jobs`. Assert:
  - Assets view shows asset cards
  - Jobs view shows job list (empty initially)
  - Active view indicator reflects the current selection

- **Page loads in <1s** — Assert `DOMContentLoaded` fires within a reasonable threshold.

#### Asset panel — content verification

- **Panel opens on click** — Click an asset. Assert the panel shows:
  - Asset name (e.g. "greeting")
  - Module path ("example_project.assets")
  - Function name
  - Definition hash (truncated or full)
  - Status badge showing "not materialized" or equivalent
  - Source code of the asset function

- **Panel populates via SSE** — After opening, assert:
  - The panel is not empty / not showing a loading skeleton indefinitely
  - Content arrives within 500ms of opening

- **Panel shows materialization result** — Materialize an asset, open its panel. Assert:
  - Status badge shows "success"
  - The materialized value is displayed (e.g. `"Hello from Barca!"`)
  - Artifact path is shown
  - Timestamp of materialization is displayed

- **Panel closes cleanly** — Close the panel. Assert:
  - Panel element is removed from DOM or hidden
  - No lingering network connections (EventSource closed)

#### Materialization UX — state transitions in the UI

- **Before materialization** — Asset card shows:
  - No status badge or a "not materialized" badge
  - Materialize button is enabled

- **During materialization (queued/running)** — After clicking materialize:
  - Status badge changes to show in-progress state (spinner or "running" text)
  - Materialize button is disabled or hidden (prevent double-click)
  - If panel is open, panel reflects the running state

- **After successful materialization** — Assert:
  - Status badge updates to "success" (via SSE, no page reload)
  - Materialize button re-enables
  - If panel is open, it shows the result value and artifact metadata
  - No full page reload occurred (verify via navigation event or page reference stability)

- **After failed materialization** — Assert:
  - Status badge shows error state
  - If panel is open, error message is displayed
  - Materialize button re-enables (user can retry)

- **Slow materialization progress** — Materialize `slow_computation`. Assert:
  - UI shows running state for ~3s
  - During that time, the rest of the page remains interactive
  - After completion, badge updates to success

#### SSE streaming behavior

- **No polling** — Monitor network requests after page load. Assert:
  - Only SSE connections (EventSource), no XHR polling intervals
  - SSE connection stays open

- **Multiple SSE streams coexist** — Open asset panel while main page stream is active. Assert:
  - Both streams receive events independently
  - Closing the panel doesn't break the main page stream

- **SSE delivers correct patch targets** — Trigger materialization. Assert:
  - The SSE event patches the correct DOM element (the specific asset's status badge, not some other asset)
  - Other assets on the page are not affected

#### Jobs view — content verification

- **Job appears after materialization** — Trigger materialization, switch to jobs view. Assert:
  - Job row shows: asset name, status, timestamp
  - Status matches the current materialization state

- **Job panel streams updates** — Click a job. Assert:
  - Panel shows job details (asset name, run hash, status)
  - If job is still running, panel live-updates when it completes

- **Completed job shows artifact info** — After materialization success, view the job. Assert:
  - Artifact path is shown
  - Artifact format is shown
  - No error message

#### Responsiveness & dark mode

- **Dark mode renders correctly** — If applicable, toggle dark mode. Assert all elements render without broken colors or invisible text.
- **No layout shifts during updates** — Materialize an asset and assert no CLS (Cumulative Layout Shift) above threshold when badges/panels update.

---

## Infrastructure & tooling (TODO)

- [ ] **Test server harness** — Helper that starts barca-server on a random port, waits for `/api/assets` to return 200, yields the base URL, and kills the process on teardown.
- [ ] **Fixture management** — Copy `examples/basic_app/` to a temp directory per test run. Provide helpers to modify asset source files and reindex.
- [ ] **CI integration** — GitHub Actions workflow that builds the workspace, builds the Python extension, and runs both test suites.
- [ ] **Playwright config** — `playwright.config.ts` with base URL from the test harness, reasonable timeouts, and screenshot-on-failure.
- [ ] **Test data cleanup** — Each test run should use an isolated `.barca/` and `.barcafiles/` directory so tests don't interfere with each other or with local dev state.
- [ ] **Failing asset fixture** — Add an `@asset()` function to the example project that deliberately raises an exception, for error-path testing.
