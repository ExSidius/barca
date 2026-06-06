#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export BENCH_STATE="$(mktemp -d)"
PREFECT_HOME=/tmp/prefect_resilience PREFECT_LOGGING_LEVEL=CRITICAL "$SCRIPT_DIR/.venv/bin/python" "$SCRIPT_DIR/run.py" 2>/dev/null
