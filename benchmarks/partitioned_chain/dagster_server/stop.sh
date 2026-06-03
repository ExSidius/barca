#!/usr/bin/env bash
kill "$(cat /tmp/dagster_bench.pid 2>/dev/null)" 2>/dev/null || true
pkill -f "dagster dev.*3333" 2>/dev/null || true
