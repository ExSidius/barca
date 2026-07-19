#!/usr/bin/env bash
# Host-side entry point for the Docker benchmark harness. This is what a
# developer actually runs — it builds benchmarks/docker/Dockerfile and runs
# it with a genuinely enforced CPU + memory ceiling shared identically
# across barca, Dagster, and Prefect (see benchmarks/README.md's
# "Reproducible fairness via Docker" section for why this exists alongside
# the native benchmarks/<name>/bench.sh path).
#
# Usage:
#   benchmarks/docker/bench.sh                    # all 21 benchmarks
#   benchmarks/docker/bench.sh trivial chain_100   # a subset
#   benchmarks/docker/bench.sh scheduler_overhead  # the daemon/scheduler one (slow)
#
# Override the resource ceiling with:
#   BARCA_BENCH_CORES=8   # pinned core count (default: 4, same convention
#                          # as benchmarks/lib/env.sh) — translated into a
#                          # --cpuset-cpus=0-N-1 range
#   BARCA_BENCH_MEM=8g     # memory ceiling (default: 4g)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OUT_DIR="$SCRIPT_DIR/out"

BARCA_BENCH_CORES="${BARCA_BENCH_CORES:-4}"
BARCA_BENCH_MEM="${BARCA_BENCH_MEM:-4g}"
CPUSET="0-$((BARCA_BENCH_CORES - 1))"

IMAGE_TAG="barca-bench"

mkdir -p "$OUT_DIR"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Barca benchmark Docker harness"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  cpuset       : $CPUSET ($BARCA_BENCH_CORES cores)"
echo "  memory limit : $BARCA_BENCH_MEM"
echo "  image        : $IMAGE_TAG"
echo "  out dir      : $OUT_DIR"
echo "  benchmarks   : ${*:-<all>}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Always rebuild: correctness over speed. The repo is small and this
# guarantees the image reflects the current worktree rather than a stale
# cached layer from a previous run.
docker build -f "$SCRIPT_DIR/Dockerfile" -t "$IMAGE_TAG" "$REPO_ROOT"

docker run --rm \
    --cpuset-cpus="$CPUSET" \
    --memory="$BARCA_BENCH_MEM" \
    -v "$OUT_DIR:/out" \
    "$IMAGE_TAG" \
    "$@"

echo ""
echo "Results written to $OUT_DIR"
