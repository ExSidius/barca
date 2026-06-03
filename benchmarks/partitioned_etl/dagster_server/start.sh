#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export DAGSTER_HOME=/tmp/dagster_server_bench
mkdir -p "$DAGSTER_HOME"
cd "$SCRIPT_DIR"
"$SCRIPT_DIR/.venv/bin/dagster" dev -f definitions.py -p 3333 &
echo $! > /tmp/dagster_bench.pid
for i in $(seq 1 30); do
  curl -s http://localhost:3333/graphql -H "Content-Type: application/json" -d '{"query":"{__typename}"}' > /dev/null 2>&1 && echo "Ready" && exit 0
  sleep 1
done
echo "Failed to start"
exit 1
