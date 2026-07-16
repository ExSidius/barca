#!/usr/bin/env bash
# Multi-file discovery (98 assets across 50 files): barca vs dagster vs prefect
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../lib/env.sh"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK: Multi-file discovery (98 assets across 50 files)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
bench_env_banner
echo ""

hyperfine \
    --warmup "${2:-1}" \
    --runs "${1:-5}" \
    --export-markdown "$SCRIPT_DIR/results.md" \
    --command-name "barca (Rust+Python)" "$(bench_pin "$SCRIPT_DIR/barca/run.sh")" \
    --command-name "dagster" "$(bench_pin "$SCRIPT_DIR/dagster/run.sh")" \
    --command-name "prefect" "$(bench_pin "$SCRIPT_DIR/prefect/run.sh")"

# Peak memory (opt-in — set BARCA_BENCH_MEMORY=1; adds an extra untimed run per framework)
[[ "${BARCA_BENCH_MEMORY:-0}" == "1" ]] && bench_mem_report "$SCRIPT_DIR"
