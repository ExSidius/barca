# Architecture Decisions (v0.2.0)

This document records the key design decisions in barca's execution engine,
what alternatives were tried, and why we settled on the current approach.

## 1. Unix domain sockets for worker coordination

### The decision

Workers communicate with the Rust coordinator via **length-prefixed JSON frames
over Unix domain sockets (UDS)**. Each worker maintains a persistent connection
for the duration of its lifetime.

### What we tried

**v0.1.x: Stderr JSON protocol.** Workers wrote `BARCA:2:{json}` lines to stderr.
Rust read them line-by-line. This was simple but one-directional — the coordinator
could send work via batch files but couldn't communicate back to a running worker.
This made `parallel()` impossible without a separate mechanism.

**v0.2.0 first attempt: Per-worker sockets with round-robin polling.** Each worker
had a dedicated UDS. The coordinator polled each connection with 1ms timeouts in a
loop. At N workers, one scan took ~N milliseconds. This scaled to N=100 but hung at
N>100 because the polling latency exceeded the rate workers completed tasks.

**v0.2.0 final: Task-per-connection with mpsc channels.** Each UDS connection gets a
dedicated tokio task that bridges the socket to an mpsc channel. The coordinator
reads from a single channel receiver — O(1) per message regardless of pool size.
Proven at 290K msg/s with 128 workers.

### Why UDS

- **Bidirectional**: Workers can send results AND receive new commands on the same
  connection. Essential for the pull-based scheduling model and `parallel()`.
- **Zero-copy on macOS/Linux**: UDS doesn't go through the network stack. Kernel
  copies data directly between process address spaces.
- **No serialization framework needed**: Length-prefixed JSON is simple to implement
  in both Rust and Python, debuggable with standard tools.
- **Per-message overhead ~7μs**: Measured via our socket-stress test. For any task
  doing real work (>1ms), the protocol overhead is invisible.

### What we rejected

- **Shared memory / mmap**: Fast but complex. Would require a custom serialization
  format and careful synchronization. Not worth it when UDS overhead is already <10μs.
- **gRPC / HTTP**: Heavy for local IPC. Adds protobuf/HTTP framing overhead and
  dependency complexity for no benefit over UDS.
- **Named pipes (FIFOs)**: Unidirectional per pipe. Would need two pipes per worker,
  doubling the file descriptor usage for no gain over UDS.

---

## 2. Stateless workers with a global ready queue

### The decision

Workers are **stateless executors**. They finish a task, report back to Rust, and
Rust assigns the next task from a global ready set. No task is pre-assigned to any
worker.

### What we tried

**Pre-assigned per-worker queues.** The original coordinator had `Vec<VecDeque<ItemId>>`
— one queue per worker. Items were distributed round-robin at load time. This created
problems:

- **Head-of-line blocking**: If worker 0's first task was slow, its queued tasks
  starved even though workers 1-15 were idle.
- **Reshuffling complexity**: When a worker called `parallel()` and got suspended,
  its remaining queued tasks needed to be redistributed. This required deadlock
  detection, temp worker spawning, and complex queue management.
- **No scheduling intelligence**: Round-robin assignment couldn't account for cache
  locality, task priority, or runtime estimates.

### Why a global ready queue with Rust-driven assignment

- **Zero waste**: No idle worker has an empty queue while another worker has a full one.
  Every worker always executes the highest-priority available task.
- **Natural backpressure**: If one task is slow, Rust assigns more tasks to idle workers.
  No reshuffling needed.
- **Simple parallel()**: When a worker calls `parallel()`, its children enter the
  global ready set. Any idle worker picks them up. No redistribution.
- **Future-proof**: The ready queue is the natural place to add scheduling heuristics
  (cache locality, priority, estimated duration) without changing the worker model.

---

## 3. SIGSTOP/SIGCONT for parallel()

### The decision

When a worker calls `parallel()`, the Rust coordinator:

1. **SIGSTOP**s the worker process (freezes it, zero CPU, full state preserved)
2. **Spawns a temp replacement** worker to maintain pool capacity
3. Adds child items to the global ready queue
4. When all children complete: kills the temp, **SIGCONT**s the original, sends results

### What we tried

**Coordinator suspension model.** The original design had `suspended: HashMap<usize, GroupId>`
tracking which workers were waiting for parallel groups. A `check_deadlock()` function
detected when all workers were suspended and spawned temp workers. This was complex:

- Deadlock detection had edge cases (what if only some workers are suspended?)
- Temp workers needed their own queue slots
- The `advance_worker` / `WakeWorker` / `ResumeWorker` action types added coordinator
  complexity
- With pre-assigned queues, a suspended worker's tasks were stranded

**Inline execution.** For pool_size=1, the worker could execute parallel children
itself (no round-trip to Rust). This works but limits parallelism to one process.

### Why SIGSTOP/SIGCONT

- **Zero CPU while frozen**: A SIGSTOP'd process uses zero CPU but retains all
  state in memory. It resumes exactly where it left off.
- **Maintains pool capacity**: By spawning a temp replacement, the active worker
  count stays at `pool_size`. No throughput loss during parallel dispatch.
- **Recursive nesting**: If a temp worker also calls `parallel()`, the same
  mechanism applies recursively. Frozen processes stack; active pool always equals
  `pool_size`.
- **No coordinator complexity**: The coordinator doesn't need to know about
  workers at all. It just tracks items and their states. The I/O loop handles
  all process management.
- **Clean resource accounting**: Frozen process count = nesting depth. Each
  uses zero CPU. The operating system handles all the scheduling.

### Limitations

- **Unix-only**: SIGSTOP/SIGCONT is a Unix signal. On Windows, this mechanism
  would need a different implementation (e.g., SuspendThread/ResumeThread).
- **Memory**: Frozen processes retain their full memory footprint. Deep nesting
  with large in-memory datasets could use significant RAM.

---

## 4. Type-safe plan-to-coordinator bridge

### The decision

The coordinator has a `load_phase(phase, provided_inputs)` method that consumes
a planner `Phase` directly. Every `Item` carries a `StepId` — the planner's
canonical identity. No intermediate string-based mapping.

### The problem we solved

The original bridge used two runtime `HashMap<String, ItemId>` maps:

```rust
let mut item_node_ids: HashMap<ItemId, String> = HashMap::new();
let mut node_to_item: HashMap<String, ItemId> = HashMap::new();
```

Dependencies were resolved by string lookup:

```rust
// Silent drop if upstream_id not in map!
if let Some(&upstream_item) = node_to_item.get(upstream_id) {
    deps.push(...);
}
```

This caused real bugs:
- **Missing outputs** (`final_output: null`) — output collection didn't match
  because the coordinator's branch-suffixed node_ids didn't match the planner's
  step_ids.
- **Progress undercounting** — callbacks fired for some steps but not others.
- **Failures not propagating** — failed items were recorded in the coordinator
  but never checked by commands.rs, so the process exited with code 0.

All of these were silent — no error, no panic, just wrong results.

### The step accounting invariant

Every step the system knows about **must** reach a terminal state (done, failed,
or skipped). This is enforced at two levels:

1. **Static steps** (from planner): `load_phase()` adds items and returns a count.
   `commands.rs` asserts this count equals the plan's step count. Any mismatch is
   a programming error — panic.

2. **Dynamic steps** (from `parallel()`): The `ParallelGroup` tracks
   `completed_count` which must equal `items.len()` before the group resolves.
   The frozen parent is never SIGCONT'd until this condition is met.

---

## 5. Rust for planning, Python for execution

### The decision

Barca's Rust binary handles: parsing, DAG construction, execution planning, cache
checking, worker lifecycle, and database persistence. Python workers handle: user
function execution, data serialization, and parallel dispatch requests.

### Why not all-Python

The planning phase must be **invisible** — sub-100ms for typical workloads, including
parsing, hashing, cache lookup, and plan generation. Python's interpreter startup
alone is ~30ms. By doing planning in Rust:

- Parse 2002 assets in 21ms (ruff's parser)
- Plan in <1ms
- Per-step dispatch overhead: 0.4ms

The Rust binary adds ~4ms of fixed overhead. Python adds ~18ms per worker process
spawn. For a 100-step pipeline, the total orchestration overhead is ~22ms — less
than a single Python import statement for most libraries.

### Why not all-Rust

User code is Python. Barca must execute it. Rather than embedding a Python
interpreter (which would couple us to a specific Python version and break
virtualenvs), we spawn standard Python processes. This means:

- Users' existing virtualenvs work unchanged
- Any Python version works (we test 3.12+)
- No FFI boundary for user code
- Workers are isolated processes — one crash doesn't take down the orchestrator

---

## 6. What we didn't build (yet)

### Backoff on retries

The coordinator retries failed items immediately (push back to ready queue). The
old `scheduler.rs` had exponential backoff (`retry_backoff_seconds * attempt`).
We removed this for simplicity. For most use cases, immediate retry is fine. If
backoff is needed, it can be added to the coordinator without changing the worker
model.

### Worker affinity / cache locality

The global ready queue assigns tasks to the first idle worker. A smarter scheduler
could prefer assigning tasks to workers that already have relevant modules imported
or data cached in memory. This is a natural extension of the pull-based model —
the ready queue becomes a priority queue with affinity scoring.

### Windows support

SIGSTOP/SIGCONT is Unix-only. Windows would need `SuspendThread`/`ResumeThread`
or a cooperative suspension model (worker checks a flag between tasks). The UDS
protocol would need to switch to named pipes or TCP localhost on Windows.

### Distributed execution

The current model is single-machine, multi-process. Distributing across machines
would require replacing UDS with TCP sockets and adding a work-stealing protocol.
The stateless worker model is already compatible — workers don't share state, so
they could run on different machines with artifact storage on a shared filesystem
or object store.
