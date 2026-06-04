#!/usr/bin/env bash
if [ -f /tmp/dagster_bench.pid ]; then
  kill "$(cat /tmp/dagster_bench.pid)" 2>/dev/null || true
  rm -f /tmp/dagster_bench.pid
fi
# Kill any remaining dagster processes
pkill -f "dagster dev.*3333" 2>/dev/null || true
