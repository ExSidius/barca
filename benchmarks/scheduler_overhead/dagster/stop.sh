#!/usr/bin/env bash
# Tear down the `dagster dev` started by start.sh (webserver, code server, and
# the schedule daemon it spawns).
if [ -f /tmp/dagster_sched_bench.pid ]; then
    kill "$(cat /tmp/dagster_sched_bench.pid)" 2>/dev/null || true
    rm -f /tmp/dagster_sched_bench.pid
fi
pkill -f "dagster dev" 2>/dev/null || true
pkill -f "dagster.*api grpc" 2>/dev/null || true
pkill -f "dagster-daemon" 2>/dev/null || true
