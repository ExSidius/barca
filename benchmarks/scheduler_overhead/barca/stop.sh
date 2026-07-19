#!/usr/bin/env bash
# Tear down the `barca serve` started by start.sh. Kill by PID file, then a
# pkill fallback for any stray server on the benchmark's ports.
if [ -f /tmp/barca_sched_bench.pid ]; then
    kill "$(cat /tmp/barca_sched_bench.pid)" 2>/dev/null || true
    rm -f /tmp/barca_sched_bench.pid
fi
pkill -f "barca serve .*scheduler_overhead" 2>/dev/null || true
