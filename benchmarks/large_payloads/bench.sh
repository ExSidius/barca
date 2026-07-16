#!/usr/bin/env bash
# Large payloads (5-asset linear chain, 10k rows/step): barca vs dagster vs prefect
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../lib/env.sh"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK: Large payloads (5-asset linear chain, 10k rows/step)"
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
