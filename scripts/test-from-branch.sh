#!/usr/bin/env bash
#
# Test barca from a fresh clone of the current branch.
#
# Usage:
#   ./scripts/test-from-branch.sh                 # test all examples
#   ./scripts/test-from-branch.sh iris_pipeline    # test one example
#
# What it does:
#   1. Creates a temp directory
#   2. Clones the current branch into it
#   3. Builds the Rust CLI
#   4. For each example: uv sync, reindex, refresh leaf asset
#   5. Cleans up on exit
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BRANCH="$(git -C "$REPO_ROOT" rev-parse --abbrev-ref HEAD)"
EXAMPLE_FILTER="${1:-}"

TMPDIR="$(mktemp -d)"
trap 'echo "Cleaning up $TMPDIR"; rm -rf "$TMPDIR"' EXIT

echo "=== Testing branch: $BRANCH ==="
echo "=== Clone directory: $TMPDIR/barca ==="
echo ""

# 1. Clone
git clone --branch "$BRANCH" "file://$REPO_ROOT" "$TMPDIR/barca" 2>&1 | tail -1
cd "$TMPDIR/barca"

# 2. Build CLI and stage into Python package
echo "Building barca CLI..."
cargo build -p barca-cli 2>&1 | tail -1
BARCA="$TMPDIR/barca/target/debug/barca"
echo "Built: $BARCA"

# Stage binary so `uv sync` bundles it into the venv
mkdir -p "$TMPDIR/barca/crates/barca-py/data/scripts"
cp "$BARCA" "$TMPDIR/barca/crates/barca-py/data/scripts/barca"
chmod +x "$TMPDIR/barca/crates/barca-py/data/scripts/barca"
echo "Staged binary for uv sync"
echo ""

# 3. Test each example
test_example() {
    local name="$1"
    local dir="$TMPDIR/barca/examples/$name"

    if [ ! -d "$dir" ]; then
        echo "SKIP: $name (not found)"
        return
    fi

    echo "--- Testing example: $name ---"
    cd "$dir"

    # Install Python deps
    echo "  uv sync..."
    uv sync 2>&1 | tail -1

    # Verify uv run barca works
    if uv run barca --help >/dev/null 2>&1; then
        echo "  uv run barca: OK"
    else
        echo "  WARN: uv run barca not available (using cargo binary directly)"
    fi

    # Reindex
    echo "  reindex..."
    "$BARCA" reindex 2>/dev/null

    # Find the leaf asset (highest ID = last in dependency order)
    local leaf_id
    leaf_id=$("$BARCA" assets list 2>/dev/null | grep -oP '│\s*\K\d+' | sort -n | tail -1)

    if [ -z "$leaf_id" ]; then
        echo "  FAIL: no assets found"
        return 1
    fi

    # Refresh the leaf asset
    echo "  refresh asset #$leaf_id..."
    local output
    output=$("$BARCA" assets refresh "$leaf_id" 2>/dev/null)

    if echo "$output" | grep -q "success"; then
        echo "  PASS: $name (asset #$leaf_id succeeded)"
    else
        echo "  FAIL: $name"
        echo "$output" | tail -5
        return 1
    fi

    # Second run should be cached
    echo "  verifying cache..."
    local start end elapsed
    start=$(date +%s)
    "$BARCA" assets refresh "$leaf_id" 2>/dev/null >/dev/null
    end=$(date +%s)
    elapsed=$((end - start))

    if [ "$elapsed" -le 5 ]; then
        echo "  PASS: cache hit (${elapsed}s)"
    else
        echo "  WARN: cache may not be working (${elapsed}s)"
    fi

    echo ""
}

EXAMPLES=("basic_app" "iris_pipeline")
PASS=0
FAIL=0

for example in "${EXAMPLES[@]}"; do
    if [ -n "$EXAMPLE_FILTER" ] && [ "$example" != "$EXAMPLE_FILTER" ]; then
        continue
    fi
    if test_example "$example"; then
        PASS=$((PASS + 1))
    else
        FAIL=$((FAIL + 1))
    fi
done

echo "=== Results: $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
