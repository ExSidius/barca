# Testing Checklist

Manual test cases to verify before releases. These are derived from real bugs encountered during development.

## Server Lifecycle

- [ ] **Ctrl+C terminates `uv run barca`** — Press Ctrl+C once while the server is running via `uv run barca` in `examples/basic_app/`. The process should exit within ~1 second. (Bug: Python's SIGINT handler swallows the signal when the GIL is released, leaving the process unkillable.)
- [ ] **Ctrl+C terminates `cargo run -p barca-server`** — Same test but via the standalone Rust binary.
- [ ] **Ctrl+C during active materialization** — Start a materialization, then Ctrl+C. The process should still exit promptly (spawned Python worker subprocesses should not keep it alive).

## Asset Materialization

- [ ] **Basic materialize round-trip** — Click materialize on an asset in the UI, verify it completes and the result appears.
- [ ] **Duplicate materialization guard** — Click materialize twice quickly on the same asset. Only one job should be queued.

## SSE / UI

- [ ] **Asset panel opens and streams updates** — Click an asset to open the side panel. Verify the panel populates via SSE.
- [ ] **Page load renders all assets** — Navigate to `http://127.0.0.1:3000` and verify all indexed assets appear.

## Python Bridge

- [ ] **`uv run python -m barca.inspect` works standalone** — Run from `examples/basic_app/`. Should print JSON describing discovered assets.
- [ ] **`uv run python -m barca.worker` works standalone** — Materializes a single asset and prints the result JSON.

## Build

- [ ] **`just build-py` succeeds** — Builds the PyO3 extension and installs it into the active venv.
- [ ] **`just build-css` succeeds** — Rebuilds Tailwind CSS without errors.
- [ ] **`cargo build --workspace` succeeds** — Full workspace compiles with no errors.
