#!/usr/bin/env bash
# `barca run` cache semantics: upstream assets cache-aware by default,
# --refresh <names> selective, --refresh-all / --no-cache for the whole cone.
#
# Run: bash tests/integration/test_run_refresh.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BARCA="${BARCA:-${REPO_ROOT}/.venv/bin/barca}"
[ -x "$BARCA" ] || BARCA="$(command -v barca)"
PASS=0
FAIL=0
TMPDIR=$(mktemp -d)

pass() { echo "  ✓ $1"; PASS=$((PASS + 1)); }
fail() { echo "  ✗ $1"; FAIL=$((FAIL + 1)); }
cleanup() { rm -rf "$TMPDIR"; }
trap cleanup EXIT

steps() { echo "$1" | python3 -c "import json,sys; print(json.load(sys.stdin).get('steps_executed', -1))"; }
check() { # name expected actual
  if [ "$3" = "$2" ]; then pass "$1 (steps=$3)"; else fail "$1: expected $2 steps, got $3"; fi
}

cat > "$TMPDIR/pipe.py" << 'PYEOF'
from barca import asset, task

@asset()
def raw() -> dict:
    return {"v": 1}

@asset(inputs={"d": raw})
def clean(d: dict) -> dict:
    return {"v": d["v"] + 1}

@task(inputs={"d": clean})
def validate(d: dict) -> dict:
    return d
PYEOF

cd "$TMPDIR"
echo "=== barca run refresh semantics ==="

"$BARCA" run validate pipe.py > /dev/null          # cold: raw + clean + validate
check "warm run executes task only" 1 "$(steps "$("$BARCA" run validate pipe.py)")"
check "--refresh clean re-runs clean + task" 2 "$(steps "$("$BARCA" run validate pipe.py --refresh clean)")"
check "--refresh raw,clean re-runs both + task" 3 "$(steps "$("$BARCA" run validate pipe.py --refresh raw,clean)")"
check "--refresh-all re-runs whole cone" 3 "$(steps "$("$BARCA" run validate pipe.py --refresh-all)")"
check "--no-cache equals --refresh-all" 3 "$(steps "$("$BARCA" run validate pipe.py --no-cache)")"

echo ""
echo "Passed: $PASS  Failed: $FAIL"
[ "$FAIL" -eq 0 ]
