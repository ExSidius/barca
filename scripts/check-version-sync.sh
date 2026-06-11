#!/usr/bin/env bash
# Verify all version strings are consistent across the project.
set -euo pipefail

CORE=$(grep '^version' crates/barca-core/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
CLI=$(grep '^version' crates/barca-cli/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
SERVER=$(grep '^version' crates/barca-server/Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
PY_TOML=$(grep '^version' pyproject.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
PY_INIT=$(grep '__version__' python/barca/__init__.py | sed 's/.*"\(.*\)".*/\1/')

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
