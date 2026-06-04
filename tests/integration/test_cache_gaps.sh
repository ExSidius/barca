#!/usr/bin/env bash
# Tests for known staleness gaps (core correctness).
# These define expected behavior for features not yet implemented.
# Failing tests = work remaining.
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
#    If asset in file A imports helper from file B, changing B should invalidate A.
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

# First run — executes
OUT1=$($BARCA get result "$TMPDIR/cross_file/assets.py" 2>/dev/null || echo '{"steps_executed":-1}')
S1=$(steps "$OUT1")
[ "$S1" = "1" ] && pass "cross-file: first run executes" || fail "cross-file: first run (got $S1)"

# Second run — cached
OUT2=$($BARCA get result "$TMPDIR/cross_file/assets.py" 2>/dev/null || echo '{"steps_executed":-1}')
S2=$(steps "$OUT2")
[ "$S2" = "0" ] && pass "cross-file: cached on second run" || fail "cross-file: not cached (got $S2)"

# Modify helper in OTHER file
sed -i '' 's/return x \* 2/return x * 99/' "$TMPDIR/cross_file/helpers.py"

# Third run — should detect the cross-file change
OUT3=$($BARCA get result "$TMPDIR/cross_file/assets.py" 2>/dev/null || echo '{"steps_executed":-1}')
S3=$(steps "$OUT3")
[ "$S3" = "1" ] && pass "cross-file: helper change detected" || fail "cross-file: not detected (got $S3)"

# ═══════════════════════════════════════════════════════════════════════════════
# 2. Sensors should ALWAYS re-run (they observe external state)
#    A sensor's output is not deterministic from its code — it depends on the
#    external world. Caching a sensor would miss real changes.
# ═══════════════════════════════════════════════════════════════════════════════
echo "=== Sensor always-re-run ==="

cat > "$TMPDIR/sensor.py" << 'PYEOF'
from barca import asset, sensor

@sensor()
def check_inbox():
    return (True, {"files": ["a.csv"]})

@asset(inputs={"inbox": check_inbox})
def process(inbox) -> dict:
    _, data = inbox
    return {"processed": len(data["files"])}
PYEOF

rm -f "$REPO_ROOT/.barca/metadata.db"

# First run
OUT1=$($BARCA get process "$TMPDIR/sensor.py" 2>/dev/null || echo '{"steps_executed":-1}')
S1=$(steps "$OUT1")
[ "$S1" -ge 1 ] && pass "sensor: first run executes" || fail "sensor: first run (got $S1)"

# Second run — sensor should STILL re-run (not cached), making downstream stale
OUT2=$($BARCA get process "$TMPDIR/sensor.py" 2>/dev/null || echo '{"steps_executed":-1}')
S2=$(steps "$OUT2")
[ "$S2" -ge 1 ] && pass "sensor: re-runs on second call (not cached)" || fail "sensor: incorrectly cached (got $S2 steps — should be >0)"

# ═══════════════════════════════════════════════════════════════════════════════
# 3. Per-partition independent invalidation
#    Changing the transform function should NOT re-run fetch (different function).
#    Changing fetch should NOT re-run unmodified partitions (same definition).
# ═══════════════════════════════════════════════════════════════════════════════
echo "=== Per-partition invalidation ==="

cat > "$TMPDIR/partitions.py" << 'PYEOF'
from barca import asset, partitions

@asset(partitions={"region": partitions(["us", "eu", "ap"])})
def fetch(region: str) -> dict:
    return {"region": region, "data": f"raw_{region}"}

@asset(inputs={"data": fetch}, partitions={"region": partitions(["us", "eu", "ap"])})
def transform(data: dict, region: str) -> dict:
    return {"region": region, "result": data["data"].upper()}
PYEOF

rm -f "$REPO_ROOT/.barca/metadata.db"

# First run: all 6 steps (3 fetch + 3 transform)
OUT1=$($BARCA get transform "$TMPDIR/partitions.py" 2>/dev/null || echo '{"steps_executed":-1}')
S1=$(steps "$OUT1")
[ "$S1" = "6" ] && pass "partition: first run = 6 steps" || fail "partition: first run (got $S1)"

# Second run: fully cached
OUT2=$($BARCA get transform "$TMPDIR/partitions.py" 2>/dev/null || echo '{"steps_executed":-1}')
S2=$(steps "$OUT2")
[ "$S2" = "0" ] && pass "partition: fully cached" || fail "partition: not cached (got $S2)"

# Modify ONLY transform — fetch should stay cached (3 re-runs, not 6)
sed -i '' 's/data\["data"\].upper()/data["data"].lower()/' "$TMPDIR/partitions.py"
OUT3=$($BARCA get transform "$TMPDIR/partitions.py" 2>/dev/null || echo '{"steps_executed":-1}')
S3=$(steps "$OUT3")
[ "$S3" = "3" ] && pass "partition: only transform re-runs (fetch cached)" || fail "partition: expected 3, got $S3"

# Revert transform, modify ONLY fetch — transform should re-run too (upstream stale)
cat > "$TMPDIR/partitions.py" << 'PYEOF'
from barca import asset, partitions

@asset(partitions={"region": partitions(["us", "eu", "ap"])})
def fetch(region: str) -> dict:
    return {"region": region, "data": f"CHANGED_{region}"}

@asset(inputs={"data": fetch}, partitions={"region": partitions(["us", "eu", "ap"])})
def transform(data: dict, region: str) -> dict:
    return {"region": region, "result": data["data"].upper()}
PYEOF
OUT4=$($BARCA get transform "$TMPDIR/partitions.py" 2>/dev/null || echo '{"steps_executed":-1}')
S4=$(steps "$OUT4")
[ "$S4" = "6" ] && pass "partition: fetch change cascades to transform (6 steps)" || fail "partition: fetch cascade (got $S4)"

# ═══════════════════════════════════════════════════════════════════════════════
echo ""
echo "Results: $PASS passed, $FAIL failed"
echo ""
if [ "$FAIL" -gt 0 ]; then
    echo "Known gaps remaining:"
    echo "  - Cross-file: cone analysis only traces same-file deps"
    echo "  - Sensor: currently cached like regular assets"
    echo ""
fi
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
