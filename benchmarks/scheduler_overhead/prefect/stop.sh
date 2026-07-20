#!/usr/bin/env bash
# Tear down the Prefect `.serve()` process started by start.sh.
if [ -f /tmp/prefect_sched_bench.pid ]; then
    kill "$(cat /tmp/prefect_sched_bench.pid)" 2>/dev/null || true
    rm -f /tmp/prefect_sched_bench.pid
fi
pkill -f "scheduler_overhead/prefect" 2>/dev/null || true
