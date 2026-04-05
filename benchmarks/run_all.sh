#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUNS=${1:-3}
CORES=$(nproc)
BARCA_VENV="$REPO_ROOT/.venv/bin"

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
PATH="$BARCA_VENV:$PATH" python bench_trivial.py "$RUNS" 1
echo ""

echo "── Barca -j $CORES ($CORES threads, free-threaded Python) ──"
PATH="$BARCA_VENV:$PATH" python bench_trivial.py "$RUNS" "$CORES"
echo ""

echo "── Barca -j 64 (64 threads, free-threaded Python) ──"
PATH="$BARCA_VENV:$PATH" python bench_trivial.py "$RUNS" 64
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
PATH="$BARCA_VENV:$PATH" python bench.py "$RUNS" 1
echo ""

echo "── Barca -j $CORES ($CORES threads, free-threaded Python) ──"
PATH="$BARCA_VENV:$PATH" python bench.py "$RUNS" "$CORES"
echo ""

echo "── Barca -j 64 (64 threads, free-threaded Python) ──"
PATH="$BARCA_VENV:$PATH" python bench.py "$RUNS" 64
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
PATH="$BARCA_VENV:$PATH" python bench_cold_start.py "$RUNS"
echo ""

echo "── Prefect (flow + 1 task) ──"
cd "$SCRIPT_DIR/prefect_bench"
PREFECT_HOME=/tmp/prefect_cold PREFECT_LOGGING_LEVEL=ERROR .venv/bin/python bench_cold_start.py "$RUNS" 2>/dev/null
echo ""

echo "── Dagster (materialize 1 asset) ──"
cd "$SCRIPT_DIR/dagster_bench"
DAGSTER_HOME=/tmp/dagster_cold .venv/bin/python bench_cold_start.py "$RUNS" 2>/dev/null
echo ""

# ── Benchmark 3: Spaceflights Pipeline ──
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK 3: Spaceflights (10-asset diamond DAG)"
echo "  Adapted from Kedro spaceflights starter"
echo "  3 sources → 3 preps → merge → split → train → eval"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

echo "── Barca -j 1 (sequential) ──"
cd "$SCRIPT_DIR/barca_bench"
PATH="$BARCA_VENV:$PATH" python bench_spaceflights.py "$RUNS" 1
echo ""

echo "── Barca -j $CORES ($CORES threads, free-threaded Python) ──"
PATH="$BARCA_VENV:$PATH" python bench_spaceflights.py "$RUNS" "$CORES"
echo ""

echo "── Prefect (8 threads, 1 process) ──"
cd "$SCRIPT_DIR/prefect_bench"
PREFECT_HOME=/tmp/prefect_sf PREFECT_LOGGING_LEVEL=ERROR .venv/bin/python bench_spaceflights.py "$RUNS" 2>/dev/null
echo ""

echo "── Dagster (sequential, in-process) ──"
cd "$SCRIPT_DIR/dagster_bench"
DAGSTER_HOME=/tmp/dagster_sf .venv/bin/python bench_spaceflights.py "$RUNS" 2>/dev/null
echo ""

# ── Benchmark 4: Server Benchmarks ──
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK 4a: Server startup time"
echo "  Time from process start to first successful API response"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

echo "── Barca (uvicorn + FastAPI) ──"
cd "$SCRIPT_DIR/barca_bench"
PATH="$BARCA_VENV:$PATH" python bench_server.py "$RUNS" startup
echo ""

echo "── Prefect (prefect server start) ──"
cd "$SCRIPT_DIR/prefect_bench"
PREFECT_HOME=/tmp/prefect_srv PREFECT_LOGGING_LEVEL=ERROR PREFECT_API_URL=http://127.0.0.1:4200/api .venv/bin/python bench_server.py "$RUNS" startup 2>/dev/null
echo ""

echo "── Dagster (dagster dev) ──"
cd "$SCRIPT_DIR/dagster_bench"
DAGSTER_HOME=/tmp/dagster_srv .venv/bin/python bench_server.py "$RUNS" startup 2>/dev/null
echo ""

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK 4b: Server API latency"
echo "  Time from HTTP request to materialization complete"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

echo "── Barca (POST /assets/{id}/refresh) ──"
cd "$SCRIPT_DIR/barca_bench"
PATH="$BARCA_VENV:$PATH" python bench_server.py "$RUNS" refresh
echo ""

echo "── Barca (POST /reconcile) ──"
PATH="$BARCA_VENV:$PATH" python bench_server.py "$RUNS" reconcile
echo ""

echo "── Prefect (flow run via server) ──"
cd "$SCRIPT_DIR/prefect_bench"
PREFECT_HOME=/tmp/prefect_srv PREFECT_LOGGING_LEVEL=ERROR PREFECT_API_URL=http://127.0.0.1:4200/api .venv/bin/python bench_server.py "$RUNS" flow 2>/dev/null
echo ""

echo "── Dagster (GraphQL materialization) ──"
cd "$SCRIPT_DIR/dagster_bench"
DAGSTER_HOME=/tmp/dagster_srv .venv/bin/python bench_server.py "$RUNS" refresh 2>/dev/null
echo ""

echo "============================================"
echo "  Benchmark suite complete"
echo "============================================"
