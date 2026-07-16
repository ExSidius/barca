#!/usr/bin/env bash
# Shared benchmark environment: standardizes CPU and worker-count fairness
# across barca, Dagster, and Prefect so results don't depend on which
# machine (or how many cores it has) the suite happens to run on.
#
# Source this from a bench.sh:
#   SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
#   source "$SCRIPT_DIR/../lib/env.sh"
#
# What it does:
#   - Pins every benchmarked process to a fixed set of cores via `taskset`,
#     so the OS scheduler can't migrate work mid-run and unrelated load on
#     other cores doesn't leak into the measurement.
#   - Exports BARCA_POOL_SIZE (read by the barca CLI) and BARCA_BENCH_WORKERS
#     (read by the Dagster/Prefect benchmark scripts that configure
#     max_concurrent / max_workers) to the *same* value, so no framework is
#     over- or under-subscribed relative to the others.
#   - Prints an environment banner (cores pinned, CPU model, total RAM) so
#     every run's output records what it was measured under.
#
# Override the core count with BARCA_BENCH_CORES (default: 4). Keep this
# at or below the smallest machine you expect to run benchmarks on.

BARCA_BENCH_CORES="${BARCA_BENCH_CORES:-4}"

export BARCA_BENCH_WORKERS="$BARCA_BENCH_CORES"
export BARCA_POOL_SIZE="$BARCA_BENCH_WORKERS"

BARCA_BENCH_CORE_RANGE="0-$((BARCA_BENCH_CORES - 1))"

if command -v taskset >/dev/null 2>&1; then
    BARCA_BENCH_PIN="taskset -c $BARCA_BENCH_CORE_RANGE"
else
    echo "warning: taskset not found — benchmarks will run unpinned" >&2
    BARCA_BENCH_PIN=""
fi

# Wraps a command string with the pinning prefix, for use as a hyperfine
# --command-name argument (hyperfine runs its command strings through a
# shell, so a leading "taskset -c ..." works exactly like at the CLI).
bench_pin() {
    if [[ -n "$BARCA_BENCH_PIN" ]]; then
        echo "$BARCA_BENCH_PIN $*"
    else
        echo "$*"
    fi
}

bench_env_banner() {
    local cpu_model total_ram
    cpu_model="$(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | cut -d: -f2 | sed 's/^ *//')"
    total_ram="$(free -h 2>/dev/null | awk '/^Mem:/ {print $2}')"
    echo "  cores pinned : $BARCA_BENCH_CORES ($BARCA_BENCH_CORE_RANGE)$([[ -z "$BARCA_BENCH_PIN" ]] && echo ' [unpinned: taskset unavailable]')"
    echo "  cpu model    : ${cpu_model:-unknown}"
    echo "  total ram    : ${total_ram:-unknown}"
    echo "  worker count : $BARCA_BENCH_WORKERS (barca pool_size / dagster max_concurrent / prefect max_workers)"
}
