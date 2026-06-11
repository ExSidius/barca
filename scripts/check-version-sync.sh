#!/usr/bin/env bash
# Verify all version strings are consistent across the project.
set -euo pipefail

extract() {
    local file="$1" pattern="$2"
    local val
    val=$(grep "$pattern" "$file" | head -1 | sed 's/.*"\(.*\)".*/\1/' || true)
    if [ -z "$val" ]; then
        echo "ERROR: could not extract version from $file (pattern: $pattern)" >&2
        exit 1
    fi
    echo "$val"
}

CORE=$(extract crates/barca-core/Cargo.toml '^version')
CLI=$(extract crates/barca-cli/Cargo.toml '^version')
SERVER=$(extract crates/barca-server/Cargo.toml '^version')
PY_TOML=$(extract pyproject.toml '^version')
PY_INIT=$(extract python/barca/__init__.py '__version__')

ERRORS=0
for name_ver in "barca-cli:$CLI" "barca-server:$SERVER" "pyproject.toml:$PY_TOML" "__init__.py:$PY_INIT"; do
    name="${name_ver%%:*}"
    ver="${name_ver#*:}"
    if [ "$ver" != "$CORE" ]; then
        echo "Version mismatch: barca-core=$CORE but $name=$ver"
        ERRORS=1
    fi
done
if [ "$ERRORS" -eq 1 ]; then exit 1; fi
echo "All versions consistent: $CORE"
