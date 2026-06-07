#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
N="${1:-10}"
rm -rf .barca
barca run "fan_out_${N}" "$SCRIPT_DIR/assets.py" --agent 2>/dev/null
