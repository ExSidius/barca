#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../lib/env.sh"

# Unlike every other bench.sh here (where $1=runs, $2=warmup), this one has
# a third dimension: N, the fan-out size. Args: $1=runs $2=warmup $3=N
# (N defaults to 10; valid values are 10/50/100, matching the fan_out_N
# targets in barca/assets.py).
N="${3:-10}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK: Parallel fan-out ($N tasks, zero work)"
echo "  Measures pure framework dispatch overhead"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
bench_env_banner
echo ""

hyperfine \
    --warmup "${2:-2}" \
    --runs "${1:-5}" \
    --export-markdown "$SCRIPT_DIR/results.md" \
    --command-name "barca parallel()" "$(bench_pin "$SCRIPT_DIR/barca/run.sh $N")" \
    --command-name "dagster" "$(bench_pin "$SCRIPT_DIR/dagster/run.sh $N")" \
    --command-name "prefect" "$(bench_pin "$SCRIPT_DIR/prefect/run.sh $N")"

# Peak memory (opt-in — set BARCA_BENCH_MEMORY=1; adds an extra untimed run per framework)
[[ "${BARCA_BENCH_MEMORY:-0}" == "1" ]] && bench_mem_report "$SCRIPT_DIR" "$N"
