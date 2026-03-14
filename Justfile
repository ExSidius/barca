# Run the server with hot reload (restarts on file changes).
# Requires: cargo install cargo-watch
dev:
    cargo watch -x 'run -p barca-cli'

# Build Tailwind CSS for production (run after changing templates or Tailwind classes).
build-css:
    ./bin/tailwindcss -i ./static/css/input.css -o ./static/css/output.css --minify

# Build and install the barca Python extension + CLI into the active venv.
# Requires: maturin (uv tool install maturin)
build-py:
    cargo build -p barca-cli --release
    mkdir -p crates/barca-py/data/scripts
    cp target/release/barca crates/barca-py/data/scripts/barca
    maturin develop -m crates/barca-py/Cargo.toml
    rm -rf crates/barca-py/data/scripts

# Build the server binary.
build:
    cargo build -p barca-cli

# Run end-to-end integration tests against real Python projects.
# Builds the Python extension and runs the w1/w2/w3 test suites.
test-e2e:
    just build-py
    cd examples/basic_app && uv sync
    cargo test -p barca-cli --test 'w*' -- --test-threads=1

# Run the server against an example app.
# Usage: just example basic_app
example name:
    just build-py
    cd examples/{{name}} && uv sync && cargo run -p barca-cli
