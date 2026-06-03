#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DAGSTER_HOME=/tmp/dagster_deep_diamond "$SCRIPT_DIR/.venv/bin/python" "$SCRIPT_DIR/run.py" 2>/dev/null
