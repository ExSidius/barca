#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../lib/env.sh"
N="${1:-10}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK: Parallel fan-out ($N tasks, zero work)"
echo "  Measures pure framework dispatch overhead"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
bench_env_banner
echo ""

hyperfine \
    --warmup 2 \
    --runs "${2:-5}" \
    --export-markdown "$SCRIPT_DIR/results.md" \
    --command-name "barca parallel()" "$(bench_pin "$SCRIPT_DIR/barca/run.sh $N")" \
    --command-name "dagster" "$(bench_pin "$SCRIPT_DIR/dagster/run.sh $N")" \
    --command-name "prefect" "$(bench_pin "$SCRIPT_DIR/prefect/run.sh $N")"

# Peak memory (opt-in — set BARCA_BENCH_MEMORY=1; adds an extra untimed run per framework)
[[ "${BARCA_BENCH_MEMORY:-0}" == "1" ]] && bench_mem_report "$SCRIPT_DIR" "$N"
