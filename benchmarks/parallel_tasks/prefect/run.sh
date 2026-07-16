#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export BENCH_N="${1:-10}"
cd "$SCRIPT_DIR"
"$SCRIPT_DIR/.venv/bin/python" run.py 2>/dev/null
