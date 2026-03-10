# Templates Architecture

## Overview

Barca uses a **hybrid approach** for UI organization that balances simplicity, performance, and maintainability:

1. **Rust string constants** for HTML templates (no external files)
2. **Web Components** for reusable, self-contained UI elements
3. **Dedicated `templates.rs` module** for organization

## Why This Approach?

### ✅ What We DO Use

#### 1. Rust String Constants (`templates.rs`)
```rust
pub fn asset_card(asset_id: i64, function_name: &str, ...) -> String {
    format!(r#"<article>...</article>"#, ...)
}
```

**Benefits:**
- **Zero runtime overhead** - compiled into the binary
- **Type-safe** - compile-time checking
- **Fast** - no file I/O at runtime
- **Simple deployment** - single binary, no asset bundling
- **Easy refactoring** - IDE support, find/replace works

**Use for:**
- Page layouts
- Component templates
- Repeated HTML structures

#### 2. Web Components (Custom Elements)
```javascript
class AssetStatusBadge extends HTMLElement {
  render() { ... }
}
customElements.define("asset-status-badge", AssetStatusBadge);
```

**Benefits:**
- **Truly reusable** - works anywhere in the DOM
- **Self-contained** - encapsulates behavior + styling
- **Dynamic** - can update based on attributes
- **Standard** - native browser API, no framework

**Use for:**
- Interactive UI elements (badges, tooltips, modals)
- Components that need to update dynamically
- Reusable widgets across pages

### ❌ What We DON'T Use

#### External Template Files (Handlebars, Tera, etc.)
**Why not:**
- Adds runtime file I/O overhead
- Requires bundling/deployment complexity
- Goes against "extremely simple and fast" goal
- No type safety
- Harder to refactor

#### Heavy JavaScript Frameworks (React, Vue, etc.)
**Why not:**
- Massive bundle sizes (100KB+)
- Build complexity
- Overkill for our simple UI
- Slower initial page load

## Current Architecture

### File Structure
```
src/
├── main.rs          # Minimal entry point, startup wiring
├── lib.rs           # Core orchestration (reindex, job queue, worker)
├── server.rs        # Routes, handlers, rendering
├── templates.rs     # HTML templates and components
├── models.rs        # Data structures
├── store.rs         # Database layer
└── ...
```

**Heuristic:** No file should exceed ~500 lines. Split further if needed.

### Template Organization (`templates.rs`)

#### Base Templates
- `page()` - Base HTML document with head, scripts, styles
- `web_components()` - Web component definitions
- `styles()` - Global CSS

#### Reusable Components
- `theme_toggle()` - Dark/light mode switcher
- `page_header()` - Page title + reindex button
- `detail_nav()` - Back navigation + reindex button
- `asset_card()` - Asset display card
- `definition_section()` - Asset definition details
- `empty_asset_list()` - Empty state

#### Utilities
- `escape_html()` - HTML escaping helper

### Usage Pattern

```rust
// In main.rs route handlers
async fn index_page(State(state): State<AppState>) -> Html<String> {
    let assets = store.list_assets().await?;

    Ok(Html(templates::page(
        "Barca",
        &format!(
            r#"
            {}
            <main>
              {}
              {}
            </main>
            "#,
            templates::theme_toggle(),
            templates::page_header("Assets", "Description..."),
            render_asset_list(&assets)
        ),
    )))
}
```

## Datastar Integration

Barca uses the [datastar-rust](https://github.com/starfederation/datastar-rust) SDK for SSE patches. Do not hand-roll event formatting.

### HTML + Datastar Pattern

- Server renders HTML in Rust (`templates.rs`); no external template files
- Patches target DOM elements by ID (e.g. `#asset-list`, `#asset-panel`, `#main-content`)
- Use `datastar::prelude::PatchElements` for all SSE patches:
  ```rust
  PatchElements::new(html).selector("#asset-panel").write_as_axum_sse_event()
  ```
- HTML fragments must include the target element with matching ID so the client can apply the patch

### SSE Push Pattern (No Polling)

- **Design requirement:** Avoid polling; use server-push via `datastar-patch-elements` SSE events
- Use `tokio::sync::broadcast` to notify SSE handlers when backend state changes (e.g. job completion)
- Worker/core logic calls `broadcast.send(asset_id)` after persisting; SSE handler subscribes and yields patches
- Stream with `async_stream::stream!`; yield `Ok(PatchElements::new(...).write_as_axum_sse_event())`

### EventSource + Panel Pattern

- Client opens `EventSource(url)` when user opens a panel (e.g. clicks asset card)
- First SSE event = initial content; subsequent events = server-pushed updates
- Client closes `EventSource` when panel closes
- Custom `applyDatastarPatch()` parses the SSE data format and applies DOM updates

## Web Components in Detail

### Current Components

#### AssetStatusBadge
```html
<asset-status-badge label="Fresh" tone="fresh"></asset-status-badge>
```

**Features:**
- Observes `label` and `tone` attributes
- Auto-updates when attributes change
- Applies appropriate Tailwind classes based on tone
- Handles light/dark mode styling

### When to Create a Web Component

**Good candidates:**
- Status indicators
- Interactive buttons with state
- Tooltips, popovers
- Form controls with custom behavior
- Any UI element used in multiple places that needs interactivity

**Not good candidates:**
- Static HTML structures (use template functions)
- One-off components
- Server-rendered content that never changes

## Performance Characteristics

### Template Rendering
- **Time:** ~microseconds (string concatenation)
- **Memory:** Minimal (stack-allocated strings)
- **Network:** No extra requests (inline in HTML)

### Web Components
- **Load time:** ~1-2ms per component definition
- **Memory:** ~1KB per component class
- **Render time:** ~1ms per instance

### Page Load
1. Single HTML request
2. Inline CSS/JS (no extra requests)
3. Tailwind CDN (cached after first load)
4. Datastar CDN (cached after first load)

**Total:** ~50-100ms for first load, ~10-20ms for subsequent loads

## Future Considerations

### Potential Additions

1. **More Web Components**
   - `<asset-graph>` - Dependency visualization
   - `<run-timeline>` - Execution timeline
   - `<code-diff>` - Show definition changes

2. **Template Partials**
   - Break down large templates into smaller functions
   - Keep in `templates.rs` for consistency

3. **CSS Organization**
   - Consider extracting to `templates::styles::*` submodule if it grows
   - Keep inline for now (maintains simplicity)

### What to Avoid

- ❌ External template files
- ❌ Heavy build processes
- ❌ JavaScript frameworks
- ❌ CSS-in-JS libraries
- ❌ Complex state management

## Best Practices

### Template Functions
```rust
// ✅ Good: Clear, focused, reusable
pub fn asset_card(id: i64, name: &str) -> String { ... }

// ❌ Bad: Too many parameters, hard to maintain
pub fn render_everything(a: &str, b: i64, c: bool, d: Vec<String>, ...) -> String { ... }
```

### Web Components
```javascript
// ✅ Good: Small, focused, self-contained
class StatusBadge extends HTMLElement { ... }

// ❌ Bad: Large, complex, many dependencies
class MegaComponent extends HTMLElement {
  // 500 lines of code...
}
```

### Composition
```rust
// ✅ Good: Compose small templates
format!(
    r#"{}{}{}#,
    templates::header(),
    templates::content(),
    templates::footer()
)

// ❌ Bad: One giant template string
format!(r#"<html>...1000 lines...</html>"#)
```

## Migration Guide

### Adding a New Template

1. Add function to `src/templates.rs`:
```rust
pub fn my_component(data: &str) -> String {
    format!(r#"<div>{}</div>"#, escape_html(data))
}
```

2. Use in `src/main.rs`:
```rust
let html = templates::my_component(&data);
```

### Adding a New Web Component

1. Add to `web_components()` in `src/templates.rs`:
```javascript
class MyComponent extends HTMLElement {
  connectedCallback() { this.render(); }
  render() { this.innerHTML = `<div>...</div>`; }
}
customElements.define("my-component", MyComponent);
```

2. Use in templates:
```html
<my-component data="value"></my-component>
```

## Summary

This architecture provides:
- ✅ **Simple** - No build tools, no external files
- ✅ **Fast** - Zero runtime overhead, minimal JS
- ✅ **Maintainable** - Organized, type-safe, refactorable
- ✅ **Flexible** - Easy to add components or templates
- ✅ **Minimal** - ~300 lines of template code total

Perfect for Barca's goal of being "extremely simple and fast"!
