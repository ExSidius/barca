# Datastar Complete Reference for Barca / Rust + Axum SPA

> Protocol version targeted: **v1.0.0-RC.1** (what the Rust SDK `datastar = "0.3"` implements).
> Rust SDK latest: `datastar = "0.3.2"` with feature `axum`.

---

## Table of Contents

1. [Core Concepts](#1-core-concepts)
2. [Signal System](#2-signal-system)
3. [All `data-*` Attribute Directives](#3-all-data--attribute-directives)
4. [Action Plugins (@get, @post, etc.)](#4-action-plugins)
5. [SSE Protocol — Exact Wire Format](#5-sse-protocol--exact-wire-format)
6. [Rust/Axum SSE Integration](#6-rustaxum-sse-integration)
7. [Panels, Modals, and Drawers](#7-panels-modals-and-drawers)
8. [Forms, Buttons, Navigation](#8-forms-buttons-and-navigation)
9. [Loading States and Optimistic UI](#9-loading-states-and-optimistic-ui)
10. [Streaming Partial HTML Updates](#10-streaming-partial-html-updates)
11. [Multiple Concurrent SSE Streams](#11-multiple-concurrent-sse-streams)
12. [Protocol Gotchas](#12-protocol-gotchas)
13. [Fragments vs Full-Page Navigation](#13-fragments-vs-full-page-navigation)
14. [Dagster-style SPA Patterns](#14-dagster-style-spa-patterns)

---

## 1. Core Concepts

### What Datastar Is

Datastar is a ~11 KB single-file JavaScript library that provides:
- **Frontend reactivity** via signals (like Alpine.js)
- **Backend-driven DOM patching** via Server-Sent Events (like htmx)

The key insight: **the backend is the source of truth**. The browser's reactive state is a copy of what the server sends. The server streams HTML fragments and signal patches; the browser morphs the DOM reactively.

### Loading Datastar

```html
<!-- Pinned to the RC.1 tag used by this project's Rust SDK -->
<script type="module"
  src="https://cdn.jsdelivr.net/gh/starfederation/datastar@1.0.0-RC.1/bundles/datastar.js">
</script>
```

The JS bundles contain all core plugins; no additional setup is required.

### Architecture at a Glance

```
Browser                                Server (Rust/Axum)
------                                 ------------------
data-* attributes declare intent       Routes return SSE streams
$signals hold reactive state           PatchElements → morph DOM nodes
@get/@post trigger HTTP requests       PatchSignals → update browser state
EventSource receives SSE events        ReadSignals extractor reads browser state
```

---

## 2. Signal System

### What Signals Are

Signals are reactive variables. Every signal is accessible in `data-*` attribute expressions using `$` prefix.

```html
<!-- Declare a signal with an initial value -->
<div data-signals:count="0"></div>

<!-- Read a signal in an expression -->
<span data-text="$count"></span>

<!-- Mutate a signal on click -->
<button data-on:click="$count++">Increment</button>
```

### Creating Signals

Three ways:

1. **`data-signals`** — explicit declaration (preferred)
2. **`data-bind`** — creates automatically when bound to an input
3. **Server SSE** — `datastar-patch-signals` event creates/updates signals

```html
<!-- Single signal -->
<div data-signals:foo="1"></div>

<!-- Nested signal (dot notation) -->
<div data-signals:form.email="''"></div>

<!-- Multiple signals at once -->
<div data-signals="{foo: 1, bar: 'hello', baz: false}"></div>
```

### Signal Naming Rules

- HTML attribute names are case-insensitive; Datastar converts hyphenated names to camelCase automatically.
- `data-signals:foo-bar` creates signal `$fooBar`
- Signal names **cannot contain `__`** (double underscore is reserved for modifiers)
- Signals starting with `_` are **local-only** — they are excluded from requests sent to the backend

```html
<!-- Local-only signal (not sent to server) -->
<div data-signals:_panel-open="false"></div>
```

### Computed Signals

Read-only signals derived from other signals:

```html
<div data-computed:full-name="$firstName + ' ' + $lastName"></div>
<span data-text="$fullName"></span>
```

### Signal Nesting

```html
<div data-signals:user.name="'Alice'"></div>
<div data-signals:user.role="'admin'"></div>
<!-- Access: $user.name, $user.role -->
```

### What Gets Sent to the Server

All signals (except `_` prefixed) are sent automatically with every request:
- **GET**: as `?datastar={"foo":1,"bar":"hello"}` query param (JSON-encoded)
- **POST/PUT/PATCH/DELETE**: as JSON body `{"foo":1,"bar":"hello"}`

The Rust extractor `ReadSignals<T>` handles both cases.

---

## 3. All `data-*` Attribute Directives

### CRITICAL SYNTAX NOTE for This Project

The project's `CLAUDE.md` notes that the correct attribute syntax uses **hyphen**, not colon: `data-on-click` vs `data-on:click`. However, the official docs and all examples from Datastar use the **colon** syntax `data-on:click`. Both appear to work because the JS library normalizes the attribute name.

**Recommendation**: Use the colon syntax (`data-on:click`) as documented officially. The barca codebase's existing `data-on-click` usage works because HTML normalizes `data-on:click` and `data-on-click` differently — check CLAUDE.md for the project's specific convention.

> **Project-specific**: `CLAUDE.md` says to use `data-on-click="@post('/url')"` (hyphen). The official Datastar docs show `data-on:click="@post('/url')"` (colon). The **colon** is the canonical form. Both work at the HTML parser level because `data-on:click` becomes the DOM attribute `data-on:click` while `data-on-click` becomes `data-on-click` — these are different attribute names. **The official SDK and examples all use colon.** Verify by testing which the barca HTML currently uses.

---

### State / Signals

#### `data-signals`
Declares or patches signals into the reactive store.

```html
<div data-signals:count="0"></div>
<div data-signals:user.name="'Alice'"></div>
<div data-signals="{page: 1, pageSize: 20, query: ''}"></div>
```

Modifiers:
- `__ifmissing` — only set if the signal doesn't already exist (useful for defaults)
- `__case.camel` / `__case.kebab` / `__case.snake` / `__case.pascal` — signal name casing

```html
<div data-signals:my-signal__ifmissing="'default'"></div>
```

#### `data-computed`
Creates a read-only derived signal. Updates automatically when dependencies change.

```html
<div data-computed:has-results="$results.length > 0"></div>
<div data-show="$hasResults">...</div>
```

Expressions must be pure (no side effects). Use `data-effect` for side effects.

#### `data-effect`
Executes a side-effectful expression whenever its signal dependencies change.

```html
<div data-effect="document.title = 'Count: ' + $count"></div>
```

---

### Display / DOM

#### `data-text`
Sets the text content of an element reactively.

```html
<span data-text="$count"></span>
<span data-text="'Hello, ' + $user.name"></span>
```

#### `data-show`
Toggles visibility (`display: none`) based on a boolean expression.

```html
<div data-show="$isLoading">Loading...</div>
<div data-show="!$isLoading && $results.length > 0">Results...</div>
```

**Tip**: Add `style="display:none"` to prevent flicker before JS initializes:

```html
<div data-show="$panelOpen" style="display:none">Panel content</div>
```

#### `data-class`
Conditionally adds/removes CSS classes.

```html
<!-- Single class -->
<div data-class:font-bold="$isActive"></div>

<!-- Multiple classes -->
<div data-class="{
  'bg-blue-500': $status === 'running',
  'bg-green-500': $status === 'success',
  'bg-red-500': $status === 'failed',
  'text-white': $status !== 'idle'
}"></div>
```

#### `data-style`
Sets inline CSS properties reactively.

```html
<div data-style:display="$hidden && 'none'"></div>
<div data-style="{
  'background-color': $color,
  'opacity': $loading ? '0.5' : '1'
}"></div>
```

Empty string, `null`, `undefined`, or `false` restores the original inline style value.

#### `data-attr`
Sets arbitrary HTML attributes reactively.

```html
<button data-attr:disabled="$isLoading"></button>
<input data-attr:placeholder="'Search ' + $entityName + '...'"></input>
<a data-attr:href="'/assets/' + $selectedId"></a>
```

Multiple attributes at once:

```html
<button data-attr="{'aria-expanded': $open, disabled: $loading}"></button>
```

---

### Interaction

#### `data-on`
Attaches event listeners. The expression runs when the event fires. The `evt` variable is the event object; `el` is the element.

```html
<button data-on:click="@post('/materialize')">Run</button>
<input data-on:input="$query = el.value">
<div data-on:keydown="evt.key === 'Escape' && ($panelOpen = false)"></div>
```

Modifiers (chain with `__`):
- `__once` — fire only once
- `__prevent` — calls `evt.preventDefault()`
- `__stop` — calls `evt.stopPropagation()`
- `__passive` — marks listener as passive (cannot `preventDefault`)
- `__capture` — use capture phase
- `__window` — attach to `window` instead of element
- `__outside` — triggers when event occurs outside the element
- `__debounce.500ms` — debounce by 500ms (also: `.1s`, `.leading`, `.notrailing`)
- `__throttle.200ms` — throttle by 200ms (also: `.noleading`, `.trailing`)
- `__delay.1s` — delay execution by 1s
- `__viewtransition` — wrap in `document.startViewTransition()`
- `__case.kebab` — event name casing conversion

```html
<!-- Debounced search -->
<input data-bind:query data-on:input__debounce.300ms="@get('/search')">

<!-- Click outside to close -->
<div data-on:click__outside="$panelOpen = false">Panel</div>

<!-- Window-level keydown -->
<div data-on:keydown__window="evt.key === 'k' && evt.metaKey && ($cmdOpen = true)"></div>
```

#### `data-on-intersect`
Fires when element enters or exits the viewport (IntersectionObserver).

```html
<div data-on-intersect__once="@get('/load-more')">Scroll sentinel</div>
<div data-on-intersect__exit="$visible = false"></div>
<div data-on-intersect__full="$fullyVisible = true"></div>
```

Modifiers: `__once`, `__exit`, `__half`, `__full`, `__threshold.25`

#### `data-on-interval`
Fires at a regular interval.

```html
<!-- Poll every 5 seconds -->
<div data-on-interval__duration.5s="@get('/status')"></div>
<!-- Default is 1 second -->
<div data-on-interval="$tick++"></div>
```

#### `data-on-signal-patch`
Fires whenever any signal in the reactive store updates.

```html
<div data-on-signal-patch="console.log('changed:', patch)"></div>
```

#### `data-on-signal-patch-filter`
Filters which signals trigger `data-on-signal-patch`.

```html
<div data-on-signal-patch-filter="{include: /^job/}"></div>
```

---

### Forms / Binding

#### `data-bind`
Two-way binding between a form element and a signal. Works with `input`, `select`, `textarea`.

```html
<input type="text" data-bind:search>
<input type="checkbox" data-bind:enabled>
<select data-bind:view>
  <option value="assets">Assets</option>
  <option value="jobs">Jobs</option>
</select>
<textarea data-bind:notes></textarea>
```

- Creates the signal automatically if it doesn't exist (initializes as empty string)
- For file inputs, content is base64-encoded
- For multi-select with checkboxes, use an array signal

#### `data-indicator`
Creates a boolean signal that is `true` while a fetch request is in-flight.

```html
<button
  data-on:click="@post('/materialize')"
  data-indicator:fetching
  data-attr:disabled="$fetching"
>
  <span data-show="!$fetching">Run</span>
  <span data-show="$fetching">Running...</span>
</button>
```

**Important**: `data-indicator` must appear **before** `data-init` or the action that starts the fetch in DOM order.

---

### Lifecycle

#### `data-init`
Executes an expression when the element is processed into the DOM (on load and after every patch). Equivalent to "on mount".

```html
<!-- Open SSE stream on mount -->
<div data-init="@get('/assets/panel/stream')"></div>

<!-- Initialize with delay -->
<div data-init__delay.200ms="$loaded = true"></div>
```

#### `data-ref`
Creates a signal that holds a reference to the element itself, for direct DOM access.

```html
<canvas data-ref:canvas></canvas>
<div data-effect="$canvas.getContext('2d').clearRect(0,0,100,100)"></div>
```

---

### Morphing Control

#### `data-ignore`
Prevents Datastar from processing an element (and optionally its children).

```html
<!-- Ignore element and all children -->
<div data-ignore>
  <div id="third-party-widget"></div>
</div>
<!-- Ignore only the element itself, children still processed -->
<div data-ignore__self></div>
```

#### `data-ignore-morph`
Skips this element during morphing patches (preserves DOM state exactly as-is).

```html
<!-- Useful for canvas, video, iframes, or editor components -->
<div id="editor" data-ignore-morph></div>
```

#### `data-preserve-attr`
Preserves specific attribute values during morphing (prevents the server patch from overwriting them).

```html
<!-- Keep <details> open/closed state across patches -->
<details open data-preserve-attr="open">
  <summary>Pipeline</summary>
  ...
</details>
```

---

### Debugging

#### `data-json-signals`
Renders the current signal store as JSON — invaluable for debugging.

```html
<pre data-json-signals></pre>
<!-- Filter to specific signals -->
<pre data-json-signals="{include: /^job/}"></pre>
<!-- Compact format -->
<pre data-json-signals__terse></pre>
```

---

### Pro Attributes (require commercial license — reference only)

- `data-persist` — persists signals to localStorage/sessionStorage
- `data-query-string` — syncs signals with URL query params
- `data-replace-url` — updates the browser URL without reload
- `data-match-media` — reacts to CSS media queries
- `data-animate` — animates attributes over time
- `data-custom-validity` — custom form validation messages
- `data-scroll-into-view` — scrolls element into view
- `data-on-raf` — fires on every `requestAnimationFrame`
- `data-on-resize` — fires when element dimensions change
- `data-view-transition` — sets `view-transition-name`

---

## 4. Action Plugins

Actions are called with `@` prefix inside expressions.

### HTTP Actions

All five HTTP verb actions share the same signature:

```
@get(url, options?)
@post(url, options?)
@put(url, options?)
@patch(url, options?)
@delete(url, options?)
```

By default, every request:
1. Includes `Datastar-Request: true` header
2. Sends all non-`_` signals as payload (GET → query param, others → JSON body)
3. Expects a `text/event-stream` response with SSE events

```html
<button data-on:click="@post('/assets/123/materialize')">Materialize</button>
<button data-on:click="@delete('/assets/123')">Delete</button>
<input data-on:input__debounce.300ms="@get('/search')">
```

### Action Options

```javascript
@post('/endpoint', {
  // Signal filtering
  filterSignals: {
    include: /^form\./,   // Only send signals starting with "form."
    exclude: /^_/,        // Exclude local signals (already default)
  },

  // Content type
  contentType: 'json',    // Default — sends JSON body
  contentType: 'form',    // Sends as multipart/form-data

  // For form content type: which form to serialize
  selector: '#my-form',

  // Custom headers
  headers: { 'X-CSRF-Token': csrfToken },

  // Keep SSE connection open even when page is hidden
  openWhenHidden: true,   // Default: false for GET, true for others

  // Override the entire payload
  payload: { customKey: $someValue },

  // Retry behavior
  retry: 'auto',          // Default: retry on network errors only
  retry: 'error',         // Also retry on 4xx/5xx
  retry: 'always',        // Retry on all non-204
  retry: 'never',         // Never retry
  retryInterval: 1000,    // ms between retries (default)
  retryScaler: 2,         // Exponential backoff multiplier (default)
  retryMaxWaitMs: 30000,  // Max wait between retries (default)
  retryMaxCount: 10,      // Max retry attempts (default)

  // Request cancellation
  requestCancellation: 'auto',     // Cancels existing request on same element (default)
  requestCancellation: 'cleanup',  // Cancels when element is removed
  requestCancellation: 'disabled', // Never cancel
})
```

### Response Handling by Content Type

Datastar automatically handles the response based on `Content-Type`:

| Response Content-Type | Action |
|---|---|
| `text/event-stream` | Standard SSE — process `datastar-patch-*` events |
| `text/html` | Direct DOM patch; headers `datastar-selector`, `datastar-mode` control targeting |
| `application/json` | Signal patch; header `datastar-only-if-missing` controls behavior |
| `text/javascript` | Execute as script |

For SSE-based SPAs, always return `text/event-stream`.

### Request Lifecycle Events

Listen to fetch lifecycle on any element:

```html
<div data-on:datastar-fetch="
  evt.detail.type === 'error' && ($errorMsg = 'Request failed')
"></div>
```

Event types: `started`, `finished`, `error`, `retrying`, `retries-failed`

---

## 5. SSE Protocol — Exact Wire Format

### Overview

The server sends Server-Sent Events over a long-lived HTTP connection with `Content-Type: text/event-stream`.

### General SSE Format

```
event: EVENT_TYPE\n
id: EVENT_ID\n          (optional)
retry: MILLIS\n          (optional; default 1000)
data: LINE1\n
data: LINE2\n
\n
```

Each SSE message is terminated by a **blank line** (`\n\n`). Multiple events can be sent over the same connection.

### Event Type: `datastar-patch-elements`

Morphs HTML into the DOM.

**Minimal form** (targets element by ID):
```
event: datastar-patch-elements
data: elements <div id="asset-123">Updated content</div>

```

The element with `id="asset-123"` is morphed. The default mode is `outer` (replace the whole element including the tag itself).

**Full form with all options**:
```
event: datastar-patch-elements
id: some-optional-id
data: selector #main-content
data: mode inner
data: useViewTransition true
data: elements <ul class="asset-list">
data: elements   <li>Item 1</li>
data: elements   <li>Item 2</li>
data: elements </ul>

```

**Data line keys for `datastar-patch-elements`**:

| Key | Values | Default | Description |
|---|---|---|---|
| `selector` | CSS selector string | element's own `id` | Target element for patching |
| `mode` | `outer` / `inner` / `replace` / `remove` / `prepend` / `append` / `before` / `after` | `outer` | How to apply the HTML |
| `useViewTransition` | `true` / `false` | `false` | Wrap in View Transitions API |
| `elements` | HTML string (one line per `data:`) | — | The HTML to patch in |
| `namespace` | `svg` / `mathml` | (none) | For SVG or MathML namespaces |

**Patch mode semantics**:
- `outer` — morph the entire target element (tag + children); **preserves** form values, focus, scroll
- `inner` — morph only the innerHTML; **preserves** the outer element
- `replace` — destroy and recreate the element (resets all component state)
- `remove` — delete the element from DOM
- `prepend` — insert as first child of target
- `append` — insert as last child of target
- `before` — insert before target (as sibling)
- `after` — insert after target (as sibling)

### Event Type: `datastar-patch-signals`

Updates signal values in the browser's reactive store. Uses JSON Merge Patch semantics (RFC 7386) — partial updates, `null` removes a key.

```
event: datastar-patch-signals
data: signals {"count": 42, "status": "running"}

```

With `onlyIfMissing` (only sets signals that don't already have a value):

```
event: datastar-patch-signals
data: onlyIfMissing true
data: signals {"defaultView": "assets", "pageSize": 20}

```

**Remove a signal** by setting to `null`:

```
event: datastar-patch-signals
data: signals {"tempError": null}

```

### Multiple Events in One Stream

Send them back-to-back — each blank line terminates an event:

```
event: datastar-patch-signals
data: signals {"status": "running", "progress": 0}

event: datastar-patch-elements
data: elements <div id="status-badge" class="badge-running">Running</div>

event: datastar-patch-signals
data: signals {"progress": 50}

event: datastar-patch-elements
data: selector #progress-bar
data: mode inner
data: elements <div style="width:50%"></div>

event: datastar-patch-signals
data: signals {"status": "done", "progress": 100}

event: datastar-patch-elements
data: elements <div id="status-badge" class="badge-done">Done</div>

```

### ExecuteScript (Sugar for PatchElements)

Not a separate event type — implemented as a `PatchElements` with `mode: append` on `body`:

```
event: datastar-patch-elements
data: selector body
data: mode append
data: elements <script data-effect="el.remove()">console.log('hello from server')</script>

```

---

## 6. Rust/Axum SSE Integration

### Cargo.toml

```toml
[dependencies]
datastar = { version = "0.3", features = ["axum"] }
axum = { version = "0.8", features = ["macros"] }
tokio = { version = "1", features = ["full"] }
async-stream = "0.3"  # or asynk-strim = "0.1" as used in SDK examples
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### Imports

```rust
use datastar::prelude::{PatchElements, PatchSignals, ExecuteScript};
use datastar::consts::ElementPatchMode;
use datastar::axum::ReadSignals;
use axum::response::{IntoResponse, Sse, sse::Event};
```

### Reading Signals from the Browser

```rust
#[derive(serde::Deserialize)]
pub struct MySignals {
    pub query: String,
    pub page: i64,
    pub selected_id: Option<String>,
}

// Works for both GET (query param) and POST (JSON body)
async fn handler(ReadSignals(signals): ReadSignals<MySignals>) -> impl IntoResponse {
    // signals.query, signals.page, etc.
}
```

`ReadSignals<T>` is an Axum extractor that:
- For GET: parses `?datastar=<json>` query parameter
- For POST/etc.: parses the JSON body directly

The `OptionalFromRequest` impl means you can use `Option<ReadSignals<T>>` if the signals may be absent.

### Sending SSE with `async-stream`

```rust
use async_stream::stream;
use axum::response::{IntoResponse, Sse, sse::Event};
use std::convert::Infallible;

async fn my_handler() -> impl IntoResponse {
    Sse::new(stream! {
        // Patch a signal
        let patch = PatchSignals::new(r#"{"status": "running"}"#);
        yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());

        // Do some work...
        tokio::time::sleep(Duration::from_secs(1)).await;

        // Patch an element
        let html = r#"<div id="result">Done!</div>"#;
        let patch = PatchElements::new(html);
        yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());

        // Update signals again
        let patch = PatchSignals::new(r#"{"status": "done"}"#);
        yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());
    })
}
```

### Builder API for PatchElements

```rust
// Minimal: target by element ID
PatchElements::new("<div id='foo'>content</div>")

// With selector (target something other than by ID)
PatchElements::new("<li>new item</li>")
    .selector("#my-list")
    .mode(ElementPatchMode::Append)

// Replace inner HTML only
PatchElements::new("<li>item 1</li><li>item 2</li>")
    .selector("#my-list")
    .mode(ElementPatchMode::Inner)

// Insert before another element
PatchElements::new("<div class='new-row'>...</div>")
    .selector("#row-5")
    .mode(ElementPatchMode::Before)

// Remove an element
PatchElements::new_remove("#row-5")

// With view transition
PatchElements::new("<div id='main'>...</div>")
    .use_view_transition(true)

// With SSE event ID (for reconnect replay)
PatchElements::new("<div id='foo'>...</div>")
    .id("event-123")
```

All `ElementPatchMode` variants:
- `ElementPatchMode::Outer` (default)
- `ElementPatchMode::Inner`
- `ElementPatchMode::Replace`
- `ElementPatchMode::Remove`
- `ElementPatchMode::Prepend`
- `ElementPatchMode::Append`
- `ElementPatchMode::Before`
- `ElementPatchMode::After`

### Builder API for PatchSignals

```rust
// JSON string
PatchSignals::new(r#"{"count": 42, "status": "done"}"#)

// Only set if not already present
PatchSignals::new(r#"{"defaultView": "assets"}"#)
    .only_if_missing(true)

// Remove a signal (set to null)
PatchSignals::new(r#"{"tempError": null}"#)

// With serde_json
let json = serde_json::json!({
    "jobId": job_id,
    "progress": 75,
    "status": "running"
});
PatchSignals::new(json.to_string())
```

### ExecuteScript

```rust
ExecuteScript::new("window.scrollTo(0, document.body.scrollHeight)")

// With attributes and auto_remove control
ExecuteScript::new("initEditor()")
    .auto_remove(false)
    .attributes(["type=\"module\""])
```

### Reading Signals Sent as Query Params (GET with `@get`)

For GET requests, the browser sends signals as `?datastar={"foo":"bar"}`. The `ReadSignals` extractor handles this automatically. If you need raw access:

```rust
use axum::extract::Query;
use serde::Deserialize;

#[derive(Deserialize)]
struct DatastarQuery {
    datastar: String,  // JSON-encoded signal blob
}

async fn handler(Query(q): Query<DatastarQuery>) {
    let signals: serde_json::Value = serde_json::from_str(&q.datastar).unwrap();
}
```

### Axum Response Headers for Non-SSE HTML Responses

If you return `text/html` directly (not SSE), you can control patch behavior via response headers:

```rust
use datastar::axum::header::{DATASTAR_SELECTOR, DATASTAR_MODE};

(
    [(DATASTAR_SELECTOR, "#my-target"), (DATASTAR_MODE, "inner")],
    Html("<div>New content</div>"),
)
```

---

## 7. Panels, Modals, and Drawers

### Pattern 1: Client-side Toggle with Server-loaded Content

The panel exists in the DOM but is hidden. Opening it triggers a server fetch that populates content.

```html
<!-- Toggle signal -->
<div data-signals:_panel-open="false"></div>

<!-- Trigger -->
<button data-on:click="$_panelOpen = true; @get('/assets/' + $selectedId + '/panel')">
  View Details
</button>

<!-- Panel shell (always in DOM) -->
<div
  id="panel-overlay"
  data-show="$_panelOpen"
  style="display:none"
  class="fixed inset-0 bg-black/50 z-40"
  data-on:click__outside="$_panelOpen = false"
>
  <div id="panel" class="fixed right-0 top-0 h-full w-96 bg-white shadow-xl overflow-y-auto p-6">
    <!-- Server fills this in -->
    <div id="panel-content">
      <div class="animate-pulse">Loading...</div>
    </div>
    <button data-on:click="$_panelOpen = false" class="absolute top-4 right-4">✕</button>
  </div>
</div>
```

Server handler:

```rust
async fn asset_panel(
    AxumPath(asset_id): AxumPath<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let html = render_panel_content(&asset_id, &state).await;
    Sse::new(stream! {
        let patch = PatchElements::new(html)
            .selector("#panel-content")
            .mode(ElementPatchMode::Inner);
        yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());
    })
}
```

### Pattern 2: Long-lived Panel SSE Stream (Barca's actual pattern)

The panel opens its **own persistent EventSource** that streams live updates:

```javascript
// In a web component or script
function openAssetPanel(assetId) {
  // Show the panel shell
  document.getElementById('panel').style.display = 'block';

  // Create a dedicated EventSource for this panel
  const es = new EventSource(`/assets/${assetId}/panel/stream`);

  es.addEventListener('datastar-patch-elements', (e) => {
    applyDatastarPatch(e.data);  // Manual SSE parsing
  });

  // Store for cleanup
  window._currentPanelEs = es;
}

function closePanel() {
  if (window._currentPanelEs) {
    window._currentPanelEs.close();
    window._currentPanelEs = null;
  }
  document.getElementById('panel').style.display = 'none';
}
```

The server streams updates to the panel as long as it's open:

```rust
async fn asset_panel_stream(
    AxumPath(asset_id): AxumPath<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let mut rx = state.broadcast.subscribe();

    Sse::new(stream! {
        // Initial render
        let html = render_panel(&asset_id, &state).await;
        let patch = PatchElements::new(html).selector("#panel-content").mode(ElementPatchMode::Inner);
        yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());

        // Stream updates
        loop {
            match rx.recv().await {
                Ok(event) if event.asset_id == asset_id => {
                    let html = render_panel(&asset_id, &state).await;
                    let patch = PatchElements::new(html)
                        .selector("#panel-content")
                        .mode(ElementPatchMode::Inner);
                    yield Ok::<Event, Infallible>(patch.write_as_axum_sse_event());
                }
                Err(_) => break,
                _ => {}
            }
        }
    })
}
```

### Pattern 3: Dialog/Modal

```html
<dialog id="confirm-modal" data-ref:modal>
  <h2>Confirm Action</h2>
  <p data-text="$modalMessage"></p>
  <button data-on:click="$modal.close()">Cancel</button>
  <button data-on:click="@delete($modalTarget); $modal.close()">Confirm</button>
</dialog>

<!-- Trigger -->
<button data-on:click="
  $modalMessage = 'Delete asset ' + $assetName + '?';
  $modalTarget = '/assets/' + $assetId;
  $modal.showModal()
">
  Delete
</button>
```

### Keyboard Shortcuts for Panels

```html
<!-- Global keyboard handler -->
<div
  data-on:keydown__window="
    evt.key === 'Escape' && $_panelOpen && ($__panelOpen = false);
    (evt.key === 'k' && (evt.metaKey || evt.ctrlKey)) && ($cmdOpen = !$cmdOpen)
  "
></div>
```

---

## 8. Forms, Buttons, and Navigation

### Basic Form Submission

No `<form>` element required. Any element with `data-on:click` can trigger a POST:

```html
<div data-signals="{name: '', email: ''}">
  <input type="text" data-bind:name placeholder="Name">
  <input type="email" data-bind:email placeholder="Email">
  <button data-on:click="@post('/submit')">Submit</button>
</div>
```

Signals `name` and `email` are sent automatically in the POST body.

### File Upload (Multipart Form)

```html
<form id="upload-form" enctype="multipart/form-data">
  <input type="file" name="file" />
  <button data-on:click="@post('/upload', {contentType: 'form', selector: '#upload-form'})">
    Upload
  </button>
</form>
```

### Filtering Which Signals Are Sent

```html
<!-- Only send form-prefixed signals -->
<button data-on:click="@post('/submit', {filterSignals: {include: /^form\./}})">
  Submit
</button>

<!-- Exclude specific signals -->
<button data-on:click="@post('/save', {filterSignals: {exclude: /^_/}})">
  Save
</button>
```

### Navigation Patterns

**Standard hyperlinks** — let the browser handle history:

```html
<a href="/assets">Assets</a>
<a href="/jobs">Jobs</a>
```

**Dynamic navigation** (update just the main content area):

```html
<button data-on:click="@get('/assets?view=list')">
  Assets
</button>
```

Server returns SSE that patches `#main-content`:

```rust
let patch = PatchElements::new(rendered_html)
    .selector("#main-content")
    .mode(ElementPatchMode::Inner);
```

**Tab navigation with aria**:

```html
<div role="tablist">
  <button
    role="tab"
    aria-selected="true"
    data-on:click="@get('/view/assets')"
  >Assets</button>
  <button
    role="tab"
    aria-selected="false"
    data-on:click="@get('/view/jobs')"
  >Jobs</button>
</div>
<div role="tabpanel" id="main-content">
  <!-- Server patches here -->
</div>
```

The server returns both the updated tab panel and the updated button states:

```
event: datastar-patch-elements
data: elements <div role="tabpanel" id="main-content">...assets...</div>

event: datastar-patch-elements
data: elements <div role="tablist">
data: elements   <button role="tab" aria-selected="true" ...>Assets</button>
data: elements   <button role="tab" aria-selected="false" ...>Jobs</button>
data: elements </div>

```

### Click-to-Edit Pattern

```html
<div id="asset-name-display">
  <span data-text="$assetName">my_asset</span>
  <button data-on:click="@get('/assets/123/edit')">Edit</button>
</div>
```

Server responds with an input form morphed into `#asset-name-display`:

```rust
PatchElements::new(r#"
  <div id="asset-name-display">
    <input type="text" data-bind:asset-name value="my_asset">
    <button data-on:click="@put('/assets/123')">Save</button>
    <button data-on:click="@get('/assets/123')">Cancel</button>
  </div>
"#)
```

---

## 9. Loading States and Optimistic UI

### Loading Indicator Pattern

```html
<button
  data-on:click="@post('/assets/123/materialize')"
  data-indicator:_fetching
  data-attr:disabled="$_fetching"
  data-class="{'opacity-50 cursor-wait': $_fetching, 'cursor-pointer': !$_fetching}"
>
  <span data-show="!$_fetching">Materialize</span>
  <span data-show="$_fetching" style="display:none">
    <svg class="animate-spin h-4 w-4 inline" ...></svg>
    Running...
  </span>
</button>
```

### Global Loading State

```html
<div data-signals:_any-loading="false">
  <!-- Spinner shown when anything is loading -->
  <div
    class="fixed top-4 right-4 z-50"
    data-show="$_anyLoading"
    style="display:none"
  >
    Loading...
  </div>
</div>
```

Track per-operation with named indicators:

```html
<button data-indicator:_reindex-loading data-on:click="@post('/reindex')">
  Reindex
</button>
<button data-indicator:_materialize-loading data-on:click="@post('/materialize/123')">
  Materialize
</button>

<!-- Show spinner for specific operation -->
<span data-show="$_reindexLoading">Reindexing...</span>
```

### The Tao says: Avoid Optimistic Updates

The Datastar philosophy discourages optimistic updates that could mislead users. Instead:
1. Show a loading state immediately
2. Disable the trigger element
3. Wait for the server to confirm
4. Server sends the real updated state back

```html
<!-- Anti-pattern: don't do this -->
<!-- <button data-on:click="$items.push($newItem); @post('/add')"> -->

<!-- Correct pattern -->
<button
  data-on:click="@post('/add')"
  data-indicator:_adding
  data-attr:disabled="$_adding"
>
  Add
</button>
<!-- Server responds with the updated list via PatchElements -->
```

### Progress Bar via Streaming SSE

```html
<div id="progress-container">
  <div id="progress-bar" style="width:0%" class="h-2 bg-blue-500 transition-all"></div>
  <span id="progress-pct">0%</span>
</div>
<button data-on:click="@post('/long-job')">Start</button>
```

Server streams incremental updates:

```rust
async fn long_job() -> impl IntoResponse {
    Sse::new(stream! {
        for pct in (0..=100).step_by(5) {
            let html = format!(
                r#"<div id="progress-bar" style="width:{pct}%" class="h-2 bg-blue-500 transition-all"></div>
                   <span id="progress-pct">{pct}%</span>"#
            );
            yield Ok::<Event, Infallible>(
                PatchElements::new(html).selector("#progress-container").mode(ElementPatchMode::Inner)
                    .write_as_axum_sse_event()
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
}
```

---

## 10. Streaming Partial HTML Updates

### Key Rules for Morphing

1. **Every top-level element in the HTML must have an `id`** for the morphing algorithm to find the target.
2. **Inner elements also benefit from IDs** to preserve their state during morphing.
3. **Morphing preserves**: focus, form values, scroll position, CSS transitions in progress.
4. **`data-ignore-morph`** prevents an element's subtree from being touched during a patch.

### Targeting Strategies

**By element ID** (implicit — the element carries its own id):

```
event: datastar-patch-elements
data: elements <div id="asset-status">running</div>

```

**By CSS selector** (useful for inserting without an existing target):

```
event: datastar-patch-elements
data: selector #asset-list
data: mode append
data: elements <div id="asset-999" class="asset-row">New Asset</div>

```

**Multiple elements in one event** (all are processed):

```
event: datastar-patch-elements
data: elements <div id="counter">42</div><div id="status">running</div>

```

Or across multiple events in the same stream.

### Rust Template Pattern

Build HTML as Rust strings, emit via SSE:

```rust
fn render_asset_row(asset: &Asset) -> String {
    format!(
        r#"<div id="asset-{id}" class="asset-row">
             <span class="asset-name">{name}</span>
             <span class="asset-status">{status}</span>
           </div>"#,
        id = escape_html(&asset.id),
        name = escape_html(&asset.name),
        status = escape_html(&asset.status),
    )
}

// In handler:
let patch = PatchElements::new(render_asset_row(&asset));
yield Ok(patch.write_as_axum_sse_event());
```

### Removing Elements

```rust
// Remove by selector
PatchElements::new_remove("#asset-123")

// Remove with mode
PatchElements::new("").selector("#asset-123").mode(ElementPatchMode::Remove)
```

HTML side — you don't need anything special, just don't reference the removed ID afterwards.

---

## 11. Multiple Concurrent SSE Streams

### Architecture Overview

Datastar supports multiple concurrent SSE streams per page. Each `@get`/`@post` opens its own SSE connection. The browser's EventSource can also be created manually via JavaScript.

### Broadcast Channel Pattern (Barca's Pattern)

Use `tokio::sync::broadcast` to fan out updates to all active SSE subscribers:

```rust
// In AppState
pub struct AppState {
    pub tx: tokio::sync::broadcast::Sender<AppEvent>,
    // ...
}

#[derive(Clone)]
pub struct AppEvent {
    pub asset_id: String,
    pub kind: EventKind,
}

// In main
let (tx, _) = tokio::sync::broadcast::channel(64);
```

Each SSE handler subscribes to the broadcast:

```rust
async fn asset_panel_stream(
    AxumPath(asset_id): AxumPath<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let mut rx = state.tx.subscribe();

    Sse::new(stream! {
        // Initial state
        yield Ok(PatchElements::new(render_panel(&asset_id)).write_as_axum_sse_event());

        // Listen for updates
        loop {
            match rx.recv().await {
                Ok(event) if event.asset_id == asset_id => {
                    yield Ok(PatchElements::new(render_panel(&asset_id))
                        .selector("#panel-content")
                        .mode(ElementPatchMode::Inner)
                        .write_as_axum_sse_event());
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                _ => continue,
            }
        }
    })
}
```

The main list page can have its own subscriber for the live overview:

```rust
async fn asset_list_stream(State(state): State<AppState>) -> impl IntoResponse {
    let mut rx = state.tx.subscribe();

    Sse::new(stream! {
        loop {
            match rx.recv().await {
                Ok(_event) => {
                    // Refresh the whole list whenever anything changes
                    let assets = load_assets(&state).await;
                    yield Ok(PatchElements::new(render_asset_list(&assets))
                        .selector("#asset-list")
                        .mode(ElementPatchMode::Inner)
                        .write_as_axum_sse_event());
                }
                Err(_) => break,
            }
        }
    })
}
```

### Avoiding Goroutine/Task Leaks

When the browser closes the tab or navigates away, the SSE connection drops. Axum will drop the stream, causing the `stream!` generator to be cancelled at the next `await` point. This is correct — just make sure you don't hold locks across `await` points.

### Using `tokio::select!` for Multiple Channels

```rust
Sse::new(stream! {
    let mut rx = state.tx.subscribe();
    let mut interval = tokio::time::interval(Duration::from_secs(30));

    loop {
        select! {
            Ok(event) = rx.recv() => {
                // Handle broadcast event
                yield Ok(PatchElements::new(render_update(&event))
                    .write_as_axum_sse_event());
            }
            _ = interval.tick() => {
                // Heartbeat / keepalive comment
                // SSE spec supports ": comment\n\n" as a keepalive
                // axum's Sse handles this via KeepAlive
            }
        }
    }
})
.keep_alive(
    axum::response::sse::KeepAlive::new()
        .interval(Duration::from_secs(15))
        .text("keepalive")
)
```

---

## 12. Protocol Gotchas

### v1.0.0-RC.1 Specifics

This is the version the Rust SDK (`datastar = "0.3.x"`) implements. Key things to know:

1. **Event names are `datastar-patch-elements` and `datastar-patch-signals`** — not the older `datastar-merge-fragments` or similar names from older alphas. The Rust SDK's `consts.rs` confirms: `"datastar-patch-elements"` and `"datastar-patch-signals"`.

2. **Attribute syntax: colon vs hyphen**. The HTML spec says `data-foo-bar` and `data-foo:bar` are two different attributes. Datastar's `data-on:click` uses the colon, which becomes the JS property `dataset['on:click']`. This **does work** because the Datastar library explicitly looks for the colon-separated form. Using `data-on-click` (hyphen) routes to a *different* attribute plugin that may or may not work the same way. **Always use the colon form** (`data-on:click`, `data-bind:foo`, `data-signals:count`) as shown in all official docs.

3. **Signals sent under `datastar` namespace**. The browser sends `{"key":"value"}` as the `datastar` query param for GET, or as the body directly for POST. The Rust `ReadSignals` extractor handles this.

4. **No polling**: Datastar does not poll. All server-to-client communication must be pushed via SSE. The browser opens an EventSource and the server streams.

5. **SSE event must end with `\n\n`** (blank line). This is handled automatically by the SDK when you use `write_as_axum_sse_event()`.

6. **Multi-line HTML in SSE**: Each line of HTML is sent as a separate `data:` line (the SDK does this automatically). The client reassembles them.

7. **Morphing requires IDs**: Without an `id` on the HTML element, the morphing algorithm cannot find the target. The `selector` option is the alternative.

8. **`data-indicator` placement**: Must appear before any `data-init` or action that uses it in DOM order. Put `data-indicator:foo` before `data-init="@get('/endpoint')"` on the same element, or earlier in the DOM.

9. **Double underscore `__` is reserved** for modifiers; signal names cannot contain it.

10. **`_`-prefixed signals are local**: They are excluded from request payloads automatically. Use them for purely client-side UI state (`$_panelOpen`, `$_loading`).

### Common Mistakes

**Wrong**: Forgetting the `id` on patched elements:
```html
<!-- Wrong: no id, morphing can't find this -->
<div class="status">running</div>
```
```html
<!-- Right -->
<div id="asset-123-status" class="status">running</div>
```

**Wrong**: Using `data-on-click` when you mean to use `data-on:click`:
```html
<!-- May or may not work depending on Datastar version/config -->
<button data-on-click="@post('/run')">Run</button>
<!-- Correct per official docs -->
<button data-on:click="@post('/run')">Run</button>
```

**Wrong**: Sending HTML without proper escaping:
```rust
// Vulnerable to XSS
format!("<div id='name'>{}</div>", user_input)
// Safe
format!("<div id='name'>{}</div>", escape_html(&user_input))
```

**Wrong**: Holding a mutex lock across an `await` in an SSE stream:
```rust
// Deadlock risk
let lock = state.store.lock().await;
yield Ok(patch.write_as_axum_sse_event()); // Still holding lock!
```
```rust
// Correct
let data = {
    let lock = state.store.lock().await;
    lock.get_data().await?
    // lock dropped here
};
yield Ok(PatchElements::new(render(&data)).write_as_axum_sse_event());
```

**Wrong**: Streaming SSE without proper content type. Axum's `Sse::new()` sets `Content-Type: text/event-stream` automatically — don't override it.

---

## 13. Fragments vs Full-Page Navigation

### Fragment Updates (SSE-based, SPA-like)

Use SSE + `PatchElements` to update parts of the page without navigation:

```html
<nav>
  <button data-on:click="@get('/view/assets')">Assets</button>
  <button data-on:click="@get('/view/jobs')">Jobs</button>
</nav>
<div id="main-content">
  <!-- Server patches this on navigation -->
</div>
```

Server returns just the content fragment:

```rust
async fn view_assets(State(state): State<AppState>) -> impl IntoResponse {
    let html = render_assets_view(&state).await;
    Sse::new(stream! {
        yield Ok::<Event, Infallible>(
            PatchElements::new(html)
                .selector("#main-content")
                .mode(ElementPatchMode::Inner)
                .write_as_axum_sse_event()
        );
        // Also update nav to reflect active state
        yield Ok::<Event, Infallible>(
            PatchElements::new(render_nav("assets"))
                .selector("nav")
                .mode(ElementPatchMode::Outer)
                .write_as_axum_sse_event()
        );
    })
}
```

### Full-Page Navigation

For major context switches, use standard `<a>` links. The browser handles back/forward history correctly. On load, the server renders the full page with the correct initial state.

```html
<!-- Links for full navigation -->
<a href="/assets">Assets</a>
<a href="/settings">Settings</a>
```

### Hybrid: Shallow Routing

Use `data-replace-url` (Pro) or `ExecuteScript` to update the URL without full navigation:

```rust
// Update URL when navigating between views
ExecuteScript::new(format!("history.pushState(null, '', '/assets/{}')", asset_id))
```

Or with the Pro attribute:

```html
<div data-replace-url="`/assets/${$assetId}`" data-effect="$assetId"></div>
```

---

## 14. Dagster-style SPA Patterns

Building a Dagster-like orchestrator UI with Datastar and Rust/Axum. Patterns specific to asset graphs, job pipelines, and real-time execution monitoring.

### Page Shell Structure

```html
<!DOCTYPE html>
<html>
<head>
  <script type="module" src="/datastar.js"></script>
  <link rel="stylesheet" href="/output.css">
</head>
<body class="flex h-screen bg-gray-950 text-gray-100">

  <!-- Global signal store (initialized once) -->
  <div
    data-signals:selected-asset-id="''"
    data-signals:panel-open="false"
    data-signals:current-view="'assets'"
  ></div>

  <!-- Sidebar (stable, rarely patched) -->
  <aside id="sidebar" class="w-56 flex-shrink-0 border-r border-gray-800">
    <nav>
      <a href="/?view=assets" data-on:click__prevent="@get('/view/assets')">Assets</a>
      <a href="/?view=jobs" data-on:click__prevent="@get('/view/jobs')">Jobs</a>
    </nav>
  </aside>

  <!-- Main area -->
  <div class="flex-1 flex flex-col overflow-hidden">
    <header id="page-header" class="border-b border-gray-800 px-6 py-4">
      <!-- Server patches this on view change -->
    </header>

    <main id="main-content" class="flex-1 overflow-auto p-6">
      <!-- Primary content area: server patches this -->
    </main>
  </div>

  <!-- Detail Panel (slide-in) -->
  <aside
    id="detail-panel"
    data-show="$panelOpen"
    style="display:none"
    class="w-96 border-l border-gray-800 flex-shrink-0 overflow-y-auto"
  >
    <div id="panel-content">
      <!-- Populated by /assets/{id}/panel/stream -->
    </div>
  </aside>

</body>
</html>
```

### Asset Card with Real-time Status

```html
<div
  id="asset-{id}"
  class="rounded-lg border p-4 cursor-pointer"
  data-on:click="
    $selectedAssetId = '{id}';
    $panelOpen = true;
    @get('/assets/{id}/panel')
  "
  data-class="{
    'border-blue-500': $selectedAssetId === '{id}',
    'border-gray-700': $selectedAssetId !== '{id}'
  }"
>
  <div class="flex items-center justify-between">
    <span class="font-mono text-sm">{name}</span>
    <asset-status-badge label="{status}" tone="{tone}"></asset-status-badge>
  </div>
  <p class="text-xs text-gray-500 mt-1">{last_materialized}</p>
</div>
```

### Asset List Live Update

The main asset list subscribes to a broadcast and refreshes rows:

```rust
async fn asset_list_stream(State(state): State<AppState>) -> impl IntoResponse {
    let mut rx = state.tx.subscribe();

    Sse::new(stream! {
        // Send initial full list
        let assets = state.store.lock().await.list_assets().await.unwrap();
        for asset in &assets {
            yield Ok::<Event, Infallible>(
                PatchElements::new(render_asset_row(asset)).write_as_axum_sse_event()
            );
        }

        // Stream individual row updates
        loop {
            match rx.recv().await {
                Ok(AppEvent { asset_id, .. }) => {
                    let store = state.store.lock().await;
                    if let Ok(Some(asset)) = store.get_asset(&asset_id).await {
                        drop(store);
                        yield Ok::<Event, Infallible>(
                            PatchElements::new(render_asset_row(&asset)).write_as_axum_sse_event()
                        );
                    }
                }
                Err(_) => break,
            }
        }
    })
}
```

### Materialize Button with Job Progress

```html
<button
  id="materialize-btn-{asset_id}"
  data-on:click="@post('/assets/{asset_id}/materialize')"
  data-indicator:_mat-loading
  data-attr:disabled="$_matLoading"
  class="btn-primary"
  data-class="{'opacity-50': $_matLoading}"
>
  <span data-show="!$_matLoading">Materialize</span>
  <span data-show="$_matLoading" style="display:none">Queued...</span>
</button>
```

The POST handler enqueues the job and returns a signal update:

```rust
async fn materialize_action(
    AxumPath(asset_id): AxumPath<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let job_id = enqueue_refresh_request(&asset_id, &state).await;

    Sse::new(stream! {
        // Immediately confirm enqueue
        yield Ok::<Event, Infallible>(
            PatchSignals::new(format!(r#"{{"lastJobId": "{job_id}"}}"#))
                .write_as_axum_sse_event()
        );

        // Update the button to show queued state
        yield Ok::<Event, Infallible>(
            PatchElements::new(render_asset_row_with_status(&asset_id, "queued"))
                .write_as_axum_sse_event()
        );
    })
}
```

### Command Palette (Cmd+K)

```html
<!-- Global signal -->
<div data-signals:_cmd-open="false"></div>

<!-- Keyboard trigger -->
<div
  data-on:keydown__window="
    (evt.key === 'k' && (evt.metaKey || evt.ctrlKey)) && (evt.preventDefault(), $_cmdOpen = !$_cmdOpen);
    evt.key === 'Escape' && ($_cmdOpen = false)
  "
></div>

<!-- Palette -->
<div
  data-show="$_cmdOpen"
  style="display:none"
  class="fixed inset-0 z-50 flex items-start justify-center pt-24"
  data-on:click="$_cmdOpen = false"
>
  <div
    class="w-full max-w-lg bg-gray-900 rounded-xl shadow-2xl border border-gray-700"
    data-on:click__stop=""
  >
    <input
      type="text"
      data-bind:_cmd-query
      data-on:input__debounce.200ms="@get('/cmd/search')"
      placeholder="Search assets, jobs..."
      class="w-full p-4 bg-transparent border-b border-gray-700 outline-none"
      data-init="el.focus()"
    >
    <div id="cmd-results" class="max-h-64 overflow-y-auto">
      <!-- Server patches here -->
    </div>
  </div>
</div>
```

### Inline Log Streaming

```html
<div id="job-logs-{job_id}" class="font-mono text-xs bg-black rounded p-4 h-64 overflow-y-auto">
  <!-- Server appends log lines here -->
</div>
```

Server streams log lines as they arrive:

```rust
async fn job_logs_stream(
    AxumPath(job_id): AxumPath<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let selector = format!("#job-logs-{job_id}");

    Sse::new(stream! {
        let mut log_rx = state.subscribe_to_job_logs(&job_id).await;

        while let Some(line) = log_rx.recv().await {
            let line_html = format!(
                r#"<div class="text-gray-300">{}</div>"#,
                escape_html(&line)
            );
            yield Ok::<Event, Infallible>(
                PatchElements::new(line_html)
                    .selector(&selector)
                    .mode(ElementPatchMode::Append)
                    .write_as_axum_sse_event()
            );
        }
    })
}
```

### Minimal Web Component for Status Badge

Datastar works alongside custom elements:

```javascript
// Define in templates::web_components()
class AssetStatusBadge extends HTMLElement {
  static observedAttributes = ['label', 'tone'];

  connectedCallback() { this._render(); }
  attributeChangedCallback() { this._render(); }

  _render() {
    const tone = this.getAttribute('tone') || 'neutral';
    const label = this.getAttribute('label') || '';
    const colors = {
      success: 'bg-green-500/20 text-green-400 ring-green-500/30',
      running: 'bg-blue-500/20 text-blue-400 ring-blue-500/30',
      failed:  'bg-red-500/20 text-red-400 ring-red-500/30',
      neutral: 'bg-gray-500/20 text-gray-400 ring-gray-500/30',
    };
    this.innerHTML = `<span class="inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium ring-1 ${colors[tone] ?? colors.neutral}">${label}</span>`;
  }
}
customElements.define('asset-status-badge', AssetStatusBadge);
```

Usage in Rust templates:

```rust
format!(r#"<asset-status-badge label="{}" tone="{}"></asset-status-badge>"#,
    escape_html(&status.label()),
    escape_html(&status.tone()),
)
```

---

## Quick Reference: Common Patterns

### Pattern → Code

| Goal | Code |
|---|---|
| Open panel | `data-on:click="$panelOpen = true; @get('/panel/stream')"` |
| Close panel | `data-on:click="$panelOpen = false"` |
| Show/hide | `data-show="$condition"` |
| Disable button while loading | `data-indicator:_load` `data-attr:disabled="$_load"` |
| Two-way input binding | `data-bind:field-name` |
| Navigate without reload | `data-on:click="@get('/view/foo')"` |
| Debounced search | `data-on:input__debounce.300ms="@get('/search')"` |
| Append to list | `.selector("#list").mode(ElementPatchMode::Append)` |
| Remove element | `PatchElements::new_remove("#elem-id")` |
| Update signal from server | `PatchSignals::new(r#"{"key": val}"#)` |
| Execute JS from server | `ExecuteScript::new("myFn()")` |
| Conditional action | `data-on:click="$ready && @post('/run')"` |
| Click outside to close | `data-on:click__outside="$open = false"` |
| Global keyboard shortcut | `data-on:keydown__window="evt.key==='Escape'&&($open=false)"` |
| Infinite scroll trigger | `data-on-intersect__once="@get('/load-more')"` |
| Poll for status | `data-on-interval__duration.5s="@get('/status')"` |
| Debug signals | `<pre data-json-signals></pre>` |

### SSE Event Quick Reference

```
# Patch element by ID (most common)
event: datastar-patch-elements
data: elements <div id="target">content</div>

# Patch inner HTML of a container
event: datastar-patch-elements
data: selector #container
data: mode inner
data: elements <div>new children</div>

# Append to a list
event: datastar-patch-elements
data: selector #list
data: mode append
data: elements <div id="item-99">new item</div>

# Remove an element
event: datastar-patch-elements
data: selector #item-99
data: mode remove
data: elements

# Update signals
event: datastar-patch-signals
data: signals {"count": 42, "status": "done"}

# Conditional signal update (only if missing)
event: datastar-patch-signals
data: onlyIfMissing true
data: signals {"defaultPage": 1}
```
