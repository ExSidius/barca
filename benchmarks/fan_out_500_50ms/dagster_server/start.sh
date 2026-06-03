#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export DAGSTER_HOME=/tmp/dagster_server_bench
mkdir -p "$DAGSTER_HOME"
cd "$SCRIPT_DIR"
# Start dagster dev in background, wait for it to be ready
"$SCRIPT_DIR/.venv/bin/dagster" dev -f definitions.py -p 3333 &
DAGSTER_PID=$!
echo "$DAGSTER_PID" > /tmp/dagster_bench.pid
# Wait for server to respond
for i in $(seq 1 30); do
  if curl -s http://localhost:3333/graphql -H "Content-Type: application/json" -d '{"query":"{__typename}"}' > /dev/null 2>&1; then
    echo "Dagster server ready (pid $DAGSTER_PID)"
    exit 0
  fi
  sleep 1
done
echo "Dagster server failed to start"
exit 1
