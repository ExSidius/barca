# `barca.record()` — attaching telemetry to a materialization

**Status:** draft — decisions below are proposals; see [Open questions](#open-questions).

Why recording metrics from user code is shaped the way it is. Same format as
`user-api-decisions.md`: each decision, the alternatives with concrete syntax,
and the reasoning.

## Problem

Day to day, a node's page should answer: is it healthy, when does it run next,
how long does it usually take, and **what happened inside it** — rows dropped,
model accuracy, rows rejected by a validator, remaining API quota. Barca already
measures what it can see from outside (wall time, CPU time, peak RSS, artifact
size). It has no way to carry numbers only the user's code knows.

## The API

```python
import barca
from barca import asset, task

@asset()
def ibp_model() -> pd.DataFrame:
    raw = load()
    df = raw.dropna(subset=["ppg"])
    barca.record(rows_in=len(raw), rows_dropped=len(raw) - len(df))
    return df

@task(inputs={"df": ibp_model})
def validate_ibp_model(df: pd.DataFrame) -> None:
    bad = df[df.units < 0]
    barca.record(negative_units=len(bad))
    assert bad.empty, f"{len(bad)} rows with negative units"
```

One function: `barca.record(**values)`. Keyword arguments are metric names;
values are scalars.

## Decisions

### A call inside the function, not a change to its return value

**Decision:** Metrics are recorded with a function call from anywhere inside
the node's execution. The return value is untouched.

**Alternatives rejected:**

```python
# A: Wrap the return value (Dagster's MaterializeResult / Output(metadata=))
@asset()
def ibp_model() -> Recorded[pd.DataFrame]:
    df = ...
    return Recorded(df, rows_dropped=n)
```

```python
# B: Declare metrics on the decorator, computed from the output
@asset(metrics=lambda df: {"rows": len(df)})
def ibp_model() -> pd.DataFrame: ...
```

```python
# C: Return a (value, metrics) tuple
@asset()
def ibp_model() -> tuple[pd.DataFrame, dict]:
    return df, {"rows_dropped": n}
```

**Why:**

- **A and C** change what the function returns, which breaks the
  "decorators are no-ops" contract: calling `ibp_model()` in a notebook or a
  unit test now hands you a wrapper or a tuple instead of the DataFrame.
  Annotating one more metric means touching the return statement and every
  caller's expectations.
- **B** can only see the output. The interesting numbers are usually
  intermediate (`rows_dropped` is a property of the *input* minus the output)
  and B can't express them. It's also a lambda inside a decorator, which
  the static parser would have to either ignore or evaluate.
- A call is additive: add it, remove it, or call it from a helper three
  levels down, and the function's signature and return value never change.

### Scalars only

**Decision:** Values must be `int`, `float`, `bool`, or `str`. No `None`, no
nested lists or dicts, no NaN or ±inf. Anything else raises `TypeError` /
`ValueError` at the call site.

**Alternative rejected:** arbitrary JSON-serializable values (a dict of
per-column null counts, a list of offending ids).

**Why:** Every scalar is either *chartable* (numbers, bools as 0/1) or
*displayable* (short strings). Nested values have no obvious rendering and turn
metrics into a dumping ground. If it's structured data, it belongs in an asset's
output — that's what assets are for. NaN and ±inf are rejected because they
have no JSON encoding and no sensible place on a chart.

### Validation happens even when running standalone

**Decision:** Outside a barca worker, `record()` validates its arguments and
then does nothing.

**Why:** Decorated functions must run unchanged in a notebook or a test. A
silent no-op keeps that working; validating anyway means a bad call
(`record(cols=df.columns)`) fails in the notebook rather than first failing in
production.

### Last value wins per key; keys merge across calls

**Decision:** Within one materialization, `record(a=1); record(b=2)` records
both; `record(a=1); record(a=5)` records `a=5`.

**Alternative rejected:** append every call as a time series within the run.

**Why:** The unit of history is the materialization. A per-run series
("progress over time") is a different feature with a different UI — see
[Out of scope](#out-of-scope). Last-wins means a loop that updates a running
count ends with the final count, which is what's wanted.

### Recorded values survive failures

**Decision:** Values recorded before an exception are persisted against the
failed attempt.

**Why:** "Read 40,000 rows, then crashed" is the most useful thing to know about
a failure. Validators are the sharpest case: `validate_ibp_model` above records
`negative_units=12` and *then* fails its assert. Losing the number on failure
would lose exactly the information the failure is about.

### Works in assets, tasks, and sensors

**Decision:** Any node kind can record.

**Why:** Validators are tasks, and they're the main source of "how bad is it"
numbers. Sensors can usefully record what they saw (`new_files=3`).

### Stored in a dedicated table, written only by Rust

**Decision:** The worker sends values over the existing Unix socket; the
coordinator buffers them per step and persists them with the run. Python never
touches the DB. This is the same path as captured stdout in the UI branch
(#100): message type, per-run buffer, persist after `persist_run`.

```sql
CREATE TABLE IF NOT EXISTS metrics (
    run_id     TEXT NOT NULL,
    node_id    TEXT NOT NULL,   -- display id: partitions get their own series
    key        TEXT NOT NULL,
    value_num  REAL,            -- int / float / bool (as 0/1)
    value_text TEXT,            -- str
    created_at TEXT DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_metrics_series ON metrics(node_id, key, created_at);
```

**Alternative rejected:** a `metrics_json` column on `materializations`.

**Why:** The questions worth asking are trends across runs *by key* — "how has
`rows_dropped` moved over the last 30 runs?" With a table that's one indexed
query; with a JSON column it's `json_extract` over every row. Splitting numbers
from text at write time also tells the UI what to chart without inspecting
values. Buffering with the run's other results means the shared-state replay
path (state-push conflict → pull → replay ledger) re-persists metrics too.

### Barca's own measurements stay separate

**Decision:** Wall time, CPU time, peak RSS, and artifact size stay in their
existing `materializations` columns and are not written into `metrics`. User
keys can't collide with them, and the UI presents them as two groups:
*measured by barca* and *recorded by you*.

**Why:** Different provenance, different guarantees. Barca's numbers exist for
every materialization; recorded ones exist only where the user asked.

### Cached materializations show the metrics that produced them

**Decision:** When `get` reuses a cached artifact, nothing new is recorded. The
"current" metrics for a fresh asset are the ones from the materialization that
produced the artifact in use — not merely the most recent row.

**Why:** If an asset was materialized under code version 2, then version 1 was
restored and served from cache, the most recent row describes an artifact
nobody is using. Tying metrics to the artifact keeps "what am I looking at"
honest.

### Limits

**Decision:** Per materialization, at most 64 keys. Keys match
`[A-Za-z_][A-Za-z0-9_.]*` and are at most 64 characters. String values are at
most 1024 characters. Violations raise at the call site.

**Why:** One bad loop (`record(**{f"row_{i}": ...})`) shouldn't be able to put a
million rows in the metadata DB. Raising rather than truncating keeps the rule
obvious and deterministic — it fails on the first run, not silently forever.

## Reading it back

- **UI:** a node's detail panel shows the latest recorded values next to barca's
  measurements, with a sparkline per numeric key over recent materializations.
- **HTTP:** the asset-state payload carries each node's current recorded values;
  `GET /assets/{name}/metrics?key=…&limit=…` returns a series.
- **CLI:** `barca stats <asset>` prints current recorded values (and includes
  them under `--json`).
- **Python client:** `barca.stats(...)` includes them.

## Wire protocol

New worker → coordinator message, sent once per `record()` call:

```json
{"type": "record", "node_id": "pipeline.py:ibp_model", "values": {"rows_dropped": 12, "source": "ibp"}}
```

The worker sets the currently executing node id before calling the user's
function (the step payload carries it; #100's log capture uses the same
hook), so `record()` needs no arguments beyond the values. Threads started
by user code inside a step share the worker's current node id, since a worker
executes one step at a time.

## Testing plan

Written before the implementation.

- **Python:** argument validation (types, NaN/inf, key pattern, limits);
  standalone no-op still validates; message shape over a fake socket.
- **Rust:** protocol round-trip; per-step buffering with last-wins merge;
  persistence; replay on state-push conflict.
- **Integration:** an asset that records → values readable via `stats`; a step
  that records then raises → values kept on the failed attempt; partitioned
  asset → one series per partition; a cache hit records nothing new and
  `stats` still shows the producing materialization's values.

## Out of scope

- **Progress / within-run time series** (`record` called in a loop, charted
  live). Logs already stream live; a structured progress API is a separate
  design.
- **Thresholds and alerts** (`rows_dropped > 100` fails or warns). Belongs with
  a checks/expectations design, which validators-as-tasks partly cover today.
- **Rich values** — plots, tables, markdown. Those are outputs; make them assets.

## Open questions

1. **Name.** `barca.record(...)` vs `barca.metric(...)` vs `barca.log_metrics(...)`.
   `record` reads well and doesn't suggest logging; `metric` is singular-sounding
   for a multi-key call. *Leaning: `record`.*
2. **`parallel()` children.** A `@task` dispatched through `parallel()` runs in
   another worker as its own item. Do its `record()` calls attach to the child
   item or roll up to the node that called `parallel()`? Rolling up matches
   "this node did this", but merging children's keys under last-wins is
   arbitrary. *Leaning: attach to the child; the parent records its own summary
   if it wants one.*
3. **Units.** Worth an optional unit convention (`record(latency_ms=…)` by naming
   vs. a `units=` side channel) so the UI can format values? *Leaning: naming
   convention only; no API.*
4. **Retention.** Metrics grow by keys × materializations forever, like
   `materializations` and `logs` today. Defer to a general retention policy, or
   cap per series now? *Leaning: defer; same policy for all three tables.*
5. **Automatic dataframe shape.** Should barca auto-record `rows`/`columns` for
   DataFrame-like outputs at serialization time (no user call), as part of
   *measured by barca*? Cheap and broadly useful, but it's a separate change.
   *Leaning: yes, as a follow-up.*
