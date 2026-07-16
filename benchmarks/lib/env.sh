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

# ─── Memory (opt-in) ─────────────────────────────────────────────────────────
#
# hyperfine only measures wall time, so peak memory is a separate, optional
# pass: set BARCA_BENCH_MEMORY=1 and call bench_mem_report after the hyperfine
# block in a bench.sh. It's opt-in because it re-runs each framework once more
# (untimed) and adds noticeable wall-clock, especially for slow-starting
# frameworks.
#
# barca is multi-process by design (Rust coordinator + N Python workers), so
# a naive wrapper that only samples the directly-execed process — e.g.
# `/usr/bin/time -v` — would report only the coordinator's own footprint and
# make barca look artificially light next to Dagster/Prefect's single
# process. bench_mem_peak instead runs the command inside a fresh, empty
# cgroup and reads back that cgroup's peak-memory counter, which sums every
# process that ever lived in it — parent or descendant, however many times
# the tree forks. Falls back to `/usr/bin/time -v` (single-process only,
# clearly labeled) if no cgroup memory controller is writable, and reports
# "unavailable" rather than a misleading number if neither works.

_bench_mem_cgroup_base() {
    # v2 unified hierarchy: only usable if "memory" is actually among the
    # enabled controllers (a hybrid v1+v2 mount can expose an empty/unrelated
    # v2 hierarchy — e.g. cpuset/hugetlb only — alongside v1 memory).
    if [[ -f /sys/fs/cgroup/cgroup.controllers ]] \
        && grep -qw memory /sys/fs/cgroup/cgroup.controllers \
        && [[ -w /sys/fs/cgroup/cgroup.subtree_control ]]; then
        echo "v2:/sys/fs/cgroup"
        return
    fi
    if [[ -f /sys/fs/cgroup/unified/cgroup.controllers ]] \
        && grep -qw memory /sys/fs/cgroup/unified/cgroup.controllers \
        && [[ -w /sys/fs/cgroup/unified/cgroup.subtree_control ]]; then
        echo "v2:/sys/fs/cgroup/unified"
        return
    fi
    if [[ -d /sys/fs/cgroup/memory ]]; then
        local own
        own="$(awk -F: '$2=="memory"{print $3}' /proc/self/cgroup 2>/dev/null)"
        if [[ -n "$own" && -w "/sys/fs/cgroup/memory$own" ]]; then
            echo "v1:/sys/fs/cgroup/memory$own"
            return
        fi
    fi
    echo ""
}

bench_mem_peak() {
    local label="$1"
    local cmd="$2"
    local base kind path cg peak_bytes peak_file join_marker

    base="$(_bench_mem_cgroup_base)"
    if [[ -z "$base" ]]; then
        if command -v /usr/bin/time >/dev/null 2>&1; then
            local kb
            kb="$(/usr/bin/time -v bash -c "$cmd" 2>&1 >/dev/null | grep -oE 'Maximum resident set size \(kbytes\): [0-9]+' | grep -oE '[0-9]+$')"
            if [[ -n "$kb" ]]; then
                awk -v l="$label:" -v kb="$kb" 'BEGIN{printf "  %-24s %8.1f MB peak (top process only — no cgroup access, undercounts multi-process frameworks)\n", l, kb/1024}'
            else
                echo "  $label: memory measurement unavailable"
            fi
        else
            echo "  $label: memory measurement unavailable (no cgroup access, no /usr/bin/time)"
        fi
        return
    fi

    kind="${base%%:*}"
    path="${base#*:}"
    cg="$path/barca-bench-mem-$$-$RANDOM"
    if ! mkdir "$cg" 2>/dev/null; then
        echo "  $label: memory measurement unavailable (couldn't create cgroup)"
        return
    fi
    [[ "$kind" == v1 ]] && echo 0 > "$cg/memory.max_usage_in_bytes" 2>/dev/null

    # Written by the subshell only if it actually joins the cgroup before
    # exec-ing the command — lets us tell "measured, genuinely ~0 bytes"
    # apart from "never joined, ran outside the cgroup unmeasured" instead
    # of both silently reading back as a 0 MB peak.
    join_marker="$(mktemp)"
    ( echo $BASHPID > "$cg/cgroup.procs" 2>/dev/null && echo ok > "$join_marker" && exec bash -c "$cmd" >/dev/null 2>&1 )
    if [[ ! -s "$join_marker" ]]; then
        rm -f "$join_marker"
        rmdir "$cg" 2>/dev/null
        echo "  $label: memory measurement unavailable (couldn't join cgroup)"
        return
    fi
    rm -f "$join_marker"

    if [[ "$kind" == v1 ]]; then
        peak_file="$cg/memory.max_usage_in_bytes"
    else
        peak_file="$cg/memory.peak"
    fi
    if [[ ! -f "$peak_file" ]]; then
        rmdir "$cg" 2>/dev/null
        echo "  $label: memory measurement unavailable (kernel/cgroup lacks $(basename "$peak_file"))"
        return
    fi
    peak_bytes="$(cat "$peak_file" 2>/dev/null || echo 0)"
    rmdir "$cg" 2>/dev/null

    awk -v l="$label:" -v b="$peak_bytes" 'BEGIN{printf "  %-24s %8.1f MB peak (whole process tree, cgroup)\n", l, b/1048576}'
}

# Convenience wrapper: reports peak memory for barca/dagster/prefect's
# run.sh in the calling bench.sh's SCRIPT_DIR, each pinned the same way the
# timed runs are. Call after the hyperfine block, guarded by
# BARCA_BENCH_MEMORY=1. Extra args (e.g. an N for parallel_tasks) are passed
# through to every run.sh so the memory pass exercises the same workload
# size as the timed runs.
bench_mem_report() {
    local script_dir="$1"; shift
    local extra_args="$*"
    echo ""
    echo "Peak memory (opt-in, BARCA_BENCH_MEMORY=1):"
    bench_mem_peak "barca (Rust+Python)" "$(bench_pin "$script_dir/barca/run.sh $extra_args")"
    bench_mem_peak "dagster" "$(bench_pin "$script_dir/dagster/run.sh $extra_args")"
    bench_mem_peak "prefect" "$(bench_pin "$script_dir/prefect/run.sh $extra_args")"
}
