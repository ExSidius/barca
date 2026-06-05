# First-Time User Experience Test

Run this to evaluate barca's UX from a fresh install. Hand it to an agent or run it yourself.

## Setup

```bash
cd /tmp
rm -rf barca-ux-test
mkdir barca-ux-test && cd barca-ux-test
uv init --no-readme
uv add barca
```

## Test Scenarios

### 1. Install

**Run:**
```bash
uv add barca
```

**What to look for:**
- Single `uv add` installs both the CLI binary and Python package.
- No build errors or missing dependencies.
- `barca --version` prints `barca 0.1.2`.

### 2. Help

**Run:**
```bash
barca --help
barca run --help
barca get --help
```

**What to look for:**
- Top-level help shows `run`, `get`, `plan`, `version` subcommands.
- `run --help` shows `--output` flag with `json`, `value`, `pretty` modes.
- `get --help` shows `target` positional, `--output` flag, and `files` positional.

### 3. Trivial pipeline

**Run:**
```bash
cat > pipeline.py << 'EOF'
from barca import asset

@asset()
def raw():
    return {"price": 100, "quantity": 5}

@asset(inputs={"data": raw})
def total(data):
    return {"total": data["price"] * data["quantity"]}
EOF

barca run pipeline.py
barca run pipeline.py --output value
barca run pipeline.py --output pretty
```

**Expected output:**

`--output json` (default): One-line JSON with `elapsed_seconds`, `steps_executed`, `phases`, `final_output`.
```json
{"elapsed_seconds":0.123,"steps_executed":2,"phases":1,"final_output":{"total":500}}
```

`--output value`: Pretty-printed final value only.
```json
{
  "total": 500
}
```

`--output pretty`: Human-friendly summary with timing.
```
Executed 2 step(s) in 0.123s (1 phase)

Result:
{
  "total": 500
}
```

### 4. Python API

**Run:**
```bash
cat > test_api.py << 'EOF'
import barca

result = barca.run("pipeline.py")
print("run result:", result)

value = barca.get("total", "pipeline.py")
print("get value:", value)

plan = barca.plan("pipeline.py")
print("plan:", plan)
EOF

python test_api.py
```

**What to look for:**
- `barca.run()` returns a dict with `elapsed_seconds`, `steps_executed`, `phases`, `final_output`.
- `barca.get()` returns the deserialized value directly (e.g. `{"total": 500}`).
- `barca.plan()` returns a dict with `total_steps` and `phases`.

### 5. Error messages

**Run:**
```bash
# File not found
barca run nonexistent.py

# Syntax error
cat > bad.py << 'EOF'
from barca import asset

@asset()
def broken():
    return {
EOF
barca run bad.py

# Runtime error
cat > divzero.py << 'EOF'
from barca import asset

@asset()
def oops():
    return 1 / 0
EOF
barca run divzero.py

# Asset not found
barca get nonexistent pipeline.py
```

**What to look for:**
- File not found: error includes the file path (`nonexistent.py`).
- Syntax error: error includes the file name and points to the syntax issue.
- Runtime error: traceback shows user code frames, NOT `_worker.py` internals.
- Asset not found: error names the missing asset and lists available ones.
- No error has a doubled "Error: Error:" prefix.

### 6. Partitions

**Run:**
```bash
cat > partitioned.py << 'EOF'
from barca import asset, partitions

@asset(partitions={"region": partitions(["us", "eu", "ap"])})
def fetch(region):
    return {"region": region, "count": len(region) * 10}

@asset(inputs={"data": fetch})
def transform(data):
    return {"processed": data["region"], "doubled": data["count"] * 2}
EOF

barca run partitioned.py --output pretty
barca get fetch partitioned.py --output value
```

**What to look for:**
- Run executes 6 steps (3 fetch + 3 transform).
- `get fetch` returns the first partition (alphabetically), not the last.
- Output format is correct for each `--output` mode.

### 7. Non-JSON types

**Run:**
```bash
cat > types.py << 'EOF'
from barca import asset

@asset()
def an_int():
    return 42

@asset()
def a_list():
    return [1, 2, 3]

@asset()
def a_set():
    return {1, 2, 3}
EOF

python -c "import barca; print(repr(barca.get('an_int', 'types.py')))"
python -c "import barca; print(repr(barca.get('a_list', 'types.py')))"
python -c "import barca; print(repr(barca.get('a_set', 'types.py')))"
```

**What to look for:**
- `an_int` returns Python `int` 42, not `{"value": 42}`.
- `a_list` returns Python `list` `[1, 2, 3]`.
- `a_set` returns Python `set` `{1, 2, 3}`.

## Grading Rubric

| Scenario | Good | Current | Notes |
|---|---|---|---|
| 1. Install | Single `uv add`, <5s | OK | |
| 2. Help | Clear subcommands, flags | OK | |
| 3. Trivial pipeline | All 3 output modes work | OK | |
| 4. Python API | run/get/plan all return correct types | OK | |
| 5. Error messages | File path, no doubled prefix, no internals | OK | |
| 6. Partitions | First partition, correct step count | OK | |
| 7. Non-JSON types | int/list/set round-trip correctly | OK | |

## Known Rough Edges

- `barca run` with a syntax error in the Python file may show a raw parse error from ruff rather than a friendly message.
- Partition output order depends on sort order of partition keys; multi-dimensional partitions use alphabetical key order.
- DataFrame round-tripping requires `barca[parquet]` extra (`uv add barca[parquet]`).
- `barca get` on a partitioned asset returns only one partition value (the first), not all.

## Improvement Log

| Date | Version | What changed |
|---|---|---|
| 2026-06-04 | 0.1.2 | Added `--output` modes, `barca version`, cleaned error messages, partition first-not-last |
