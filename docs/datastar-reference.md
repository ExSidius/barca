# Datastar Reference for Building a Dagster-like SPA

> Comprehensive reference for building a sophisticated, panel-heavy SPA with Rust (axum) backend,
> Datastar for reactivity, and Tailwind CSS for styling. Covers everything needed to build
> interactive dashboards, side panels, modals, forms, navigation, and real-time updates.

## Table of Contents

1. [Core Mental Model](#1-core-mental-model)
2. [SSE Protocol (Server → Browser)](#2-sse-protocol-server--browser)
3. [Signals & Reactive Store](#3-signals--reactive-store)
4. [All `data-*` Attribute Plugins](#4-all-data--attribute-plugins)
5. [Action Plugins](#5-action-plugins)
6. [SPA Navigation Patterns](#6-spa-navigation-patterns)
7. [UI Component Patterns](#7-ui-component-patterns)
8. [Advanced Patterns & Recipes](#8-advanced-patterns--recipes)
9. [Reimplementing SPA Plugins](#9-reimplementing-spa-plugins)
10. [Rust/Axum Backend Patterns](#10-rustaxum-backend-patterns)
11. [Gotchas & Constraints](#11-gotchas--constraints)

---

## 1. Core Mental Model

Datastar is a ~11KB hypermedia framework that combines:
- **Frontend reactivity** (like Alpine.js) — signals, computed values, effects, all declared via `data-*` attributes
- **Server-driven updates** (like htmx) — SSE-based DOM patching, the server is the source of truth

**Key principle: HOWL (Hypermedia On Whatever you Like).** The server sends HTML fragments + signal patches over SSE. The browser morphs them into the DOM. No client-side router, no JSON APIs, no virtual DOM.

**Three layers:**
1. **Browser** — `datastar.js` processes `data-*` attributes, manages reactive signals, handles SSE events
2. **Server** — Emits SSE events (`datastar-patch-elements`, `datastar-patch-signals`; scripts via `<script>` tags in patch-elements)
3. **Plugins** — Attribute plugins (`data-*`), Action plugins (`@action()`), Watcher plugins (SSE event handlers)

**Request-response cycle:**
1. User clicks `<button data-on:click="@post('/handler')">`
2. Browser serializes current signals as JSON, sends HTTP request
3. Server reads signals, processes request
4. Server streams SSE events: HTML fragments to morph, signal patches to update state
5. Browser morphs DOM, updates signals, reactive effects re-fire automatically

---

## 2. SSE Protocol (Server → Browser)

### General SSE Format

```
event: EVENT_TYPE
data: DATALINE_KEY VALUE
data: DATALINE_KEY VALUE

```
Each event terminated by a blank line (`\n\n`).

### Event Type 1: `datastar-patch-elements`

Sends HTML fragments for DOM manipulation.

| Field | Required | Default | Description |
|---|---|---|---|
| `selector` | No | Self-targeting by element ID | CSS selector for target element(s) |
| `mode` | No | `outer` | How to apply the fragment |
| `elements` | Yes (except `remove`) | — | HTML content (multi-line: repeat `data: elements` per line) |
| `useViewTransition` | No | `false` | Enable View Transitions API for smooth animation |
| `settle` | No | — | Post-patch settlement duration (ms) |
| `namespace` | No | `html` | Element namespace: `html`, `svg`, or `mathml` |

**All merge modes:**

| Mode | Behavior | Preserves State? |
|---|---|---|
| `outer` | **Default.** Morphs entire element (idiomorph diffing) | Yes — focus, form values, scroll, listeners |
| `inner` | Morphs innerHTML only | Yes for outer element |
| `replace` | Hard replace, no morphing | No — resets everything |
| `prepend` | Insert at beginning inside target | Yes for existing content |
| `append` | Insert at end inside target | Yes for existing content |
| `before` | Insert as preceding sibling | Yes for existing content |
| `after` | Insert as following sibling | Yes for existing content |
| `remove` | Delete target element(s) from DOM | N/A |

**Examples:**

```
event: datastar-patch-elements
data: selector #main-content
data: mode outer
data: elements <div id="main-content">
data: elements   <h1>Dashboard</h1>
data: elements   <p>Welcome back</p>
data: elements </div>

```

```
event: datastar-patch-elements
data: mode append
data: selector #log-entries
data: elements <div class="log-entry">New log line</div>

```

```
event: datastar-patch-elements
data: selector #panel
data: mode remove

```

```
event: datastar-patch-elements
data: selector #chart
data: mode outer
data: useViewTransition true
data: elements <div id="chart"><canvas></canvas></div>

```

### Event Type 2: `datastar-patch-signals`

Sends JSON Merge Patch (RFC 7386) to update the reactive store.

| Field | Required | Default | Description |
|---|---|---|---|
| `signals` | Yes | — | JSON object (RFC 7386 merge patch) |
| `onlyIfMissing` | No | `false` | Only set signals that don't already exist |

**RFC 7386 semantics:** Properties merge recursively. `null` values **delete** properties.

```
event: datastar-patch-signals
data: signals {"count": 42, "activePanel": "details", "loading": false}

```

```
event: datastar-patch-signals
data: onlyIfMissing true
data: signals {"theme": "dark", "sidebarOpen": true}

```

### Script Execution (no dedicated SSE event type)

Scripts are executed by including `<script>` tags inside a `datastar-patch-elements` event.
There is **no** `datastar-execute-script` event — use `append` mode to `body`:

```
event: datastar-patch-elements
data: mode append
data: selector body
data: elements <script>console.log('Job completed')</script>

```

Or embed a `<script>` inside a patched element (scripts inside patched elements are re-executed):

```
event: datastar-patch-elements
data: elements <div id="hal">
data: elements   <script>alert('Done')</script>
data: elements </div>

```

For `text/javascript` responses (non-SSE), the body is executed directly as JavaScript.

### Non-SSE Response Shortcuts

The fetch action auto-detects content types for simpler responses:

| Content-Type | Auto-dispatched As |
|---|---|
| `text/event-stream` | Standard SSE stream (default) |
| `text/html` | Single `datastar-patch-elements` event |
| `application/json` | Single `datastar-patch-signals` event |
| `text/javascript` | Script execution |

For `text/html`, use response headers to configure: `datastar-selector`, `datastar-mode`, `datastar-use-view-transition`.

---

## 3. Signals & Reactive Store

### The Global Store

All application state lives in a global `root` object — a deeply reactive proxy. Every property at any nesting level is automatically a signal.

```html
<!-- Declare signals -->
<div data-signals='{"count": 0, "user": {"name": "Alice"}, "panelOpen": false}'></div>

<!-- Access signals with $ prefix -->
<span data-text="$count"></span>              <!-- root.count -->
<span data-text="$user.name"></span>          <!-- root.user.name -->
<span data-text="$items[0]"></span>           <!-- root.items[0] -->
```

### Reactive Primitives

| Primitive | Purpose |
|---|---|
| Signal | Mutable reactive state (created by `data-signals`) |
| Computed | Derived read-only values (created by `data-computed`) |
| Effect | Side effects that re-run when dependencies change (created by `data-effect`) |

### Signal Naming Conventions

- **Underscore prefix** (`_foo`) — "private" signals, NOT sent to backend by default
- **Kebab-case** (`foo-bar`) — converted to camelCase internally (`fooBar`)
- Signals are batched — multiple updates in one expression fire effects only once

---

## 4. All `data-*` Attribute Plugins

### State & Binding

#### `data-signals` — Initialize reactive signals
```html
<!-- JSON object form -->
<div data-signals='{"count": 0, "name": "hello", "items": []}'></div>

<!-- Key form (single signal) -->
<div data-signals:count="0"></div>
<div data-signals:name="'hello'"></div>
```

#### `data-computed` — Derived values
```html
<div data-computed:fullName="$firstName + ' ' + $lastName"></div>
<div data-computed:total="$items.length"></div>
<div data-computed:isValid="$email.includes('@') && $password.length >= 8"></div>
```

#### `data-bind` — Two-way binding for form controls
```html
<input data-bind:value="$username" />
<input type="checkbox" data-bind:checked="$agreed" />
<select data-bind:value="$selectedTab">
  <option value="overview">Overview</option>
  <option value="runs">Runs</option>
</select>
<textarea data-bind:value="$description"></textarea>
```

#### `data-ref` — Element references
```html
<div data-ref:myPanel></div>
<!-- Access via $myPanel in expressions -->
```

### Display & Rendering

#### `data-text` — Reactive text content
```html
<span data-text="$count"></span>
<span data-text="`${$firstName} ${$lastName}`"></span>
<span data-text="$loading ? 'Loading...' : $result"></span>
```

#### `data-show` — Conditional visibility
```html
<div data-show="$panelOpen">Panel content</div>
<div data-show="$activeTab === 'runs'">Runs list</div>
<div data-show="$errors.length > 0">Error messages</div>
```

#### `data-class` — Dynamic CSS classes
```html
<!-- Keyed mode (most common) -->
<div data-class:hidden="!$show"></div>
<div data-class:bg-blue-500="$isActive"></div>
<div data-class:opacity-50="$isLoading"></div>
<div data-class:ring-2="$isSelected"></div>
<div data-class:translate-x-0="$sidebarOpen"></div>
<div data-class:-translate-x-full="!$sidebarOpen"></div>

<!-- Multiple classes with object form -->
<div data-class="{'font-bold': $isActive, 'text-gray-400': !$isActive}"></div>
```

#### `data-attr` — Dynamic HTML attributes
```html
<button data-attr:disabled="$isLoading"></button>
<a data-attr:href="$linkUrl">Link</a>
<div data-attr:aria-expanded="$panelOpen"></div>
<div data-attr:aria-label="$statusText"></div>
<input data-attr:placeholder="$placeholderText" />
```

#### `data-style` — Dynamic inline styles
```html
<div data-style:color="$textColor"></div>
<div data-style:display="$visible ? 'block' : 'none'"></div>
<div data-style:width="`${$progress}%`"></div>
<div data-style:transform="`translateX(${$offset}px)`"></div>
```

### Event Handling

#### `data-on` — Event listeners

**Syntax:** `data-on:eventname[__modifier1[__modifier2[__sub]]]="expression"`

```html
<!-- Click -->
<button data-on:click="@post('/api/save')">Save</button>
<button data-on:click="$count++">Increment</button>
<button data-on:click="$panelOpen = !$panelOpen">Toggle Panel</button>

<!-- Multiple statements -->
<button data-on:click="$loading = true; @post('/api/run')">Run</button>

<!-- Conditional action -->
<button data-on:click="$isValid && @post('/api/submit')">Submit</button>

<!-- Form submission -->
<form data-on:submit="@post('/api/create')">...</form>

<!-- Input events -->
<input data-on:input="$search = evt.target.value" />
<input data-on:keydown="evt.key === 'Enter' && @post('/api/search')" />

<!-- Window-level events -->
<div data-on:keydown__window="evt.key === 'Escape' && ($panelOpen = false)"></div>
```

**All modifiers:**

| Modifier | Effect |
|---|---|
| `__debounce:500ms` | Delays firing by 500ms (resets on new events). Sub-modifiers: `__leading` (fire on leading edge), `__notrailing` (suppress trailing). |
| `__throttle:300ms` | Limits to once per 300ms. Sub-modifiers: `__noleading` (suppress leading), `__trailing` (fire trailing). |
| `__once` | Fire only once, then remove listener |
| `__prevent` | `event.preventDefault()` |
| `__stop` | `event.stopPropagation()` |
| `__capture` | Use capture phase |
| `__passive` | Mark as passive |
| `__outside` | Fire only for events outside this element |
| `__window` | Listen on `window` instead of element |
| `__delay:200ms` | Delay callback execution by 200ms |
| `__leading` | Fire on leading edge (with debounce/throttle) |
| `__notrailing` | Don't fire on trailing edge |
| `__viewtransition` | Wrap callback in `document.startViewTransition()` |

```html
<!-- Debounced search -->
<input data-on:input__debounce:300ms="@get(`/search?q=${$query}`)" />

<!-- Click outside to close -->
<div data-on:click__outside="$menuOpen = false">Dropdown</div>

<!-- One-time init -->
<div data-on:click__once="$initialized = true">Init</div>

<!-- Escape key (window) -->
<div data-on:keydown__window="evt.key === 'Escape' && ($modalOpen = false)"></div>

<!-- View transition on nav -->
<button data-on:click__viewtransition="@get('/next-page')">Navigate</button>
```

### Lifecycle & DOM

#### `data-init` — Run on element load
```html
<div data-init="@get('/api/initial-data')"></div>
<div data-init__delay:500ms="@get('/api/deferred-content')"></div>
```

#### `data-effect` — Reactive side effects
```html
<div data-effect="console.log('Count is now:', $count)"></div>
<div data-effect="document.title = `(${$notifications}) Dashboard`"></div>
```

#### `data-on-intersect` — Viewport intersection (lazy loading, infinite scroll)
```html
<div data-on-intersect="@get('/api/load-more')"></div>
<div data-on-intersect__once="@get('/api/lazy-section')"></div>
```

#### `data-on-interval` — Periodic execution
```html
<!-- Poll every 5 seconds -->
<div data-on-interval__duration:5s="@get('/api/status')"></div>
<!-- Default interval: 1000ms -->
<div data-on-interval="@get('/api/status')"></div>
<!-- With leading (also runs immediately on init) -->
<div data-on-interval__duration:5s__leading="@get('/api/status')"></div>
```

#### `data-indicator` — Track fetch request status
```html
<button
  data-on:click="@post('/api/run')"
  data-indicator:fetching
  data-attr:disabled="$fetching"
>
  <span data-show="!$fetching">Run</span>
  <span data-show="$fetching">Running...</span>
</button>
```

#### `data-ignore` — Skip Datastar processing
```html
<div data-ignore>
  <!-- Content here won't be processed by Datastar -->
</div>
```

#### `data-ignore-morph` — Preserve during DOM patching
```html
<div data-ignore-morph>
  <!-- This subtree won't be morphed during SSE patches -->
</div>
```

#### `data-preserve-attr` — Preserve specific attributes during morph
```html
<div data-preserve-attr="class,style">
  <!-- class and style attributes survive morphing -->
</div>
```

---

## 5. Action Plugins

### HTTP Actions

All follow `@method(url, options?)`. Response must be SSE (`text/event-stream`), HTML, or JSON.

| Action | HTTP Method |
|---|---|
| `@get(url, opts?)` | GET |
| `@post(url, opts?)` | POST |
| `@put(url, opts?)` | PUT |
| `@patch(url, opts?)` | PATCH |
| `@delete(url, opts?)` | DELETE |

**How signals travel:**
- **GET**: Encoded as `?datastar={"signal":"value"}` query parameter
- **POST/PUT/PATCH/DELETE**: Sent in request body as JSON

**All requests include** the header `Datastar-Request: true`.

**Options object:**

```html
<button data-on:click="@post('/api', {
  contentType: 'json',                         // 'json' (default) or 'form'
  headers: {'X-Custom': 'value'},              // Custom headers
  filterSignals: {include: /^(name|email)$/},  // Which signals to send
  requestCancellation: 'auto',                 // Cancel previous in-flight request
  retryMaxCount: 3,                            // Retry attempts
  retryInterval: 1000,                         // Initial retry delay ms
  retryScaler: 2,                              // Exponential backoff multiplier
  retryMaxWaitMs: 30000                        // Max retry delay ms
})">Submit</button>
```

**Lifecycle events** dispatched on the element:

| Detail Type | When |
|---|---|
| `started` | Request initiated |
| `finished` | Request completed |
| `error` | HTTP error response |
| `retrying` | Retry attempt |
| `retries-failed` | Max retries exceeded |

### Utility Actions

| Action | Purpose | Example |
|---|---|---|
| `@setAll(value, filter?)` | Set all matching signals | `@setAll(false, {include: /^selected/})` |
| `@toggleAll(filter?)` | Toggle matching booleans | `@toggleAll({include: /^checked/})` |
| `@clipboard(text)` | Copy to clipboard | `@clipboard($shareUrl)` |
| `@peek(fn)` | Read signal without subscribing | `@peek(() => $count)` |

---

## 6. SPA Navigation Patterns

### Pattern: Server-Driven Page Navigation

No client-side router needed. The server drives navigation via SSE patches + URL replacement.

```html
<!-- Navigation bar -->
<nav>
  <button data-on:click__viewtransition="@get('/page/overview')">Overview</button>
  <button data-on:click__viewtransition="@get('/page/runs')">Runs</button>
  <button data-on:click__viewtransition="@get('/page/assets')">Assets</button>
</nav>

<!-- Main content area (server replaces this) -->
<main id="main-content">
  <!-- Server sends new content here via datastar-patch-elements -->
</main>
```

**Server response for navigation:**
```
event: datastar-patch-signals
data: signals {"activePage": "runs"}

event: datastar-patch-elements
data: selector #main-content
data: mode inner
data: useViewTransition true
data: elements <div class="p-6">
data: elements   <h1 class="text-2xl font-bold">Runs</h1>
data: elements   <div id="runs-list">...</div>
data: elements </div>

```

### Pattern: Active Navigation Highlighting

```html
<nav class="flex gap-2">
  <button
    data-on:click="@get('/page/overview')"
    data-class:bg-blue-600="$activePage === 'overview'"
    data-class:bg-gray-700="$activePage !== 'overview'"
  >Overview</button>
  <button
    data-on:click="@get('/page/runs')"
    data-class:bg-blue-600="$activePage === 'runs'"
    data-class:bg-gray-700="$activePage !== 'runs'"
  >Runs</button>
</nav>
```

### Pattern: Tab Navigation within a Panel

```html
<div class="border-b border-gray-700 flex">
  <button
    data-on:click="@get(`/assets/${$assetId}/tab/overview`)"
    data-class:border-blue-500="$activeTab === 'overview'"
    data-class:border-transparent="$activeTab !== 'overview'"
    class="px-4 py-2 border-b-2"
  >Overview</button>
  <button
    data-on:click="@get(`/assets/${$assetId}/tab/runs`)"
    data-class:border-blue-500="$activeTab === 'runs'"
    data-class:border-transparent="$activeTab !== 'runs'"
    class="px-4 py-2 border-b-2"
  >Runs</button>
</div>
<div id="tab-content">
  <!-- Server patches this -->
</div>
```

---

## 7. UI Component Patterns

### Side Panel (Slide-in Drawer)

```html
<!-- Overlay backdrop -->
<div
  data-show="$panelOpen"
  data-on:click="$panelOpen = false"
  class="fixed inset-0 bg-black/50 z-40 transition-opacity"
  data-class:opacity-0="!$panelOpen"
  data-class:opacity-100="$panelOpen"
></div>

<!-- Panel -->
<div
  class="fixed top-0 right-0 h-full w-[600px] bg-gray-900 shadow-xl z-50 transform transition-transform duration-300"
  data-class:translate-x-0="$panelOpen"
  data-class:translate-x-full="!$panelOpen"
>
  <div class="flex items-center justify-between p-4 border-b border-gray-700">
    <h2 id="panel-title" class="text-lg font-semibold" data-text="$panelTitle"></h2>
    <button data-on:click="$panelOpen = false" class="text-gray-400 hover:text-white">
      &times;
    </button>
  </div>
  <div id="panel-content" class="p-4 overflow-y-auto h-full">
    <!-- Server patches this -->
  </div>
</div>

<!-- Escape to close -->
<div data-on:keydown__window="evt.key === 'Escape' && ($panelOpen = false)"></div>
```

**Opening the panel from a button:**
```html
<button data-on:click="$panelOpen = true; $panelTitle = 'Asset Details'; @get(`/panel/asset/${assetId}`)">
  View Details
</button>
```

### Modal Dialog

```html
<!-- Modal backdrop -->
<div
  data-show="$modalOpen"
  class="fixed inset-0 bg-black/60 z-50 flex items-center justify-center"
  data-on:click="$modalOpen = false"
>
  <!-- Modal content (stop propagation so clicking inside doesn't close) -->
  <div
    data-on:click__stop=""
    class="bg-gray-800 rounded-lg shadow-2xl w-full max-w-lg mx-4 overflow-hidden"
  >
    <div class="px-6 py-4 border-b border-gray-700 flex justify-between items-center">
      <h3 class="text-lg font-semibold" data-text="$modalTitle"></h3>
      <button data-on:click="$modalOpen = false" class="text-gray-400 hover:text-white">&times;</button>
    </div>
    <div id="modal-body" class="px-6 py-4">
      <!-- Server patches this -->
    </div>
    <div class="px-6 py-4 border-t border-gray-700 flex justify-end gap-3">
      <button data-on:click="$modalOpen = false" class="px-4 py-2 bg-gray-700 rounded hover:bg-gray-600">
        Cancel
      </button>
      <button
        id="modal-confirm-btn"
        data-on:click="@post($modalAction)"
        data-indicator:modalLoading
        data-attr:disabled="$modalLoading"
        class="px-4 py-2 bg-blue-600 rounded hover:bg-blue-500"
      >
        <span data-show="!$modalLoading">Confirm</span>
        <span data-show="$modalLoading">Working...</span>
      </button>
    </div>
  </div>
</div>
```

### Dropdown Menu

```html
<div class="relative">
  <button
    data-on:click="$dropdownOpen = !$dropdownOpen"
    class="px-3 py-2 bg-gray-700 rounded hover:bg-gray-600"
  >
    Actions &#9662;
  </button>
  <div
    data-show="$dropdownOpen"
    data-on:click__outside="$dropdownOpen = false"
    class="absolute right-0 mt-1 w-48 bg-gray-800 border border-gray-700 rounded-lg shadow-xl z-30"
  >
    <button data-on:click="$dropdownOpen = false; @post('/api/run')"
      class="w-full text-left px-4 py-2 hover:bg-gray-700">
      Run
    </button>
    <button data-on:click="$dropdownOpen = false; @post('/api/cancel')"
      class="w-full text-left px-4 py-2 hover:bg-gray-700 text-red-400">
      Cancel
    </button>
  </div>
</div>
```

### Toast / Notification

```html
<!-- Toast container -->
<div id="toast-container" class="fixed bottom-4 right-4 z-50 flex flex-col gap-2">
  <!-- Server appends toasts here -->
</div>
```

**Server sends:**
```
event: datastar-patch-elements
data: mode append
data: selector #toast-container
data: elements <div id="toast-123" class="px-4 py-3 bg-green-800 text-white rounded-lg shadow-lg flex items-center gap-3" data-init__delay:4000ms="@delete('/toast/123')">
data: elements   <span>Asset materialized successfully</span>
data: elements   <button data-on:click="@delete('/toast/123')" class="text-green-300 hover:text-white">&times;</button>
data: elements </div>

```

**To dismiss (server responds to DELETE `/toast/123`):**
```
event: datastar-patch-elements
data: selector #toast-123
data: mode remove

```

### Loading Button with Indicator

```html
<button
  data-on:click="@post('/api/materialize')"
  data-indicator:isMaterializing
  data-attr:disabled="$isMaterializing"
  class="inline-flex items-center gap-2 px-4 py-2 bg-blue-600 rounded-lg hover:bg-blue-500 disabled:opacity-50 disabled:cursor-not-allowed"
>
  <!-- Spinner (shown during loading) -->
  <svg data-show="$isMaterializing" class="animate-spin h-4 w-4" viewBox="0 0 24 24">
    <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" fill="none"/>
    <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"/>
  </svg>
  <span data-show="!$isMaterializing">Materialize</span>
  <span data-show="$isMaterializing">Materializing...</span>
</button>
```

### Data Table with Selection

```html
<div data-signals='{"selectedRows": [], "selectAll": false}'>
  <table class="w-full">
    <thead>
      <tr class="border-b border-gray-700">
        <th class="p-3">
          <input type="checkbox" data-bind:checked="$selectAll"
            data-on:change="@post('/api/select-all')" />
        </th>
        <th class="p-3 text-left">Name</th>
        <th class="p-3 text-left">Status</th>
        <th class="p-3 text-left">Last Run</th>
      </tr>
    </thead>
    <tbody id="table-body">
      <!-- Server patches rows here -->
    </tbody>
  </table>
</div>
```

### Search with Debounced Input

```html
<div data-signals='{"searchQuery": ""}'>
  <div class="relative">
    <input
      type="text"
      data-bind:value="$searchQuery"
      data-on:input__debounce:300ms="@get('/api/search')"
      placeholder="Search assets..."
      class="w-full px-4 py-2 pl-10 bg-gray-800 border border-gray-700 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-transparent"
    />
    <svg class="absolute left-3 top-2.5 h-5 w-5 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/>
    </svg>
  </div>
  <div id="search-results" class="mt-4">
    <!-- Server patches results here -->
  </div>
</div>
```

### Collapsible Section / Accordion

```html
<div data-signals='{"section1Open": true, "section2Open": false}'>
  <!-- Section 1 -->
  <div class="border border-gray-700 rounded-lg mb-2">
    <button
      data-on:click="$section1Open = !$section1Open"
      class="w-full flex justify-between items-center p-4 hover:bg-gray-800"
    >
      <span class="font-medium">Configuration</span>
      <svg class="h-5 w-5 transition-transform" data-class:rotate-180="$section1Open" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"/>
      </svg>
    </button>
    <div data-show="$section1Open" class="p-4 border-t border-gray-700">
      Content here
    </div>
  </div>
</div>
```

### Infinite Scroll

```html
<div id="items-list">
  <!-- Existing items -->
</div>
<div
  data-on-intersect__once="@get('/api/items?page=2')"
  class="flex justify-center py-4"
>
  <span class="text-gray-400">Loading more...</span>
</div>
```

**Server response appends items and a new sentinel:**
```
event: datastar-patch-elements
data: mode append
data: selector #items-list
data: elements <div class="p-4 border-b border-gray-700">Item 11</div>
data: elements <div class="p-4 border-b border-gray-700">Item 12</div>

event: datastar-patch-elements
data: mode after
data: selector #items-list
data: elements <div data-on-intersect__once="@get('/api/items?page=3')" class="flex justify-center py-4">
data: elements   <span class="text-gray-400">Loading more...</span>
data: elements </div>

```

### Resizable Split Panes

```html
<div class="flex h-screen" data-signals='{"leftWidth": 300}'>
  <!-- Left pane -->
  <div data-style:width="`${$leftWidth}px`" class="flex-shrink-0 overflow-auto border-r border-gray-700">
    <div id="left-pane">Left content</div>
  </div>
  <!-- Drag handle -->
  <div
    class="w-1 bg-gray-700 hover:bg-blue-500 cursor-col-resize flex-shrink-0"
    data-on:mousedown="
      const startX = evt.clientX;
      const startW = $leftWidth;
      const onMove = (e) => { $leftWidth = Math.max(200, Math.min(600, startW + e.clientX - startX)); };
      const onUp = () => { window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp); };
      window.addEventListener('mousemove', onMove);
      window.addEventListener('mouseup', onUp);
    "
  ></div>
  <!-- Right pane -->
  <div class="flex-1 overflow-auto">
    <div id="right-pane">Right content</div>
  </div>
</div>
```

---

## 8. Advanced Patterns & Recipes

### Command Palette (Cmd+K)

A searchable command palette like Dagster's or VS Code's:

```html
<div data-signals='{"_cmdOpen": false, "_cmdQuery": "", "_cmdResults": []}'>
  <!-- Trigger: Cmd+K / Ctrl+K -->
  <div data-on:keydown__window="(evt.metaKey || evt.ctrlKey) && evt.key === 'k' && (evt.preventDefault(), $_cmdOpen = !$_cmdOpen)"></div>

  <!-- Palette overlay -->
  <div data-show="$_cmdOpen" class="fixed inset-0 z-50 flex items-start justify-center pt-[20vh] bg-black/60"
       data-on:click="$_cmdOpen = false"
       data-on:keydown__window="evt.key === 'Escape' && ($_cmdOpen = false)">
    <div data-on:click__stop="" class="w-full max-w-xl bg-gray-900 rounded-xl shadow-2xl border border-gray-700 overflow-hidden">
      <!-- Search input -->
      <div class="flex items-center px-4 border-b border-gray-700">
        <svg class="h-5 w-5 text-gray-400 shrink-0" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/>
        </svg>
        <input
          type="text"
          data-bind:value="$_cmdQuery"
          data-on:input__debounce:150ms="@get('/api/command-search')"
          placeholder="Search assets, runs, commands..."
          class="w-full px-3 py-4 bg-transparent text-white placeholder-gray-500 focus:outline-none"
          autofocus
        />
      </div>
      <!-- Results -->
      <div id="cmd-results" class="max-h-80 overflow-y-auto">
        <!-- Server patches results here -->
      </div>
      <!-- Footer hint -->
      <div class="px-4 py-2 border-t border-gray-700 flex gap-4 text-xs text-gray-500">
        <span><kbd class="px-1.5 py-0.5 bg-gray-800 rounded text-gray-400">Enter</kbd> to select</span>
        <span><kbd class="px-1.5 py-0.5 bg-gray-800 rounded text-gray-400">Esc</kbd> to close</span>
      </div>
    </div>
  </div>
</div>
```

**Server response for search results:**
```rust
// Each result item navigates or triggers an action
fn render_cmd_result(name: &str, kind: &str, action_url: &str) -> String {
    format!(
        r#"<button
          data-on:click="$_cmdOpen = false; @get('{action_url}')"
          class="w-full flex items-center gap-3 px-4 py-3 hover:bg-gray-800 text-left"
        >
          <span class="text-xs font-medium uppercase tracking-wider text-gray-500 w-16">{kind}</span>
          <span class="text-sm text-gray-200">{name}</span>
        </button>"#
    )
}
```

### Breadcrumb Trail (Server-Driven)

```html
<nav id="breadcrumb" class="flex items-center gap-1.5 text-sm text-gray-500" aria-label="Breadcrumb">
  <!-- Server replaces entire breadcrumb on navigation -->
</nav>
```

**Server sends breadcrumb + content together on nav:**
```rust
// In a navigation handler, emit two patches:
// 1) breadcrumb
yield Ok(PatchElements::new(
    r#"<nav id="breadcrumb" class="flex items-center gap-1.5 text-sm text-gray-500">
        <a data-on:click="@get('/page/assets')" class="hover:text-gray-300 cursor-pointer">Assets</a>
        <span class="text-gray-600">/</span>
        <span class="text-gray-300">my_asset</span>
    </nav>"#
).write_as_axum_sse_event());

// 2) main content
yield Ok(PatchElements::new(content_html)
    .selector("#main-content")
    .write_as_axum_sse_event());
```

### Skeleton Loading States

Show content placeholders while server fetches data:

```html
<!-- Skeleton card (rendered immediately in HTML, replaced by server) -->
<div id="asset-list" class="space-y-4">
  <div class="animate-pulse rounded-xl border border-gray-200 dark:border-gray-800 p-5">
    <div class="flex justify-between">
      <div class="space-y-3 flex-1">
        <div class="h-3 w-24 bg-gray-200 dark:bg-gray-700 rounded"></div>
        <div class="h-6 w-48 bg-gray-200 dark:bg-gray-700 rounded"></div>
        <div class="h-4 w-32 bg-gray-200 dark:bg-gray-700 rounded"></div>
      </div>
      <div class="h-8 w-20 bg-gray-200 dark:bg-gray-700 rounded-full"></div>
    </div>
  </div>
  <div class="animate-pulse rounded-xl border border-gray-200 dark:border-gray-800 p-5">
    <div class="flex justify-between">
      <div class="space-y-3 flex-1">
        <div class="h-3 w-24 bg-gray-200 dark:bg-gray-700 rounded"></div>
        <div class="h-6 w-48 bg-gray-200 dark:bg-gray-700 rounded"></div>
        <div class="h-4 w-32 bg-gray-200 dark:bg-gray-700 rounded"></div>
      </div>
      <div class="h-8 w-20 bg-gray-200 dark:bg-gray-700 rounded-full"></div>
    </div>
  </div>
</div>

<!-- On load, fetch real content which replaces the skeletons -->
<div data-init="@get('/api/assets')"></div>
```

**Rust helper for skeleton generation:**
```rust
pub fn skeleton_card() -> &'static str {
    r#"<div class="animate-pulse rounded-xl border border-gray-200 dark:border-gray-800 p-5">
      <div class="flex justify-between">
        <div class="space-y-3 flex-1">
          <div class="h-3 w-24 bg-gray-200 dark:bg-gray-700 rounded"></div>
          <div class="h-6 w-48 bg-gray-200 dark:bg-gray-700 rounded"></div>
          <div class="h-4 w-32 bg-gray-200 dark:bg-gray-700 rounded"></div>
        </div>
        <div class="h-8 w-20 bg-gray-200 dark:bg-gray-700 rounded-full"></div>
      </div>
    </div>"#
}
```

### Progress Bar (Streaming Updates)

For long-running tasks like asset materialization:

```html
<div id="progress-bar" data-signals='{"_progress": 0, "_progressLabel": ""}'>
  <div class="w-full bg-gray-800 rounded-full h-2 overflow-hidden">
    <div
      class="bg-blue-500 h-2 rounded-full transition-all duration-300"
      data-style:width="`${$_progress}%`"
    ></div>
  </div>
  <p class="mt-1 text-xs text-gray-400" data-text="$_progressLabel"></p>
</div>
```

**Server streams progress updates:**
```rust
for step in 0..=100 {
    yield Ok(PatchSignals::new(
        &format!(r#"{{"_progress": {step}, "_progressLabel": "Processing step {step}/100..."}}"#)
    ).write_as_axum_sse_event());
    tokio::time::sleep(Duration::from_millis(50)).await;
}
// Final: replace progress bar with result
yield Ok(PatchElements::new(r#"<div id="progress-bar"><p class="text-sm text-emerald-400">Complete!</p></div>"#)
    .write_as_axum_sse_event());
```

### Tooltip

Pure CSS + Tailwind, no JS needed:

```html
<div class="relative group inline-block">
  <button class="text-gray-400 hover:text-gray-200">
    <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/>
    </svg>
  </button>
  <div class="absolute bottom-full left-1/2 -translate-x-1/2 mb-2 px-3 py-1.5 bg-gray-900 text-xs text-gray-200 rounded-lg shadow-lg whitespace-nowrap opacity-0 invisible group-hover:opacity-100 group-hover:visible transition-all duration-150 pointer-events-none">
    Definition hash uniquely identifies the asset code
    <div class="absolute top-full left-1/2 -translate-x-1/2 border-4 border-transparent border-t-gray-900"></div>
  </div>
</div>
```

### Popover (Click-Triggered, Data-Driven)

```html
<div class="relative" data-signals='{"_popOpen": false}'>
  <button
    data-on:click="$_popOpen = !$_popOpen"
    class="inline-flex items-center gap-1 text-sm text-blue-400 hover:text-blue-300"
  >
    Details
    <svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"/>
    </svg>
  </button>
  <div
    data-show="$_popOpen"
    data-on:click__outside="$_popOpen = false"
    class="absolute left-0 mt-2 w-72 bg-gray-800 border border-gray-700 rounded-xl shadow-xl z-20 p-4"
  >
    <div class="text-sm text-gray-300 space-y-2">
      <p><span class="text-gray-500">Module:</span> example_project.assets</p>
      <p><span class="text-gray-500">File:</span> assets.py:12</p>
      <p><span class="text-gray-500">Hash:</span> <code class="text-xs bg-gray-900 px-1 rounded">abc123</code></p>
    </div>
  </div>
</div>
```

### Badge / Chip Components

```html
<!-- Status badges -->
<span class="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium
  bg-emerald-50 dark:bg-emerald-950/30 text-emerald-700 dark:text-emerald-400
  ring-1 ring-inset ring-emerald-600/20 dark:ring-emerald-500/30">
  <span class="h-1.5 w-1.5 rounded-full bg-emerald-500"></span>
  Fresh
</span>

<span class="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium
  bg-amber-50 dark:bg-amber-950/30 text-amber-700 dark:text-amber-400
  ring-1 ring-inset ring-amber-600/20 dark:ring-amber-500/30">
  <span class="h-1.5 w-1.5 rounded-full bg-amber-500 animate-pulse"></span>
  Running
</span>

<span class="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium
  bg-rose-50 dark:bg-rose-950/30 text-rose-700 dark:text-rose-400
  ring-1 ring-inset ring-rose-600/20 dark:ring-rose-500/30">
  <span class="h-1.5 w-1.5 rounded-full bg-rose-500"></span>
  Failed
</span>

<!-- Tag/chip with remove -->
<span class="inline-flex items-center gap-1 rounded-md bg-gray-100 dark:bg-gray-800 px-2 py-1 text-xs text-gray-600 dark:text-gray-400">
  python
  <button data-on:click="@delete('/api/tags/python')" class="hover:text-gray-900 dark:hover:text-white">
    <svg class="h-3 w-3" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M6 18L18 6M6 6l12 12"/></svg>
  </button>
</span>
```

### Empty States

```html
<!-- No data -->
<div class="flex flex-col items-center justify-center py-16 text-center">
  <svg class="h-12 w-12 text-gray-400 dark:text-gray-600" fill="none" viewBox="0 0 24 24" stroke="currentColor">
    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="1.5" d="M20 13V6a2 2 0 00-2-2H6a2 2 0 00-2 2v7m16 0v5a2 2 0 01-2 2H6a2 2 0 01-2-2v-5m16 0h-2.586a1 1 0 00-.707.293l-2.414 2.414a1 1 0 01-.707.293h-3.172a1 1 0 01-.707-.293l-2.414-2.414A1 1 0 006.586 13H4"/>
  </svg>
  <h3 class="mt-4 text-lg font-medium text-gray-900 dark:text-white">No runs yet</h3>
  <p class="mt-2 text-sm text-gray-500 dark:text-gray-400 max-w-sm">
    Materialize an asset to see its run history here.
  </p>
  <button
    data-on:click="@get('/page/assets')"
    class="mt-6 inline-flex items-center gap-2 rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-500"
  >
    View Assets
  </button>
</div>
```

### Confirmation Dialog (Inline)

Instead of a full modal, an inline "are you sure?" pattern:

```html
<div data-signals='{"_confirmDelete": false}'>
  <!-- Normal state -->
  <button
    data-show="!$_confirmDelete"
    data-on:click="$_confirmDelete = true"
    class="text-sm text-red-400 hover:text-red-300"
  >
    Delete
  </button>

  <!-- Confirmation state -->
  <div data-show="$_confirmDelete" class="inline-flex items-center gap-2">
    <span class="text-sm text-gray-400">Are you sure?</span>
    <button
      data-on:click="$_confirmDelete = false; @delete('/api/asset/123')"
      class="text-sm font-medium text-red-400 hover:text-red-300"
    >
      Yes, delete
    </button>
    <button
      data-on:click="$_confirmDelete = false"
      class="text-sm text-gray-500 hover:text-gray-300"
    >
      Cancel
    </button>
  </div>
</div>
```

### Live Log Viewer (Streaming Append)

```html
<div class="bg-gray-950 rounded-xl border border-gray-800 overflow-hidden">
  <div class="flex items-center justify-between px-4 py-2 border-b border-gray-800 bg-gray-900">
    <span class="text-xs font-medium text-gray-400 uppercase tracking-wider">Output</span>
    <div class="flex items-center gap-2">
      <span class="h-2 w-2 rounded-full bg-emerald-500 animate-pulse"></span>
      <span class="text-xs text-gray-500">Live</span>
    </div>
  </div>
  <div id="log-output" class="p-4 font-mono text-xs text-gray-300 max-h-96 overflow-y-auto space-y-0.5 scrollbar-custom">
    <!-- Server appends log lines here -->
  </div>
</div>

<!-- Auto-scroll to bottom on new content -->
<div data-on-signal-patch="document.getElementById('log-output').scrollTop = document.getElementById('log-output').scrollHeight"></div>
```

**Server streams log lines:**
```rust
while let Some(line) = log_rx.recv().await {
    let html = format!(
        r#"<div class="flex gap-3"><span class="text-gray-600 select-none">{}</span><span>{}</span></div>"#,
        timestamp, escape_html(&line)
    );
    yield Ok(PatchElements::new(html)
        .selector("#log-output")
        .mode(MergeMode::Append)
        .write_as_axum_sse_event());
}
```

### Sidebar with Collapsible Groups (Dagster-style)

```html
<nav class="w-56 border-r border-gray-200 dark:border-gray-800 flex flex-col gap-1 p-3"
     data-signals='{"_navAssets": true, "_navJobs": true, "_navSettings": false}'>

  <!-- Group: Assets -->
  <button data-on:click="$_navAssets = !$_navAssets"
    class="flex items-center justify-between px-2 py-1.5 text-[11px] font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200">
    Assets
    <svg class="h-3.5 w-3.5 transition-transform" data-class:rotate-180="$_navAssets" fill="none" viewBox="0 0 24 24" stroke="currentColor">
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"/>
    </svg>
  </button>
  <div data-show="$_navAssets" class="flex flex-col gap-0.5 ml-2">
    <a data-on:click="@get('/page/assets')"
       data-class:bg-blue-50="$activePage === 'assets'"
       data-class:dark:bg-blue-950/30="$activePage === 'assets'"
       data-class:text-blue-700="$activePage === 'assets'"
       data-class:dark:text-blue-400="$activePage === 'assets'"
       class="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer">
      All assets
    </a>
    <a data-on:click="@get('/page/assets?filter=stale')"
       class="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer">
      Stale
      <span class="ml-auto text-xs text-gray-500 bg-gray-100 dark:bg-gray-800 rounded-full px-1.5">3</span>
    </a>
  </div>

  <!-- Group: Jobs -->
  <button data-on:click="$_navJobs = !$_navJobs"
    class="flex items-center justify-between px-2 py-1.5 mt-2 text-[11px] font-semibold uppercase tracking-wider text-gray-500 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-200">
    Jobs
    <svg class="h-3.5 w-3.5 transition-transform" data-class:rotate-180="$_navJobs" fill="none" viewBox="0 0 24 24" stroke="currentColor">
      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7"/>
    </svg>
  </button>
  <div data-show="$_navJobs" class="flex flex-col gap-0.5 ml-2">
    <a data-on:click="@get('/page/runs')"
       class="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer">
      All runs
    </a>
    <a data-on:click="@get('/page/runs?status=running')"
       class="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-800 cursor-pointer">
      Active
      <span class="ml-auto h-2 w-2 rounded-full bg-amber-500 animate-pulse"></span>
    </a>
  </div>
</nav>
```

### Timeline / Activity Feed

```html
<div class="flow-root">
  <ul class="-mb-8">
    <!-- Timeline item -->
    <li class="relative pb-8">
      <!-- Connector line -->
      <span class="absolute left-4 top-4 -ml-px h-full w-0.5 bg-gray-200 dark:bg-gray-800" aria-hidden="true"></span>
      <div class="relative flex gap-3">
        <!-- Icon -->
        <span class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-emerald-100 dark:bg-emerald-900/30 ring-8 ring-white dark:ring-gray-900">
          <svg class="h-4 w-4 text-emerald-600 dark:text-emerald-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
            <path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7"/>
          </svg>
        </span>
        <!-- Content -->
        <div class="flex-1 min-w-0 pt-1">
          <p class="text-sm text-gray-700 dark:text-gray-300">
            <span class="font-medium text-gray-900 dark:text-white">daily_revenue</span> materialized successfully
          </p>
          <p class="mt-0.5 text-xs text-gray-500">2 minutes ago</p>
        </div>
      </div>
    </li>
    <!-- Last item: no connector line -->
    <li class="relative">
      <div class="relative flex gap-3">
        <span class="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-blue-100 dark:bg-blue-900/30 ring-8 ring-white dark:ring-gray-900">
          <svg class="h-4 w-4 text-blue-600 dark:text-blue-400 animate-spin" fill="none" viewBox="0 0 24 24">
            <circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
            <path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"></path>
          </svg>
        </span>
        <div class="flex-1 min-w-0 pt-1">
          <p class="text-sm text-gray-700 dark:text-gray-300">
            <span class="font-medium text-gray-900 dark:text-white">weekly_summary</span> is running...
          </p>
          <p class="mt-0.5 text-xs text-gray-500">Started 30 seconds ago</p>
        </div>
      </div>
    </li>
  </ul>
</div>
```

### Stats / Metric Cards (Dashboard Header)

```html
<div class="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
  <!-- Stat card -->
  <div class="rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
    <p class="text-[11px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Total Assets</p>
    <p class="mt-2 text-3xl font-semibold tracking-tight text-gray-900 dark:text-white" id="stat-total" data-text="$totalAssets">—</p>
    <p class="mt-1 text-xs text-gray-500 dark:text-gray-400">
      <span class="text-emerald-600 dark:text-emerald-400" data-text="`${$freshCount} fresh`"></span>
      <span class="mx-1">·</span>
      <span class="text-amber-600 dark:text-amber-400" data-text="`${$staleCount} stale`"></span>
    </p>
  </div>

  <div class="rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
    <p class="text-[11px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Active Runs</p>
    <p class="mt-2 text-3xl font-semibold tracking-tight text-gray-900 dark:text-white" data-text="$activeRuns">—</p>
    <div class="mt-2 flex items-center gap-1.5" data-show="$activeRuns > 0">
      <span class="h-2 w-2 rounded-full bg-amber-500 animate-pulse"></span>
      <span class="text-xs text-amber-600 dark:text-amber-400">In progress</span>
    </div>
  </div>

  <div class="rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
    <p class="text-[11px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Success Rate</p>
    <p class="mt-2 text-3xl font-semibold tracking-tight text-emerald-600 dark:text-emerald-400" data-text="`${$successRate}%`">—</p>
    <div class="mt-2 w-full bg-gray-200 dark:bg-gray-800 rounded-full h-1.5">
      <div class="bg-emerald-500 h-1.5 rounded-full transition-all" data-style:width="`${$successRate}%`"></div>
    </div>
  </div>

  <div class="rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
    <p class="text-[11px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Last Refresh</p>
    <p class="mt-2 text-lg font-medium text-gray-900 dark:text-white" data-text="$lastRefresh">—</p>
    <button
      data-on:click="@post('/reindex')"
      data-indicator:_reindexing
      class="mt-2 text-xs text-blue-600 dark:text-blue-400 hover:underline"
    >
      <span data-show="!$_reindexing">Reindex now</span>
      <span data-show="$_reindexing">Reindexing...</span>
    </button>
  </div>
</div>
```

### Toggle Switch

```html
<label class="relative inline-flex items-center cursor-pointer">
  <input type="checkbox" data-bind:checked="$autoRefresh" class="sr-only peer" />
  <div class="w-9 h-5 bg-gray-300 dark:bg-gray-700 peer-checked:bg-blue-600 rounded-full
    after:content-[''] after:absolute after:top-0.5 after:left-[2px] after:bg-white after:rounded-full
    after:h-4 after:w-4 after:transition-all peer-checked:after:translate-x-full"></div>
  <span class="ml-2.5 text-sm text-gray-600 dark:text-gray-400">Auto-refresh</span>
</label>
```

### Keyboard Shortcut Hints

```html
<!-- Attach to buttons -->
<button data-on:click="@post('/api/run')"
  class="inline-flex items-center gap-2 px-4 py-2 bg-gray-900 dark:bg-white text-white dark:text-gray-900 rounded-lg text-sm font-medium">
  Run
  <kbd class="hidden sm:inline-flex items-center gap-0.5 rounded border border-gray-600 dark:border-gray-300 px-1.5 py-0.5 text-[10px] font-mono text-gray-400 dark:text-gray-500">
    <span class="text-xs">⌘</span>R
  </kbd>
</button>

<!-- Global listener -->
<div data-on:keydown__window="evt.metaKey && evt.key === 'r' && (evt.preventDefault(), @post('/api/run'))"></div>
```

### Multi-Select Filter Bar

```html
<div data-signals='{"_filterStatus": "all"}' class="flex items-center gap-1 p-1 bg-gray-100 dark:bg-gray-800 rounded-lg">
  <button
    data-on:click="$_filterStatus = 'all'; @get('/api/assets')"
    data-class:bg-white="$_filterStatus === 'all'"
    data-class:dark:bg-gray-700="$_filterStatus === 'all'"
    data-class:shadow-sm="$_filterStatus === 'all'"
    data-class:text-gray-900="$_filterStatus === 'all'"
    data-class:dark:text-white="$_filterStatus === 'all'"
    class="px-3 py-1.5 text-sm rounded-md text-gray-500 dark:text-gray-400 transition-all"
  >All</button>
  <button
    data-on:click="$_filterStatus = 'fresh'; @get('/api/assets?status=fresh')"
    data-class:bg-white="$_filterStatus === 'fresh'"
    data-class:dark:bg-gray-700="$_filterStatus === 'fresh'"
    data-class:shadow-sm="$_filterStatus === 'fresh'"
    data-class:text-emerald-700="$_filterStatus === 'fresh'"
    class="px-3 py-1.5 text-sm rounded-md text-gray-500 dark:text-gray-400 transition-all"
  >Fresh</button>
  <button
    data-on:click="$_filterStatus = 'stale'; @get('/api/assets?status=stale')"
    data-class:bg-white="$_filterStatus === 'stale'"
    data-class:dark:bg-gray-700="$_filterStatus === 'stale'"
    data-class:shadow-sm="$_filterStatus === 'stale'"
    data-class:text-amber-700="$_filterStatus === 'stale'"
    class="px-3 py-1.5 text-sm rounded-md text-gray-500 dark:text-gray-400 transition-all"
  >Stale</button>
  <button
    data-on:click="$_filterStatus = 'failed'; @get('/api/assets?status=failed')"
    data-class:bg-white="$_filterStatus === 'failed'"
    data-class:dark:bg-gray-700="$_filterStatus === 'failed'"
    data-class:shadow-sm="$_filterStatus === 'failed'"
    data-class:text-rose-700="$_filterStatus === 'failed'"
    class="px-3 py-1.5 text-sm rounded-md text-gray-500 dark:text-gray-400 transition-all"
  >Failed</button>
</div>
```

---

## 9. Reimplementing SPA Plugins

The dataSPA fork adds four SPA-focused plugins that were removed from open-source Datastar.
All four are trivially simple — here's how to reimplement each using vanilla Datastar
primitives or a small inline `<script>`.

### URL Management (replaces `data-replace-url`)

The original plugin is just `window.history.replaceState` inside a reactive effect.
We can do this with `data-effect`:

```html
<!-- Option A: Use data-effect directly (zero extra JS) -->
<div data-signals='{"_currentUrl": "/"}'>
  <div data-effect="window.history.replaceState({}, '', $_currentUrl)"></div>
</div>
```

Then from the server, update the URL by patching the signal:
```
event: datastar-patch-signals
data: signals {"_currentUrl": "/assets/42"}
```

**Option B:** If you want `pushState` for back-button support, execute a script via `datastar-patch-elements`:
```
event: datastar-patch-elements
data: mode append
data: selector body
data: elements <script>window.history.pushState({}, '', '/assets/42')</script>

```

And handle popstate to re-fetch content:
```html
<div data-on:popstate__window="@get(window.location.pathname)"></div>
```

### View Transitions (replaces `data-view-transition`)

The plugin just sets `el.style.viewTransitionName`. Two options:

```html
<!-- Option A: use data-style (zero extra JS) -->
<div data-style:view-transition-name="'panel-content'">...</div>

<!-- Option B: use the __viewtransition modifier on data-on (already built in!) -->
<button data-on:click__viewtransition="@get('/next-page')">Navigate</button>
```

The `__viewtransition` modifier wraps the entire fetch+morph in `document.startViewTransition()`,
which is usually what you want for navigation transitions.

### Persist Signals (replaces `data-persist`)

A small inline script (~15 lines) in the base template:

```html
<script>
  // Restore persisted signals on load
  (function() {
    const key = 'barca-signals';
    const stored = localStorage.getItem(key);
    if (stored) {
      try {
        // Will be picked up by data-signals on body
        window.__barcaPersistedSignals = JSON.parse(stored);
      } catch(e) {}
    }
    // Save signals periodically (or on beforeunload)
    window.addEventListener('beforeunload', function() {
      // Read from Datastar's store - only persist underscore-prefixed signals
      // that we explicitly want to keep
      const toSave = {};
      // Add specific signals to persist:
      if (typeof $sidebarOpen !== 'undefined') toSave.sidebarOpen = $sidebarOpen;
      if (typeof $theme !== 'undefined') toSave.theme = $theme;
      localStorage.setItem(key, JSON.stringify(toSave));
    });
  })();
</script>
```

Or even simpler — just use `data-effect` to persist specific signals:
```html
<div data-effect="localStorage.setItem('sidebar', JSON.stringify($sidebarOpen))"></div>

<!-- On load, read it back -->
<div data-signals:sidebarOpen="JSON.parse(localStorage.getItem('sidebar') || 'true')"></div>
```

### Scroll Into View (replaces `data-scroll-into-view`)

Use `data-init` with vanilla JS:

```html
<div data-init="this.scrollIntoView({behavior: 'smooth', block: 'center'})"></div>
```

Or for newly-appended elements (e.g., scroll to new log entry):
```html
<div data-init="this.scrollIntoView({behavior: 'smooth', block: 'nearest'})"></div>
```

### Summary: What You Need vs What's Built In

| Capability | Built into vanilla Datastar? | Reimplementation |
|---|---|---|
| URL replacement | No | `data-effect` + `history.replaceState` (1 line) |
| Push state + back button | No | `datastar-patch-elements` script append + `data-on:popstate__window` |
| View transitions on nav | **Yes** (`__viewtransition` modifier) | Already works |
| View transition names | No | `data-style:view-transition-name` (1 line) |
| Persist to localStorage | No | `data-effect` + `localStorage.setItem` (1 line per signal) |
| Scroll into view | No | `data-init` + `this.scrollIntoView(...)` (1 line) |

---

## 10. Rust/Axum Backend Patterns

### Reading Signals from Requests

```rust
use axum::extract::Query;
use serde::Deserialize;

// GET requests: signals in ?datastar= query param
#[derive(Deserialize)]
struct DatastarQuery {
    datastar: Option<String>,
}

async fn handle_get(Query(q): Query<DatastarQuery>) -> impl IntoResponse {
    if let Some(signals_json) = q.datastar {
        let signals: serde_json::Value = serde_json::from_str(&signals_json).unwrap();
        // Use signals...
    }
    // Return SSE stream
}

// POST requests: signals in request body
async fn handle_post(body: String) -> impl IntoResponse {
    let signals: serde_json::Value = serde_json::from_str(&body).unwrap();
    // Use signals...
}
```

### Emitting SSE Events (Raw)

```rust
use axum::response::sse::{Event, Sse};
use futures::stream;
use std::convert::Infallible;

async fn handler() -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let events = vec![
        // Patch signals
        Event::default()
            .event("datastar-patch-signals")
            .data("signals {\"count\": 42, \"loading\": false}"),

        // Patch elements
        Event::default()
            .event("datastar-patch-elements")
            .data("selector #content\ndata: mode inner\ndata: elements <div>Hello</div>"),
    ];

    Sse::new(stream::iter(events.into_iter().map(Ok)))
}
```

### Using the `datastar` Rust Crate

The project uses `datastar = "0.3"` with the `axum` feature. Key types:

```rust
use datastar::prelude::*;

// Build a patch-elements event
let event = PatchElements::new("<div id='content'>Hello</div>")
    .selector("#content")
    .mode(MergeMode::Inner)
    .use_view_transition(true)
    .write_as_axum_sse_event(); // Returns axum::response::sse::Event

// Build a patch-signals event
let event = PatchSignals::new(r#"{"count": 42}"#)
    .write_as_axum_sse_event();
```

### SSE Stream Handler Pattern

```rust
use axum::response::sse::{Event, Sse};
use futures::stream::Stream;

async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.broadcast_tx.subscribe();

    let stream = async_stream::stream! {
        // Send initial state
        yield Ok(PatchElements::new(render_initial_html())
            .selector("#main")
            .write_as_axum_sse_event());

        // Stream updates
        while let Ok(update) = rx.recv().await {
            yield Ok(PatchElements::new(render_update(&update))
                .selector(&update.selector)
                .write_as_axum_sse_event());
        }
    };

    Sse::new(stream)
}
```

### Multi-Event Response Pattern

For a single request that needs to update multiple parts of the page:

```rust
async fn handle_action() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        // Update signals (drives nav highlighting, URL, etc.)
        yield Ok(PatchSignals::new(r#"{"activePage": "runs", "_currentUrl": "/runs"}"#)
            .write_as_axum_sse_event());

        // Update main content
        yield Ok(PatchElements::new("<div id='content'>...</div>")
            .selector("#content")
            .mode(MergeMode::Inner)
            .write_as_axum_sse_event());

        // Update breadcrumb
        yield Ok(PatchElements::new("<nav id='breadcrumb'>Home > Runs</nav>")
            .selector("#breadcrumb")
            .write_as_axum_sse_event());

        // Show toast
        yield Ok(PatchElements::new("<div id='toast-1' class='...'>Success!</div>")
            .selector("#toast-container")
            .mode(MergeMode::Append)
            .write_as_axum_sse_event());
    };

    Sse::new(stream)
}
```

### Navigation Handler Pattern (Full SPA Nav)

A reusable pattern for page-level navigation:

```rust
/// SPA navigation: updates content, breadcrumb, signals, and URL in one response.
async fn navigate_to(
    page: &str,
    breadcrumb_html: &str,
    content_html: &str,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = async_stream::stream! {
        // 1. Update signals (drives nav highlighting + URL)
        yield Ok(PatchSignals::new(
            &format!(r#"{{"activePage": "{page}", "_currentUrl": "/page/{page}"}}"#)
        ).write_as_axum_sse_event());

        // 2. Update breadcrumb
        yield Ok(PatchElements::new(breadcrumb_html)
            .selector("#breadcrumb")
            .write_as_axum_sse_event());

        // 3. Update main content area
        yield Ok(PatchElements::new(content_html)
            .selector("#main-content")
            .mode(MergeMode::Inner)
            .use_view_transition(true)
            .write_as_axum_sse_event());
    };

    Sse::new(stream)
}
```

---

## 11. Gotchas & Constraints

### Syntax (CRITICAL for this project)

| Topic | Correct | Wrong |
|---|---|---|
| Event attribute | `data-on:click` (colon separator) | `data-on-click` (hyphen) |
| Modifiers | `data-on:click__debounce:500ms` | `data-on:click.debounce-500ms` |
| Event variable | `evt.key` | `$event.key` |
| Action prefix | `@post('/url')` | `$post('/url')` |
| Signal access | `$count` | `count` or `this.count` |

> **Version note:** This project uses Datastar v1.0.0-RC.8. RC.8 uses **colon** syntax for event
> attributes (`data-on:click`) and colon for modifier params (`__debounce:300ms`). Older versions
> used hyphens and dots respectively — ignore any examples showing the old style.

### Morphing Behavior

- Elements targeted by `outer` morph **must have an `id` attribute** (unless a `selector` is specified)
- Morphing preserves: focus, form values, scroll position, event listeners
- Use `data-ignore-morph` on subtrees you don't want morphed (e.g., video players, editors)
- Use `data-preserve-attr="class"` to keep specific attributes from being overwritten
- The morph uses idiomorph: matches elements by ID, diffs children, preserves identity

### Signals

- Signals prefixed with `_` (underscore) are **not sent to the server** by default — use for local UI state
- `null` values in `datastar-patch-signals` **delete** the signal (RFC 7386)
- Kebab-case attribute keys are converted to camelCase signal names
- Multiple signal updates in one expression are batched (effects fire once)
- `evt` is available in `data-on` handlers (the DOM event object)

### SSE

- Each SSE event must end with `\n\n` (double newline)
- Multi-line HTML: repeat `data: elements` for each line
- Set `Content-Type: text/event-stream` on the response
- For long-lived SSE connections, send periodic keep-alive comments (`:\n\n`)
- The `datastar` Rust crate handles all this formatting for you

### Security

- Always escape user input in HTML fragments (`templates::escape_html()`)
- Don't put sensitive data in signals (they're visible in the DOM and sent to server)
- CSP requires `unsafe-eval` because Datastar uses `new Function()` for expressions
- Use `data-ignore` on elements that shouldn't be processed

### Performance

- Prefer `mode: inner` over `mode: outer` when you don't need to update the wrapper element
- Use `data-on-intersect__once` for lazy loading (fires once, removes observer)
- `requestCancellation: 'auto'` cancels in-flight requests when new ones start (good for search-as-you-type)
- Batch signal updates on the server into a single `datastar-patch-signals` event when possible
- Use `_` prefix for signals that are purely local UI state (dropdown open, filter selection) — they won't waste bandwidth being sent to the server

### CSS Transitions + Datastar

When using `data-show` or `data-class` to toggle visibility with CSS transitions:
- `data-show` uses `display: none` — transitions won't animate because `display` isn't animatable
- For animated show/hide, use `data-class` to toggle opacity/transform classes instead
- Combine CSS `transition-*` utilities with `data-class` for smooth enter/exit:

```html
<!-- Animated panel (CSS transitions work because we never touch display) -->
<div class="transform transition-all duration-300"
     data-class:translate-x-0="$open"
     data-class:translate-x-full="!$open"
     data-class:opacity-100="$open"
     data-class:opacity-0="!$open">
  Panel content
</div>
```

vs.

```html
<!-- Simple toggle (instant, no animation) -->
<div data-show="$open">Panel content</div>
```
