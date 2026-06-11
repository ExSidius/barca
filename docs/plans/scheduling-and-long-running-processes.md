# Architecture Decision: Scheduling, Sensors, and Long-Running Processes

## The Question

Barca needs a long-running process to enforce freshness (cron schedules, sensor polling, staleness propagation). Should this be a standalone daemon, embedded in `barca serve`, or both?

## What Exists Today

- `barca get`/`barca run` — one-shot, spawn workers, write to DB, exit
- `barca serve` — axum HTTP server with async runs, `run_mutex` serialization, `DashMap` run tracking, file watch for DAG invalidation
- `Freshness` enum (`Always`/`Manual`/`Schedule(CronExpr)`) — **parsed but not enforced**
- Sensor `(update_detected, output)` tuple — **consumed but not used for invalidation**
- SQLite via Turso in WAL mode — handles concurrent multi-process access
- Worker pools are per-invocation (each gets its own UDS socket)
- The spec (`docs/workflows/05`) says `barca run` is a "long-running process" with a reconciliation loop

**What does NOT exist:** cron evaluation logic, staleness state machine, per-node state persistence, sensor invalidation propagation, file-change-triggered re-parsing.

## Recommendation: Shared Reconciler, Two Entry Points

The scheduling loop is a **core engine capability**, not a transport feature. Extract it as a `Reconciler` in `barca-core`. Both a new CLI command and the server consume it.

```
barca-core/
  reconciler.rs    ← NEW: the scheduling loop
  staleness.rs     ← NEW: per-node state machine
  cron.rs          ← NEW: cron expression evaluation

barca-cli          → adds `barca up` (uses reconciler)
barca-server       → embeds reconciler alongside HTTP handlers
```

Three modes:

| Command | What it does | Who it's for |
|---------|-------------|--------------|
| `barca get` | One-shot. Unchanged. Always works independently. | Everyone, CI |
| `barca up pipeline.py` | Starts the reconciler. No HTTP. Just keeps the graph fresh. | Solo devs |
| `barca serve pipeline.py` | Reconciler + HTTP API. One process for everything. | Teams, programmatic access |

### Why not server-only?

A web server is **visible infrastructure**. It binds a port, requires firewall awareness, exposes a network surface. For a solo dev who just wants `Schedule("0 5 * * *")` to work, requiring `barca serve` violates "invisible" and "DuckDB of orchestration." You shouldn't need a web server to get cron scheduling.

### Why not daemon-only?

The server already exists. Teams that need both HTTP and scheduling should not run two processes. The server should eventually embed the same scheduling loop.

### Why both?

- **For individuals:** `barca up` — background process, no ports, invisible
- **For teams:** `barca serve` — one process provides HTTP and scheduling
- **For CI:** `barca get` — unchanged one-shot behavior, no dependency on running daemons
- **For the codebase:** Reconciler is tested once, consumed twice

## Multi-Process Coordination

The core technical challenge: what happens when `barca up` is running and the user also does `barca get`?

### Recommendation: Cooperative PID + SQLite serialization

**Long-running processes (singleton):**
- On start, write `.barca/coordinator.pid` with PID + mode
- On start, check if PID file exists and whether that PID is alive:
  - Alive → refuse to start ("another barca process is running on PID N")
  - Dead → remove stale file, proceed
- Use `flock()` on the PID file for atomic create-or-fail (prevents race)

**One-shot commands (always independent):**
- `barca get` and `barca run` do NOT check the PID file. They always proceed.
- SQLite WAL mode serializes writes at the file level — battle-tested, this is how every SQLite desktop app works.
- Worker pools are independent (each gets its own UDS socket path).
- Two concurrent executions work correctly, just serialize DB writes.

**No delegation in phase 1.** CLI and reconciler both write to DB independently. The reconciler avoids re-executing nodes that CLI already freshened (by checking `last_run_at` in the state table). CLI commands are always valid. Delegation (CLI → reconciler via UDS) is optimization for later.

This is the DuckDB model: multiple connections, each handles its own transactions, the storage engine serializes.

## The Reconciler

A struct in `barca-core` with a single concern: maintaining the graph at declared freshness levels.

### Each tick (default: 1 second):

1. **Re-parse DAG if source files changed** (mtime check or notify event)
2. **Evaluate per-node staleness** in topo order
3. **Cron evaluation**: has a tick elapsed since last successful run?
4. **Sensor `update_detected` propagation**: `true` → downstream stale, `false` → no change
5. **Collect runnable-stale nodes**, build execution plan
6. **Dispatch via existing coordinator/io_loop**
7. **Record results, propagate state changes**

If no nodes are runnable, the tick is a no-op — pure in-memory computation, sub-millisecond. If a pass is already running when the next tick fires, skip it (passes don't overlap, per spec).

### State machine (per node):

```
fresh → stale_waiting_for_schedule → runnable_stale → running → fresh / failed
                                   ↗
stale_waiting_for_upstream ────────
```

### New DB table: `node_state`

```sql
CREATE TABLE IF NOT EXISTS node_state (
    node_id TEXT PRIMARY KEY,
    state TEXT NOT NULL,
    definition_hash TEXT,
    last_run_hash TEXT,
    last_run_at TEXT,
    last_checked_at TEXT,
    stale_reason TEXT,
    updated_at TEXT DEFAULT (datetime('now'))
);
```

Survives restarts — reconciler reads it on startup to resume from where it left off.

## Naming: `barca up`

Short, imperative, intuitive — "bring the graph up to freshness."

- `barca up pipeline.py` — foreground (Ctrl-C to stop)
- `barca up pipeline.py --detach` — background, writes PID
- `barca up --stop` — kills background process

Rejected alternatives: `watch` (implies file-watching only), `daemon` (sounds like infrastructure), `reconcile` (exposes implementation).

## Phasing

### Phase 1: Reconciler + `barca up`
- New core modules: `reconciler.rs`, `staleness.rs`, `cron.rs`
- New DB table: `node_state`
- New CLI: `barca up <files> [--detach] [--interval <seconds>]`
- Reuses `commands::execute()` for execution (rebuilds DAG per pass — correct but not optimal)
- PID file coordination

### Phase 2: Embed in server
- `barca serve` starts reconciler alongside HTTP
- New routes: `GET /state`, `GET /state/{node_id}`
- `run_mutex` serializes reconciler passes with HTTP-triggered runs

### Phase 3: Incremental execution
- Persistent DAG cache in reconciler (invalidated by file changes)
- Warm worker pool (avoid spawn/teardown per pass)
- Decompose `commands::execute()` monolith

### Phase 4: CLI awareness
- `barca get` optionally delegates to running reconciler via UDS
- Pure optimization — standalone always works

## Risks

| Risk | Mitigation |
|------|-----------|
| Reconciler crash leaves stale PID | Check PID liveness with `kill(pid, 0)`, not just file existence |
| Two reconcilers racing at startup | `flock()` on PID file for atomic create-or-fail |
| Clock drift affects cron | Monotonic time for intervals, wall clock only for cron |
| Phase 1 rebuilds DAG every pass | Acceptable — most ticks are no-ops; user code dominates when work happens |
| SQLite contention CLI + reconciler | WAL mode handles it; Turso default busy timeout (5s) is sufficient |

## What This Does NOT Decide

- Cron library choice (implementation detail)
- Reconciler internal async architecture (implementation detail)
- UI for scheduling state (separate concern)
- Distributed scheduling (future problem, single-machine only for now)
