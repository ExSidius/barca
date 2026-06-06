# resilience_pileup

Compares how orchestrators cope with **one bad asset** among otherwise healthy work.

**Workload:** 8 independent healthy 2-asset chains + one *poison* chain whose head
fails twice (with `retry_backoff`) before recovering on the third attempt.

**What it measures:** failure-path behavior, not happy-path overhead. A pileup is
when independent, runnable work is stuck behind the flaky asset (or behind its
backoff sleeps).

**Expectation:**

| Framework | Mode | Wall-clock |
|---|---|---|
| **barca** | Rust-owned retries; healthy chains run in-process; backoff sits in a delay-queue (no worker slot) | ≈ `max(healthy work, total backoff)` |
| **dagster** (script) | `materialize()` sequential | ≈ `sum(work) + backoff` |
| **prefect** (sequential) | direct task calls | ≈ `sum(work) + backoff` |

Run: `./bench.sh 5` (needs `hyperfine` and a per-framework `.venv`; see `bench.sh`).
