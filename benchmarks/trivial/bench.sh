#!/usr/bin/env bash
# Trivial single-asset benchmark: barca vs dagster vs prefect
# Measures pure framework overhead (asset does zero work)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK: Single trivial asset (zero work)"
echo "  Measures pure framework overhead"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

hyperfine \
    --warmup 3 \
    --runs "${1:-10}" \
    --export-markdown "$SCRIPT_DIR/results.md" \
    --command-name "barca (Rust+Python)" "$SCRIPT_DIR/barca/run.sh" \
    --command-name "dagster" "$SCRIPT_DIR/dagster/run.sh" \
    --command-name "prefect" "$SCRIPT_DIR/prefect/run.sh"
