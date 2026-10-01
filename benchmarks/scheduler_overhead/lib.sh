#!/usr/bin/env bash
# Helpers specific to the scheduler-overhead benchmark. Source AFTER
# ../lib/env.sh (this reuses its `_bench_mem_cgroup_base` and `bench_pin`).

# Measure the peak memory of a long-running daemon over a fixed window.
#
# env.sh's bench_mem_peak runs a command to completion; a scheduler daemon never
# exits on its own, so this variant: creates a fresh cgroup, joins it (in a
# subshell), starts the daemon there, holds for <seconds>, stops it, and reads
# the cgroup's peak (whole process tree — daemon + every child it forks).
#
#   bench_daemon_mem_peak <label> <start_cmd> <stop_cmd> <seconds>
#
# <start_cmd> boots the daemon and returns once it's ready (our start.sh scripts
# background the process and return after a readiness check); <stop_cmd> tears it
# down. The reported peak spans boot + idle window (documented in the README).
bench_daemon_mem_peak() {
    local label="$1" start_cmd="$2" stop_cmd="$3" seconds="$4"
    local base kind path cg peak_file peak_bytes

    base="$(_bench_mem_cgroup_base)"
    if [[ -z "$base" ]]; then
        printf '  %-28s memory measurement unavailable (no writable cgroup)\n' "$label:"
        eval "$stop_cmd" >/dev/null 2>&1 || true
        return
    fi
    kind="${base%%:*}"; path="${base#*:}"
    cg="$path/barca-sched-mem-$$-$RANDOM"
    if ! mkdir "$cg" 2>/dev/null; then
        printf '  %-28s memory measurement unavailable (couldn'\''t create cgroup)\n' "$label:"
        eval "$stop_cmd" >/dev/null 2>&1 || true
        return
    fi
    [[ "$kind" == v1 ]] && echo 0 > "$cg/memory.max_usage_in_bytes" 2>/dev/null

    # Join the cgroup in a subshell so the caller's shell is unaffected; then
    # boot the daemon (inherits this cgroup), hold, and stop.
    (
        echo $BASHPID > "$cg/cgroup.procs" 2>/dev/null || exit 0
        eval "$start_cmd" >/dev/null 2>&1 || true
        sleep "$seconds"
        eval "$stop_cmd" >/dev/null 2>&1 || true
    )

    if [[ "$kind" == v1 ]]; then peak_file="$cg/memory.max_usage_in_bytes"; else peak_file="$cg/memory.peak"; fi
    if [[ ! -f "$peak_file" ]]; then
        # Child cgroup exists but has no memory controller (parent's
        # subtree_control lacks +memory) — report unavailable, not a bogus 0 MB.
        printf '  %-28s memory measurement unavailable (cgroup lacks %s)\n' "$label:" "$(basename "$peak_file")"
        sleep 1; rmdir "$cg" 2>/dev/null || true
        return
    fi
    peak_bytes="$(cat "$peak_file" 2>/dev/null || echo 0)"
    sleep 1                      # let the kill drain the cgroup before rmdir
    rmdir "$cg" 2>/dev/null || true
    awk -v l="$label:" -v b="$peak_bytes" \
        'BEGIN{printf "  %-28s %8.1f MB peak (whole process tree, boot+idle window)\n", l, b/1048576}'
}
