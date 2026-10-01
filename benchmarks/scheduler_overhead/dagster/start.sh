#!/usr/bin/env bash
# Boot `dagster dev` (bundles the schedule-ticking daemon) for a measurement
# window and wait until its GraphQL endpoint answers.
#   start.sh <definitions_file> <port>
# $SCHED_RESULTS is inherited by the launched runs (dagster dev propagates its
# environment to the run subprocesses).
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DEFS="${1:?usage: start.sh <definitions_file> <port>}"
PORT="${2:?usage: start.sh <definitions_file> <port>}"

export DAGSTER_HOME=/tmp/dagster_sched_bench
mkdir -p "$DAGSTER_HOME"
cd "$SCRIPT_DIR"

${BARCA_BENCH_PIN:-} "$SCRIPT_DIR/.venv/bin/dagster" dev -f "$DEFS" -p "$PORT" \
    >/tmp/dagster_sched_bench.log 2>&1 &
echo $! > /tmp/dagster_sched_bench.pid

# dagster dev boots a webserver + code server + daemon — allow up to 60s.
for _ in $(seq 1 60); do
    if curl -s "http://127.0.0.1:$PORT/graphql" \
        -H 'Content-Type: application/json' \
        -d '{"query":"{__typename}"}' >/dev/null 2>&1; then
        echo "dagster dev ready (pid $(cat /tmp/dagster_sched_bench.pid), port $PORT)"
        exit 0
    fi
    sleep 1
done
echo "dagster dev failed to start; log:" >&2
cat /tmp/dagster_sched_bench.log >&2 || true
exit 1
