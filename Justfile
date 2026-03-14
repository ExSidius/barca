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

# Run unit + API tests.
test:
    cargo test -p barca-server -p barca-cli --test cli_tests

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

# Install tools required for `just release`.
# Run once after cloning.
setup:
    rustup target add x86_64-apple-darwin aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu
    cargo install cargo-zigbuild
    pip install ziglang
    uv tool install maturin
    @echo "Setup complete. Set MATURIN_PYPI_TOKEN before running 'just release'."

# Publish a new release to PyPI and GitHub.
#
# Bumps all crate versions, builds wheels for all 4 platforms (macOS arm64/x86_64
# and Linux arm64/x86_64 via cargo-zigbuild), uploads to PyPI, then tags and
# creates a GitHub release with the wheels as assets.
#
# Usage:   just release 0.0.4
# Requires: just setup (run once), MATURIN_PYPI_TOKEN env var
release version:
    #!/usr/bin/env bash
    set -euo pipefail

    # ── Pre-flight checks ───────────────────────────────────────────────
    if [[ -n "$(git status --porcelain)" ]]; then
        echo "error: working tree is dirty — commit or stash changes first" >&2
        exit 1
    fi
    for tool in cargo maturin gh; do
        command -v "$tool" >/dev/null 2>&1 || { echo "error: $tool not found (run 'just setup')" >&2; exit 1; }
    done
    cargo zigbuild --version >/dev/null 2>&1 || { echo "error: cargo-zigbuild not found (run 'just setup')" >&2; exit 1; }
    [[ -n "${MATURIN_PYPI_TOKEN:-}" ]] || { echo "error: MATURIN_PYPI_TOKEN is not set" >&2; exit 1; }

    # ── Bump versions ───────────────────────────────────────────────────
    # Python PEP 440: 0.0.3rc1 — Cargo semver: 0.0.3-rc.1
    cargo_version=$(echo "{{version}}" | sed 's/rc\([0-9]*\)$/-rc.\1/' | sed 's/a\([0-9]*\)$/-alpha.\1/' | sed 's/b\([0-9]*\)$/-beta.\1/')
    echo "=== Bumping versions: pypi={{version}} cargo=${cargo_version} ==="
    sed -i '' "s/^version = \".*\"/version = \"${cargo_version}\"/" \
        crates/barca-cli/Cargo.toml \
        crates/barca-core/Cargo.toml \
        crates/barca-py/Cargo.toml \
        crates/barca-server/Cargo.toml
    sed -i '' 's/^version = ".*"/version = "{{version}}"/' \
        crates/barca-py/pyproject.toml
    cargo build -p barca-core -q  # update Cargo.lock
    git add crates/*/Cargo.toml crates/barca-py/pyproject.toml Cargo.lock
    git commit -m "chore: bump version to {{version}}"
    git push origin main

    # ── Build wheels ────────────────────────────────────────────────────
    echo "=== Building wheels ==="
    rm -rf dist/ && mkdir dist/

    for target in aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
        echo "--- $target ---"

        # Build the CLI binary (zigbuild for Linux to target glibc 2.17)
        if [[ "$target" == *linux* ]]; then
            cargo zigbuild -p barca-cli --release --target "${target}.2.17"
        else
            cargo build -p barca-cli --release --target "$target"
        fi

        # Stage CLI binary into the wheel data directory
        mkdir -p crates/barca-py/data/scripts
        cp "target/$target/release/barca" crates/barca-py/data/scripts/barca
        chmod +x crates/barca-py/data/scripts/barca

        # Build the maturin wheel (--zig for Linux to cross-compile the .so)
        if [[ "$target" == *linux* ]]; then
            maturin build --release --target "$target" --zig \
                --manifest-path crates/barca-py/Cargo.toml \
                --out dist/
        else
            maturin build --release --target "$target" \
                --manifest-path crates/barca-py/Cargo.toml \
                --find-interpreter \
                --out dist/
        fi

        rm -rf crates/barca-py/data/scripts
    done

    # Build source distribution
    echo "--- sdist ---"
    maturin sdist --manifest-path crates/barca-py/Cargo.toml --out dist/

    echo "=== Built ==="
    ls -lh dist/

    # ── Publish ─────────────────────────────────────────────────────────
    echo "=== Uploading to PyPI ==="
    maturin upload --skip-existing dist/*

    echo "=== Tagging v{{version}} ==="
    git tag "v{{version}}"
    git push origin "v{{version}}"

    echo "=== Creating GitHub release ==="
    gh release create "v{{version}}" dist/*.whl dist/*.tar.gz \
        --title "v{{version}}" \
        --generate-notes

    echo ""
    echo "Released v{{version}}"
