#!/usr/bin/env bash
# Tests for known cache/staleness gaps.
# These document expected behavior for features not yet fully implemented.
# Failing tests indicate work remaining.
#
# Run: bash tests/integration/test_cache_gaps.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BARCA="$REPO_ROOT/.venv/bin/barca"
PASS=0
FAIL=0
TMPDIR=$(mktemp -d)

pass() { echo "  ✓ $1"; PASS=$((PASS + 1)); }
fail() { echo "  ✗ $1"; FAIL=$((FAIL + 1)); }

cleanup() { rm -rf "$TMPDIR"; }
trap cleanup EXIT

steps() { echo "$1" | python3 -c "import json,sys; print(json.load(sys.stdin).get('steps_executed', -1))" 2>/dev/null; }

# ═══════════════════════════════════════════════════════════════════════════════
# 1. Cross-file cone analysis
# ═══════════════════════════════════════════════════════════════════════════════
echo "=== Cross-file cone analysis ==="

mkdir -p "$TMPDIR/cross_file"
cat > "$TMPDIR/cross_file/helpers.py" << 'PYEOF'
def compute(x):
    return x * 2
PYEOF

cat > "$TMPDIR/cross_file/assets.py" << 'PYEOF'
from barca import asset
from helpers import compute

@asset()
def result() -> dict:
    return {"value": compute(21)}
PYEOF

rm -f "$REPO_ROOT/.barca/metadata.db"

# First run
OUT1=$($BARCA get result "$TMPDIR/cross_file/assets.py" 2>/dev/null || echo '{"steps_executed":-1}')
S1=$(steps "$OUT1")
[ "$S1" = "1" ] && pass "cross-file: first run executes" || fail "cross-file: first run failed (got $S1)"

# Second run — cached
OUT2=$($BARCA get result "$TMPDIR/cross_file/assets.py" 2>/dev/null || echo '{"steps_executed":-1}')
S2=$(steps "$OUT2")
[ "$S2" = "0" ] && pass "cross-file: cached on second run" || fail "cross-file: not cached (got $S2)"

# Modify the helper in the OTHER file
sed -i '' 's/return x \* 2/return x * 99/' "$TMPDIR/cross_file/helpers.py"

# Third run — should detect cross-file change and re-execute
OUT3=$($BARCA get result "$TMPDIR/cross_file/assets.py" 2>/dev/null || echo '{"steps_executed":-1}')
S3=$(steps "$OUT3")
[ "$S3" = "1" ] && pass "cross-file: helper change in other file detected" || fail "cross-file: change not detected (got $S3 steps)"

# ═══════════════════════════════════════════════════════════════════════════════
# 2. Cache eviction / cleanup
# ═══════════════════════════════════════════════════════════════════════════════
echo "=== Cache eviction ==="

cat > "$TMPDIR/evict.py" << 'PYEOF'
from barca import asset

@asset()
def data() -> dict:
    return {"v": 1}
PYEOF

rm -f "$REPO_ROOT/.barca/metadata.db"

# Run 50 times with different code versions — DB should not grow unbounded
for i in $(seq 1 50); do
    sed -i '' "s/\"v\": [0-9]*/\"v\": $i/" "$TMPDIR/evict.py"
    $BARCA get data "$TMPDIR/evict.py" 2>/dev/null > /dev/null
done

# Check DB size: should have at most ~50 rows (one per version), not growing
ROW_COUNT=$(sqlite3 "$REPO_ROOT/.barca/metadata.db" "SELECT count(*) FROM materializations;" 2>/dev/null || echo "0")
# With no eviction, we'd have 50 rows. With eviction, we'd have fewer.
# For now, just verify it doesn't grow exponentially.
[ "$ROW_COUNT" -le 100 ] && pass "eviction: DB has $ROW_COUNT rows (bounded)" || fail "eviction: DB has $ROW_COUNT rows (unbounded growth)"

# Ideally: old versions should be cleaned up (keep only last N per node)
[ "$ROW_COUNT" -le 10 ] && pass "eviction: old versions cleaned (≤10 rows)" || fail "eviction: no cleanup (got $ROW_COUNT rows, expected ≤10)"

# ═══════════════════════════════════════════════════════════════════════════════
# 3. Concurrent access
# ═══════════════════════════════════════════════════════════════════════════════
echo "=== Concurrent access ==="

cat > "$TMPDIR/concurrent.py" << 'PYEOF'
from barca import asset
import time

@asset()
def slow() -> dict:
    time.sleep(0.1)
    return {"done": True}
PYEOF

rm -f "$REPO_ROOT/.barca/metadata.db"

# Run two barca get processes simultaneously (with timeout)
# Known gap: Turso exclusive lock blocks concurrent access.
timeout 5 $BARCA get slow "$TMPDIR/concurrent.py" 2>/dev/null > "$TMPDIR/out1.json" &
PID1=$!
sleep 0.5  # Stagger slightly
timeout 5 $BARCA get slow "$TMPDIR/concurrent.py" 2>/dev/null > "$TMPDIR/out2.json" &
PID2=$!

wait $PID1 2>/dev/null
EXIT1=$?
wait $PID2 2>/dev/null
EXIT2=$?

# Both should succeed (no crash, no corruption)
if [ "$EXIT1" = "0" ] && [ "$EXIT2" = "0" ]; then
    V1=$(python3 -c "import json; print(json.load(open('$TMPDIR/out1.json'))['final_output']['done'])" 2>/dev/null || echo "error")
    V2=$(python3 -c "import json; print(json.load(open('$TMPDIR/out2.json'))['final_output']['done'])" 2>/dev/null || echo "error")
    [ "$V1" = "True" ] && [ "$V2" = "True" ] && pass "concurrent: both produce correct output" || fail "concurrent: output error ($V1, $V2)"
else
    fail "concurrent: Turso lock blocks parallel access (exit $EXIT1, $EXIT2)"
fi

# ═══════════════════════════════════════════════════════════════════════════════
# 4. Sensor/schedule-driven staleness (aspirational)
# ═══════════════════════════════════════════════════════════════════════════════
echo "=== Sensor staleness (aspirational) ==="

cat > "$TMPDIR/sensor.py" << 'PYEOF'
from barca import asset, sensor, Schedule

@sensor(freshness=Schedule("*/5 * * * *"))
def check_inbox():
    return (True, {"files": ["a.csv"]})

@asset(inputs={"inbox": check_inbox})
def process(inbox) -> dict:
    return {"processed": len(inbox)}
PYEOF

rm -f "$REPO_ROOT/.barca/metadata.db"

# Sensors should ALWAYS re-run (they check external state)
OUT1=$($BARCA get process "$TMPDIR/sensor.py" 2>/dev/null || echo '{"steps_executed":-1}')
S1=$(steps "$OUT1")
OUT2=$($BARCA get process "$TMPDIR/sensor.py" 2>/dev/null || echo '{"steps_executed":-1}')
S2=$(steps "$OUT2")

# Sensor should re-run every time (not cached), making downstream stale
# For now: sensor is treated as a regular asset (cached). This test documents the gap.
[ "$S2" = "0" ] && fail "sensor: incorrectly cached (sensors should always re-run)" || pass "sensor: re-runs as expected"

# ═══════════════════════════════════════════════════════════════════════════════
# 5. Per-partition independent invalidation
# ═══════════════════════════════════════════════════════════════════════════════
echo "=== Per-partition invalidation ==="

cat > "$TMPDIR/partition_indep.py" << 'PYEOF'
from barca import asset, partitions

@asset(partitions={"region": partitions(["us", "eu", "ap"])})
def fetch(region: str) -> dict:
    return {"region": region, "data": f"data_for_{region}"}

@asset(inputs={"data": fetch}, partitions={"region": partitions(["us", "eu", "ap"])})
def transform(data: dict, region: str) -> dict:
    return {"region": region, "transformed": data["data"].upper()}
PYEOF

rm -f "$REPO_ROOT/.barca/metadata.db"

# First run: all 6 steps (3 fetch + 3 transform)
OUT1=$($BARCA get transform "$TMPDIR/partition_indep.py" 2>/dev/null || echo '{"steps_executed":-1}')
S1=$(steps "$OUT1")
[ "$S1" = "6" ] && pass "partition: first run executes all 6" || fail "partition: first run got $S1 (expected 6)"

# Second run: all cached
OUT2=$($BARCA get transform "$TMPDIR/partition_indep.py" 2>/dev/null || echo '{"steps_executed":-1}')
S2=$(steps "$OUT2")
[ "$S2" = "0" ] && pass "partition: all cached" || fail "partition: not cached (got $S2)"

# Now: if we could invalidate just ONE partition's upstream without changing the
# function definition... this requires external input changes (not code changes).
# For now, document that code changes invalidate ALL partitions (correct behavior).
sed -i '' 's/data\["data"\].upper()/data["data"].lower()/' "$TMPDIR/partition_indep.py"
OUT3=$($BARCA get transform "$TMPDIR/partition_indep.py" 2>/dev/null || echo '{"steps_executed":-1}')
S3=$(steps "$OUT3")
# All 3 transform steps should re-run (definition changed), fetch steps cached
[ "$S3" = "3" ] && pass "partition: only transform re-runs (fetch cached)" || fail "partition: expected 3 steps, got $S3"

# ═══════════════════════════════════════════════════════════════════════════════
# Summary
# ═══════════════════════════════════════════════════════════════════════════════
echo ""
echo "Results: $PASS passed, $FAIL failed"
echo ""
echo "Expected failures (known gaps):"
echo "  - Cross-file helper detection (cone analysis is same-file only)"
echo "  - Cache eviction (no cleanup implemented)"
echo "  - Sensor always-re-run semantics (sensors treated as regular assets)"
echo ""
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
