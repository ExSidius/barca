#!/usr/bin/env bash
# Boot a Prefect `.serve()` process (schedules + runs in one process) for a
# measurement window.
#   start.sh <flow_file>
# .serve() has no health endpoint, so readiness is a fixed boot grace plus a
# process-alive check. $SCHED_RESULTS is inherited by the in-process flow runs.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
FLOW="${1:?usage: start.sh <flow_file>}"

export PREFECT_HOME=/tmp/prefect_sched_bench
export PREFECT_LOGGING_LEVEL=CRITICAL
mkdir -p "$PREFECT_HOME"
cd "$SCRIPT_DIR"

${BARCA_BENCH_PIN:-} "$SCRIPT_DIR/.venv/bin/python" "$FLOW" \
    >/tmp/prefect_sched_bench.log 2>&1 &
echo $! > /tmp/prefect_sched_bench.pid

# Prefect's ephemeral API + serve loop take a few seconds to come up.
sleep 8
if kill -0 "$(cat /tmp/prefect_sched_bench.pid)" 2>/dev/null; then
    echo "prefect serve running (pid $(cat /tmp/prefect_sched_bench.pid))"
    exit 0
fi
echo "prefect serve failed to start; log:" >&2
cat /tmp/prefect_sched_bench.log >&2 || true
exit 1
