#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
FILES=$(find "$SCRIPT_DIR/project" -name "*.py" ! -name "__init__.py" | sort)
"$REPO_ROOT/.venv/bin/barca" get $FILES --no-cache 2>/dev/null
