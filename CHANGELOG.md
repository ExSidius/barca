# Changelog

## [0.2.2] - 2025-03-08

### Fixed

- Replace Tailwind CDN with production build: use PostCSS/Tailwind CLI to build CSS at compile time, eliminating "cdn.tailwindcss.com should not be used in production" warning

### Technical

- Added `package.json`, `tailwind.config.js`, `static/css/input.css` for Tailwind build
- Built CSS embedded via `include_str!`; run `just build-css` or `npm run build:css` after changing templates

## [0.2.1] - 2025-03-08

### Fixed

- Datastar action attributes: use `data-on:click` (colon) instead of `data-on-click` (hyphen) so Reindex and Refresh buttons work
- Materialize SSE response: only patch the asset card, not the status fragment (avoids patching non-existent `#asset-status` into main content)
- Full page reload on click: add `type="button"` and `__prevent` modifier to Datastar action buttons so clicks are intercepted and default behavior is prevented

## [0.2.0] - 2025-03-08

### Added

- Sidebar with Assets and Jobs tabs; default view is Assets
- Right-side asset panel: clicking an asset card opens a panel instead of navigating
- Job history in asset panel with status icons (queued: ..., running: spinner, success: green check, failed: red x)
- Jobs view listing recent materializations across all assets
- Live job updates via SSE push (no polling); panel stream pushes updates when jobs complete
- datastar-rust SDK integration for all SSE patches
- 3s sleep in example asset to simulate long-running computation

### Changed

- Reorganized Rust code: `lib.rs` (core orchestration), `server.rs` (web serving), `main.rs` (minimal entry point)
- Removed asset detail page (`GET /assets/{id}`); replaced with inline panel
- Asset card click opens panel via EventSource instead of navigation

### Technical

- Added `list_materializations_for_asset` and `list_recent_materializations` to store
- Added `job_completion_tx` broadcast channel for SSE push
- Extended `docs/templates-architecture.md` with Datastar Integration patterns
