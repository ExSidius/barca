#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
N="${1:-10}"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK: Parallel fan-out ($N tasks, zero work)"
echo "  Measures pure framework dispatch overhead"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

hyperfine \
    --warmup 2 \
    --runs "${2:-5}" \
    --export-markdown "$SCRIPT_DIR/results.md" \
    --command-name "barca parallel()" "$SCRIPT_DIR/barca/run.sh $N" \
    --command-name "dagster" "$SCRIPT_DIR/dagster/run.sh $N" \
    --command-name "prefect" "$SCRIPT_DIR/prefect/run.sh $N"
