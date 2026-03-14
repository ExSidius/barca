# Run the server with hot reload (restarts on file changes).
# Requires: cargo install cargo-watch
dev:
    cargo watch -x 'run -p barca-server'

# Build Tailwind CSS for production (run after changing templates or Tailwind classes).
build-css:
    ./bin/tailwindcss -i ./static/css/input.css -o ./static/css/output.css --minify

# Build and install the barca Python extension into the active venv.
# Requires: maturin (uv tool install maturin)
build-py:
    maturin develop -m crates/barca-py/Cargo.toml

# Build the server binary.
build:
    cargo build -p barca-server

# Run the server against an example app (uses Python entry point).
# Usage: just example basic_app
example name:
    just build-py
    cd examples/{{name}} && uv sync && uv run barca
