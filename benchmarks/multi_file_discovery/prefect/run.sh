#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PREFECT_HOME=/tmp/prefect_multi_file_discovery PREFECT_LOGGING_LEVEL=CRITICAL "$SCRIPT_DIR/.venv/bin/python" "$SCRIPT_DIR/run.py" 2>/dev/null
