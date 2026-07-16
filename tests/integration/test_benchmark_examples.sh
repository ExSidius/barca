#!/usr/bin/env bash
# Runs every benchmark's barca/run.sh as a correctness smoke test.
#
# These aren't the barca-vs-Dagster-vs-Prefect timing comparisons (that's
# benchmarks/*/bench.sh, which needs hyperfine + per-framework venvs and
# isn't CI-friendly) — this only exercises barca's own side of each
# benchmark, asserting it runs to completion. That's cheap (no external
# framework deps, seconds per benchmark) and, unlike the timing comparisons,
# belongs in the default pipeline: the benchmark suite is the widest set of
# real DAG shapes and features (dynamic + static partitions, fan-out/fan-in,
# multi-file discovery, sinks, retries, parallel(), large payloads, ...)
# barca is exercised against, so it doubles as a correctness sweep. A
# partitioned_etl-shaped regression (partitions_from + inputs={} silently
# dropping a dependency edge) would have failed here immediately instead of
# going unnoticed because nothing exercised that combination end to end.
#
# Excluded: *_server (persistent Dagster/Prefect process) and airflow
# variants — those test the *other* frameworks, not barca; partitioned_10k
# (docker-based, different structure, no barca/run.sh); timeseries_1000
# (2002-asset scale benchmark for the perf profiler, no run.sh — not part
# of the barca/Dagster/Prefect comparison suite).
#
# Run: bash tests/integration/test_benchmark_examples.sh
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BARCA="${REPO_ROOT}/.venv/bin/barca"
[ -x "$BARCA" ] || { echo "error: $BARCA not found — build + maturin develop first" >&2; exit 1; }

PASS=0
FAIL=0
pass() { echo "  ✓ $1"; PASS=$((PASS + 1)); }
fail() { echo "  ✗ $1"; FAIL=$((FAIL + 1)); }

for run_sh in "$REPO_ROOT"/benchmarks/*/barca/run.sh; do
    bench_dir="$(basename "$(dirname "$(dirname "$run_sh")")")"
    workdir="$(mktemp -d)"
    OUT=$(cd "$workdir" && bash "$run_sh" 2>&1)
    STATUS=$?
    rm -rf "$workdir"
    if [ "$STATUS" -eq 0 ]; then
        pass "$bench_dir"
    else
        fail "$bench_dir (exit $STATUS): $OUT"
    fi
done

echo ""
echo "Results: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
