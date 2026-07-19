#!/usr/bin/env bash
# Scheduler-overhead benchmark: barca vs Dagster vs Prefect.
#
# Unlike the rest of the suite (one-shot cold-start timing via hyperfine), this
# measures long-running scheduler DAEMONS across three dimensions:
#   1. Trigger latency  — tick-due → task actually executing, all pinned to
#                         `* * * * *` (the finest cadence all three share → fair).
#   2. Idle footprint   — peak memory of the daemon holding N jobs, at rest.
#   3. Minimum cadence  — finest interval that actually fires. Barca does 1s
#                         (6-field cron); Dagster/Prefect floor at 60s. This axis
#                         is a CAPABILITY comparison, not a latency race.
#
# Runtime warning: dimension 1 waits for real minute boundaries, so it takes
# ~(FIRES+1) minutes PER framework. Override the fire count with $1 (default 3).
#
#   ./bench.sh [FIRES] [IDLE_SECONDS]
set -uo pipefail   # NOT -e: daemons/kills return nonzero by design; handled inline

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
source "$SCRIPT_DIR/../lib/env.sh"
source "$SCRIPT_DIR/lib.sh"

FIRES="${1:-3}"
IDLE_SECONDS="${2:-60}"
CADENCE_SECONDS=20
BARCA_PORT=8290
DAGSTER_PORT=3333

RESULTS_MD="$SCRIPT_DIR/results.md"
RESULTS="$(mktemp)"
export SCHED_RESULTS="$RESULTS"

BARCA_BIN="$REPO_ROOT/.venv/bin/barca"
DAGSTER_BIN="$SCRIPT_DIR/dagster/.venv/bin/dagster"
PREFECT_PY="$SCRIPT_DIR/prefect/.venv/bin/python"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  BENCHMARK: Scheduler overhead (daemon mode)"
echo "  barca serve  vs  dagster dev  vs  prefect .serve()"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
bench_env_banner
echo "  fires sampled: $FIRES   idle window: ${IDLE_SECONDS}s"
echo ""
echo "  FAIRNESS: latency is measured with a 1-minute cron (\`* * * * *\`) — the"
echo "  finest cadence Dagster and Prefect support — so it is apples-to-apples."
echo "  Barca additionally supports SUB-MINUTE (6-field) cron at 1s resolution;"
echo "  that capability is measured separately in dimension 3."
echo ""

# Reset the shared results.md.
: > "$RESULTS_MD"
{
    echo "# Scheduler overhead — results"
    echo ""
    echo "_Fair latency comparison at 1-minute cron; barca's sub-minute capability called out separately._"
    echo ""
} >> "$RESULTS_MD"

# ─── Dimension 1: trigger latency (all pinned to `* * * * *`) ─────────────────
#
# run_latency <label> <start_cmd> <stop_cmd>
run_latency() {
    local label="$1" start_cmd="$2" stop_cmd="$3"
    : > "$RESULTS"
    echo "── latency: $label ──"
    if ! eval "$start_cmd"; then
        echo "  (skipped — $label daemon did not start)"
        eval "$stop_cmd" >/dev/null 2>&1 || true
        echo "| $label | (daemon failed to start) |" >> "$RESULTS_MD"
        return
    fi
    # Wait for FIRES minute-boundaries, plus boot/poll slack.
    sleep $(( (FIRES + 1) * 60 + 20 ))
    eval "$stop_cmd" >/dev/null 2>&1 || true
    local stats
    stats="$(python3 "$SCRIPT_DIR/latency_stats.py" "$RESULTS" "$label")"
    echo "  $stats"
    python3 - "$label" "$stats" >> "$RESULTS_MD" <<'PY'
import json, sys
label, stats = sys.argv[1], json.loads(sys.argv[2])
print(f"| {label} | fires={stats['fires']} min={stats['min_s']}s "
      f"median={stats['median_s']}s p95={stats['p95_s']}s max={stats['max_s']}s |")
PY
}

echo "## 1. Trigger latency (lower = fires closer to the tick)"; echo ""
{ echo "## 1. Trigger latency (seconds past the minute boundary)"; echo ""; echo "| framework | latency |"; echo "|---|---|"; } >> "$RESULTS_MD"

if [[ -x "$BARCA_BIN" ]]; then
    run_latency "barca serve" \
        "$(bench_pin "$SCRIPT_DIR/barca/start.sh $SCRIPT_DIR/barca/latency_job.py $BARCA_PORT")" \
        "$SCRIPT_DIR/barca/stop.sh"
else
    echo "  barca: skipped (no $BARCA_BIN — run 'maturin develop --release')"
fi
if [[ -x "$DAGSTER_BIN" ]]; then
    run_latency "dagster dev" \
        "$(bench_pin "$SCRIPT_DIR/dagster/start.sh $SCRIPT_DIR/dagster/definitions.py $DAGSTER_PORT")" \
        "$SCRIPT_DIR/dagster/stop.sh"
else
    echo "  dagster: skipped (no venv — run 'uv sync' in dagster/)"
fi
if [[ -x "$PREFECT_PY" ]]; then
    run_latency "prefect serve" \
        "$(bench_pin "$SCRIPT_DIR/prefect/start.sh $SCRIPT_DIR/prefect/flow.py")" \
        "$SCRIPT_DIR/prefect/stop.sh"
else
    echo "  prefect: skipped (no venv — run 'uv sync' in prefect/)"
fi
echo "" >> "$RESULTS_MD"

# ─── Dimension 2: idle footprint (peak memory, boot+idle window) ──────────────
echo ""; echo "## 2. Idle footprint (peak memory holding 10 scheduled jobs)"; echo ""
{ echo "## 2. Idle footprint (peak memory, 10 idle jobs)"; echo ""; echo "\`\`\`"; } >> "$RESULTS_MD"
{
    if [[ -x "$BARCA_BIN" ]]; then
        bench_daemon_mem_peak "barca serve" \
            "$(bench_pin "$SCRIPT_DIR/barca/start.sh $SCRIPT_DIR/barca/idle_job.py $BARCA_PORT")" \
            "$SCRIPT_DIR/barca/stop.sh" "$IDLE_SECONDS"
    fi
    if [[ -x "$DAGSTER_BIN" ]]; then
        bench_daemon_mem_peak "dagster dev" \
            "$(bench_pin "$SCRIPT_DIR/dagster/start.sh $SCRIPT_DIR/dagster/idle_definitions.py $DAGSTER_PORT")" \
            "$SCRIPT_DIR/dagster/stop.sh" "$IDLE_SECONDS"
    fi
    if [[ -x "$PREFECT_PY" ]]; then
        bench_daemon_mem_peak "prefect serve" \
            "$(bench_pin "$SCRIPT_DIR/prefect/start.sh $SCRIPT_DIR/prefect/idle_flow.py")" \
            "$SCRIPT_DIR/prefect/stop.sh" "$IDLE_SECONDS"
    fi
} | tee -a "$RESULTS_MD"
echo "\`\`\`" >> "$RESULTS_MD"

# ─── Dimension 3: minimum cadence (barca's sub-minute capability) ─────────────
echo ""; echo "## 3. Minimum cadence — finest interval that actually fires"; echo ""
{ echo ""; echo "## 3. Minimum achievable cadence"; echo ""; echo "| framework | finest cron | fires in ${CADENCE_SECONDS}s |"; echo "|---|---|---|"; } >> "$RESULTS_MD"

barca_cadence_fires="n/a"
if [[ -x "$BARCA_BIN" ]]; then
    : > "$RESULTS"
    if eval "$(bench_pin "$SCRIPT_DIR/barca/start.sh $SCRIPT_DIR/barca/cadence_job.py $BARCA_PORT")"; then
        sleep "$CADENCE_SECONDS"
        "$SCRIPT_DIR/barca/stop.sh" >/dev/null 2>&1 || true
        barca_cadence_fires="$(wc -l < "$RESULTS" | tr -d ' ')"
    fi
    echo "  barca serve   */2 * * * * *  (every 2s)  → ${barca_cadence_fires} fires in ${CADENCE_SECONDS}s"
fi
echo "  dagster dev   * * * * *      (60s floor — sub-minute cron not expressible) → ~0 in ${CADENCE_SECONDS}s"
echo "  prefect serve * * * * *      (60s floor — sub-minute cron not expressible) → ~0 in ${CADENCE_SECONDS}s"
{
    echo "| barca serve | \`*/2 * * * * *\` (2s) | ${barca_cadence_fires} |"
    echo "| dagster dev | \`* * * * *\` (60s floor) | ~0 |"
    echo "| prefect serve | \`* * * * *\` (60s floor) | ~0 |"
    echo ""
    echo "> Barca ticks at **1-second** resolution and accepts 6-field cron; Dagster and"
    echo "> Prefect cannot express any cron finer than **1 minute**."
} >> "$RESULTS_MD"

rm -f "$RESULTS"
echo ""
echo "Results written to $RESULTS_MD"
