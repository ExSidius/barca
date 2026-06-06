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

hyperfine \
    --warmup 1 \
    --runs "${1:-5}" \
    --export-markdown "$SCRIPT_DIR/results.md" \
    --command-name "barca (Rust+Python)" "$SCRIPT_DIR/barca/run.sh" \
    --command-name "dagster" "$SCRIPT_DIR/dagster/run.sh" \
    --command-name "prefect" "$SCRIPT_DIR/prefect/run.sh"
