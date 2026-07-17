#!/usr/bin/env bash
# Baked into the barca-bench image as its ENTRYPOINT. Runs inside the
# container at /work. Accepts benchmark names as $@; with no args, runs the
# fixed list below (every benchmark under benchmarks/ that has a bench.sh,
# confirmed via `find benchmarks -mindepth 2 -maxdepth 2 -name bench.sh`).
#
# For each benchmark: prints a header, cds into benchmarks/$name, runs
# BARCA_BENCH_MEMORY=1 ./bench.sh with each bench.sh's own default
# runs/warmup, tees stdout+stderr to /out/$name.log, and copies the
# hyperfine-generated results.md (if present) to /out/$name-results.md.
# One benchmark failing doesn't stop the rest; failures are recorded in
# /out/SUMMARY.txt.

set -u

ALL_BENCHMARKS=(
    trivial
    chain_100
    deep_diamond
    etl_duckdb
    etl_duckdb_dataframes
    fan_out_500
    fan_out_500_50ms
    incremental_backfill
    large_payloads
    map_reduce
    mixed_io_cpu
    multi_file_discovery
    parallel_tasks
    partitioned_chain
    partitioned_etl
    partitioned_fan_in
    resilience_pileup
    spaceflights
    wide_join
    wide_layers
)

if [[ $# -gt 0 ]]; then
    BENCHMARKS=("$@")
else
    BENCHMARKS=("${ALL_BENCHMARKS[@]}")
fi

mkdir -p /out

SUCCEEDED=()
FAILED=()

for name in "${BENCHMARKS[@]}"; do
    echo ""
    echo "=== $name ==="

    dir="/work/benchmarks/$name"
    log="/out/$name.log"

    if [[ ! -d "$dir" || ! -f "$dir/bench.sh" ]]; then
        echo "  no such benchmark (missing $dir/bench.sh)" | tee "$log"
        FAILED+=("$name (no bench.sh)")
        continue
    fi

    set +e
    (
        cd "$dir" && BARCA_BENCH_MEMORY=1 ./bench.sh
    ) >"$log" 2>&1
    status=$?
    set -e

    if [[ $status -eq 0 ]]; then
        SUCCEEDED+=("$name")
    else
        FAILED+=("$name (exit $status)")
    fi
    set -u

    # Surface the log to the container's own stdout too, so `docker run`
    # without a detach still shows live progress.
    cat "$log"

    if [[ -f "$dir/results.md" ]]; then
        cp "$dir/results.md" "/out/$name-results.md"
    fi
done

{
    echo "Barca benchmark Docker harness — run summary"
    echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo ""
    echo "Succeeded (${#SUCCEEDED[@]}):"
    for n in "${SUCCEEDED[@]:-}"; do
        [[ -n "$n" ]] && echo "  - $n"
    done
    echo ""
    echo "Failed (${#FAILED[@]}):"
    for n in "${FAILED[@]:-}"; do
        [[ -n "$n" ]] && echo "  - $n"
    done
} >/out/SUMMARY.txt

cat /out/SUMMARY.txt

# Don't fail the container just because some benchmarks failed — the whole
# point is to run the full suite and report which ones didn't work, not to
# stop partway through.
exit 0
