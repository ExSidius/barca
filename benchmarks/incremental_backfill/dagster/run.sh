#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
for i in $(seq 1 10); do
  DAGSTER_HOME=/tmp/dagster_backfill "$SCRIPT_DIR/.venv/bin/python" "$SCRIPT_DIR/run.py" 2>/dev/null
done
