---
name: Rust DAG planner architecture decisions
description: Design decisions for introducing Rust into barca — DAG construction, AST analysis, execution planning, and process architecture
type: project
---

Barca is moving toward a hybrid Rust+Python architecture (started 2026-06-02).

**What Rust owns:**
- Python parsing via ruff's AST (same parser as ruff/uv)
- DAG construction + topo sort (petgraph)
- Execution plan generation with parallelism tiers
- Staleness detection via dependency cone hashing (same-file helpers + constants + cross-file imports)
- Eventually: file watching (notify crate), scheduling, serving the UI (axum)

**What Python owns:**
- Function execution (must import + call user code)
- Nothing else if we can help it — Python is demoted to "execution runtime only"

**Key decisions and rationale:**
- **No need for sophisticated AST analysis if inputs are explicit** — DAG structure is fully determined by `inputs={}` decorator metadata. AST analysis is for *staleness detection*, not DAG construction.
- **Same-file dependency cone tracking is essential** — helpers and global constants changing should trigger staleness. Ruff's AST makes this straightforward (tree walk + scope lookup).
- **Cross-file imports should also be tracked** — resolve import paths, parse those files, include in cone hash. Medium difficulty but worth doing.
- **Dynamic dispatch / third-party libs are out of scope** — dynamic dispatch is impossible statically (use `@unsafe` escape hatch). Third-party libs are pinned in lockfile, no need to track.
- **Process architecture**: Prefer PyO3 in-process (single `barca` process) over separate containers. Rust as native extension callable from Python, or Rust binary managing a Python subprocess via stdin/stdout JSON protocol.
- **petgraph for DAG** — de facto Rust graph library, no reason to hand-roll. Gives topo sort, cycle detection, neighbor traversal.
- **Prototype lives in `crates/barca-dag/`** — extract.rs (ruff parser → node extraction), dag.rs (petgraph DAG + execution plan), main.rs (CLI that emits JSON plans).

**Why:** Reindex latency is noticeably worse than dagster/prefect on simple cases. Rust parsing + DAG construction is the path to instant reindex and reactive watch mode.

**How to apply:** All DAG/planning work goes in Rust crates. Python side should converge toward being a thin execution layer only.
