# Development Setup

How to install and test barca from source (a git branch rather than a PyPI release).

## Quick start (with Just)

If you have [just](https://github.com/casey/just) installed:

```bash
git clone https://github.com/ExSidius/barca.git
cd barca
just build-py                          # builds CLI + Python extension, installs into venv
cd examples/iris_pipeline && uv sync   # install example deps
uv run barca reindex                   # discover assets
uv run barca assets refresh 1          # run the pipeline
```

## Manual setup (without Just)

```bash
git clone https://github.com/ExSidius/barca.git
cd barca

# 1. Build the Rust CLI binary
cargo build -p barca-cli --release

# 2. Stage the binary so maturin bundles it into the Python wheel
mkdir -p crates/barca-py/data/scripts
cp target/release/barca crates/barca-py/data/scripts/barca
chmod +x crates/barca-py/data/scripts/barca

# 3. Install the Python extension (includes the CLI binary)
cd examples/iris_pipeline   # or whichever example
uv sync

# 4. Verify
uv run barca reindex

# 5. Clean up the staged binary (don't commit it)
rm -f ../../crates/barca-py/data/scripts/barca
```

## Testing from a branch

To test a feature branch in full isolation (fresh clone, no shared state):

```bash
./scripts/test-from-branch.sh                  # test all examples
./scripts/test-from-branch.sh iris_pipeline    # test one example
```

This clones the current branch into a temp directory, builds everything from
scratch, runs each example, verifies caching, and cleans up on exit.

## How `uv run barca` works

The `barca` Python package (built by maturin from `crates/barca-py/`) contains
two things:

1. **A native Python extension** (`barca._barca`): the `@asset()` decorator,
   `inspect_modules()`, `materialize_asset()` — all compiled Rust via PyO3.

2. **The CLI binary** (`barca`): bundled via maturin's `data/scripts/` mechanism.
   When the wheel is installed, the binary lands in the venv's `bin/` directory,
   making `uv run barca` work.

In development, step 2 requires manually building and staging the binary
(see above). In a PyPI release, the `just release` command handles this
automatically for all target platforms.

## Without `uv run` (cargo directly)

If you don't need `uv run barca` and just want to iterate on Rust code:

```bash
cargo build -p barca-cli
cd examples/iris_pipeline && uv sync   # only needed once for Python deps
cargo run -p barca-cli -- reindex
cargo run -p barca-cli -- assets refresh 1
```

This is faster for Rust development since you skip the maturin build. The
`cargo run -p barca-cli --` prefix replaces `uv run barca`.

## Running tests

```bash
# Unit + API tests (fast, no Python needed)
cargo test -p barca-server

# CLI integration tests against real Python examples
just build-py
cd examples/basic_app && uv sync
cd examples/iris_pipeline && uv sync
cargo test -p barca-cli
```
