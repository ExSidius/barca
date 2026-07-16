#!/usr/bin/env bash
# Resilience / anti-pileup benchmark: barca vs dagster vs prefect.
#
# Workload: 8 independent healthy 2-asset chains + one "poison" chain whose head
# fails twice (with backoff) before recovering. The question is whether a single
# flaky asset stalls the whole run.
#
#   - barca:   retries are Rust-owned; healthy chains run in-process and the
#              backing-off chain holds no worker slot → wall-clock ≈ max(work, backoff).
#   - dagster/prefect (script/sequential mode): the backing-off asset serializes
#              with everything else → wall-clock ≈ sum(work) + backoff.
#
# Each framework needs its own .venv (see per-dir pyproject.toml), e.g.:
#   for d in dagster prefect; do (cd $d && uv venv && uv pip install -e .); done
# barca uses the workspace .venv (maturin develop --release).
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "$SCRIPT_DIR/../lib/env.sh"

bench_env_banner
echo ""

hyperfine \
    --warmup 1 \
    --runs "${1:-5}" \
    --export-markdown "$SCRIPT_DIR/results.md" \
    --command-name "barca (Rust+Python)" "$(bench_pin "$SCRIPT_DIR/barca/run.sh")" \
    --command-name "dagster" "$(bench_pin "$SCRIPT_DIR/dagster/run.sh")" \
    --command-name "prefect" "$(bench_pin "$SCRIPT_DIR/prefect/run.sh")"

# Peak memory (opt-in — set BARCA_BENCH_MEMORY=1; adds an extra untimed run per framework)
[[ "${BARCA_BENCH_MEMORY:-0}" == "1" ]] && bench_mem_report "$SCRIPT_DIR"
