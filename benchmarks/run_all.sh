#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUNS=${1:-3}
CORES=$(nproc)

echo "============================================"
echo "  Orchestrator Benchmark Suite"
echo "  $(date)"
echo "  $(uname -m) / ${CORES} cores"
echo "  Runs per test: $RUNS"
echo "============================================"
echo ""

# ── Benchmark 1a: Trivial ──
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK 1a: 500 trivial jobs (zero work)"
echo "  Measures pure framework overhead"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

echo "── Barca -j 1 (sequential) ──"
cd "$SCRIPT_DIR/barca_bench"
python bench_trivial.py "$RUNS" 1
echo ""

echo "── Barca -j $CORES (default: $CORES cores) ──"
python bench_trivial.py "$RUNS"
echo ""

echo "── Prefect (64 threads, 1 process) ──"
cd "$SCRIPT_DIR/prefect_bench"
PREFECT_HOME=/tmp/prefect_bench PREFECT_LOGGING_LEVEL=ERROR .venv/bin/python bench_trivial.py "$RUNS" 2>/dev/null
echo ""

echo "── Dagster (sequential, in-process) ──"
cd "$SCRIPT_DIR/dagster_bench"
DAGSTER_HOME=/tmp/dagster_bench .venv/bin/python bench_trivial.py "$RUNS" 2>/dev/null
echo ""

# ── Benchmark 1b: 50ms work ──
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK 1b: 500 jobs x 50ms work each"
echo "  Simulates real-world I/O (API calls, DB queries)"
echo "  Sequential minimum: 500 * 50ms = 25.0s"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

echo "── Barca -j 1 (sequential) ──"
cd "$SCRIPT_DIR/barca_bench"
python bench.py "$RUNS" 1
echo ""

echo "── Barca -j $CORES (default: $CORES cores) ──"
python bench.py "$RUNS"
echo ""

echo "── Prefect (64 threads, 1 process) ──"
cd "$SCRIPT_DIR/prefect_bench"
PREFECT_HOME=/tmp/prefect_bench PREFECT_LOGGING_LEVEL=ERROR .venv/bin/python bench.py "$RUNS" 2>/dev/null
echo ""

echo "── Dagster (sequential, in-process) ──"
cd "$SCRIPT_DIR/dagster_bench"
DAGSTER_HOME=/tmp/dagster_bench .venv/bin/python bench.py "$RUNS" 2>/dev/null
echo ""

# ── Benchmark 2: Cold Start ──
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK 2: Cold start (single trivial asset)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

echo "── Barca (reset + reindex + materialize) ──"
cd "$SCRIPT_DIR/barca_bench"
python bench_cold_start.py "$RUNS"
echo ""

echo "── Prefect (flow + 1 task, warm server) ──"
cd "$SCRIPT_DIR/prefect_bench"
PREFECT_HOME=/tmp/prefect_cold PREFECT_LOGGING_LEVEL=ERROR .venv/bin/python bench_cold_start.py "$RUNS" 2>/dev/null
echo ""

echo "── Dagster (materialize 1 asset, warm modules) ──"
cd "$SCRIPT_DIR/dagster_bench"
DAGSTER_HOME=/tmp/dagster_cold .venv/bin/python bench_cold_start.py "$RUNS" 2>/dev/null
echo ""

# ── Benchmark 3: Server Job Pickup ──
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK 3: Server job pickup latency (Barca only)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
cd "$SCRIPT_DIR/barca_bench"
python bench_pickup.py 5
echo ""

echo "============================================"
echo "  Benchmark suite complete"
echo "============================================"
