#!/usr/bin/env bash
# Integration tests for the barca CLI.
# Run: bash tests/integration/test_cli.sh
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

# ─── Test: trivial asset ────────────────────────────────────────────────────
echo "=== Trivial asset ==="

cat > "$TMPDIR/trivial.py" << 'PYEOF'
from barca import asset

@asset()
def hello() -> dict:
    return {"msg": "hello"}
PYEOF

OUTPUT=$($BARCA run "$TMPDIR/trivial.py" 2>/dev/null)
echo "$OUTPUT" | grep -q '"msg":"hello"' && pass "correct output" || fail "wrong output: $OUTPUT"

# ─── Test: linear chain passes values ────────────────────────────────────────
echo "=== Linear chain ==="

cat > "$TMPDIR/chain.py" << 'PYEOF'
from barca import asset

@asset()
def a() -> dict:
    return {"value": 1}

@asset(inputs={"data": a})
def b(data: dict) -> dict:
    return {"value": data["value"] + 10}

@asset(inputs={"data": b})
def c(data: dict) -> dict:
    return {"value": data["value"] * 2}
PYEOF

OUTPUT=$($BARCA run "$TMPDIR/chain.py" 2>/dev/null)
echo "$OUTPUT" | grep -q '"value":22' && pass "chain passes values correctly" || fail "chain result: $OUTPUT"

# ─── Test: user print() visible, protocol hidden ────────────────────────────
echo "=== User print statements ==="

cat > "$TMPDIR/prints.py" << 'PYEOF'
from barca import asset

@asset()
def chatty() -> dict:
    print("VISIBLE_USER_OUTPUT")
    return {"value": 42}

@asset(inputs={"data": chatty})
def consumer(data: dict) -> dict:
    print(f"GOT_{data['value']}")
    return {"result": data["value"] * 2}
PYEOF

OUTPUT=$($BARCA run "$TMPDIR/prints.py" 2>/dev/null)
echo "$OUTPUT" | grep -q "VISIBLE_USER_OUTPUT" && pass "user print visible in stdout" || fail "user print missing"
echo "$OUTPUT" | grep -q "GOT_42" && pass "downstream print visible" || fail "downstream print missing"
echo "$OUTPUT" | grep -q '"result":84' && pass "correct result with prints" || fail "wrong result: $OUTPUT"
echo "$OUTPUT" | grep -q '"node_id"' && fail "PROTOCOL LEAKED to stdout" || pass "protocol hidden from user"

# ─── Test: fan-out parallelism ───────────────────────────────────────────────
echo "=== Fan-out parallelism ==="

cat > "$TMPDIR/fanout.py" << 'PYEOF'
from barca import asset

@asset()
def a() -> dict:
    return {"x": 1}

@asset()
def b() -> dict:
    return {"x": 2}

@asset()
def c() -> dict:
    return {"x": 3}

@asset(inputs={"a": a, "b": b, "c": c})
def merge(a: dict, b: dict, c: dict) -> dict:
    return {"sum": a["x"] + b["x"] + c["x"]}
PYEOF

OUTPUT=$($BARCA run "$TMPDIR/fanout.py" 2>/dev/null)
echo "$OUTPUT" | grep -q '"sum":6' && pass "fan-out merge correct" || fail "fan-out result: $OUTPUT"

# ─── Test: aliased inputs ────────────────────────────────────────────────────
echo "=== Aliased inputs ==="

cat > "$TMPDIR/alias.py" << 'PYEOF'
from barca import asset

@asset()
def raw_prices() -> dict:
    return {"price": 100}

@asset(inputs={"data": raw_prices})
def normalized(data: dict) -> dict:
    return {"normalized_price": data["price"] / 100}
PYEOF

OUTPUT=$($BARCA run "$TMPDIR/alias.py" 2>/dev/null)
echo "$OUTPUT" | grep -q '"normalized_price":1' && pass "aliased input works" || fail "alias result: $OUTPUT"

# ─── Test: multi-file ────────────────────────────────────────────────────────
echo "=== Multi-file ==="

cat > "$TMPDIR/file1.py" << 'PYEOF'
from barca import asset

@asset()
def from_file1() -> dict:
    return {"source": "file1"}
PYEOF

cat > "$TMPDIR/file2.py" << 'PYEOF'
from barca import asset

@asset()
def from_file2() -> dict:
    return {"source": "file2"}
PYEOF

OUTPUT=$($BARCA run "$TMPDIR/file1.py" "$TMPDIR/file2.py" 2>/dev/null)
echo "$OUTPUT" | grep -q "steps_executed.*2\|steps_executed\":2" && pass "multi-file runs both assets" || fail "multi-file result: $OUTPUT"

# ─── Summary ─────────────────────────────────────────────────────────────────
echo ""
echo "Results: $PASS passed, $FAIL failed"
[ "$FAIL" -eq 0 ] && exit 0 || exit 1
