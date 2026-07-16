---
title: 'RFC Process & Template'
description: How barca RFCs work, and the template to copy for a new one.
---

An RFC is how a change to barca's design gets written down and reviewed *before*
it's built, so the tradeoffs get argued about on paper instead of discovered in
review. Every RFC in this section either shipped as a real change (`Accepted`)
or documents a piece of barca's current surface as a retroactive baseline —
see [RFC-0001](/rfcs/0001-node-kinds-and-freshness/) onward.

## When to write one

Any change that touches the public surface — the CLI, the Python API, decorator
kwargs, the HTTP API, or the artifact/wire format — or that changes the
Rust↔Python boundary (`crates/barca-core` ↔ `python/barca/_worker.py`) should
start as an RFC. A pure internal refactor with no surface change doesn't need
one; that kind of decision belongs in
[Architecture Decisions](/architecture-decisions/) instead.

## Process

1. Copy this file to `NNNN-slug.md` in `site/src/content/docs/rfcs/`, using the
   next free number (check the sidebar and this directory for the highest
   existing `NNNN`). Leave `NNNN` zero-padded to 4 digits.
2. Fill in the sections that apply — see "Usage notes" at the bottom. Set
   `Status: Draft`.
3. Open a PR. Reviewers argue about the design in the PR, not just the code.
4. On merge, set `Status: Accepted` (or `Rejected`, if the PR closes without
   merging the design) and add the page to the `RFCs` group in
   `site/astro.config.mjs`.
5. If a later RFC replaces this one, set this one's `Status` to `Superseded`
   and link forward in `Supersedes / Related`; link back from the new one.

---

# RFC-NNNN: [Title]

- **Status:** Draft | Under Review | Accepted | Rejected | Superseded
- **Date:**
- **Touches:** barca-core | barca-cli | python/barca | HTTP server | dev server/UI (check all that apply)
- **Supersedes / Related:**

---

## 1. Summary

One paragraph.

## 2. Motivation

What's broken or missing today? Reference the specific layer — e.g. "the planner in
`barca-core` can't express X" vs. "`barca.get()` in `python/barca/api.py` has no way to Y."

## 3. Guide-Level Explanation

Explain it as if it shipped. Pick every surface this change actually touches and show it in use.
Delete sections that don't apply — most RFCs will only touch 1-2 of these.

### 3.1 CLI
```
barca <command> [args]
```
Example invocation + output, in the same style as the README (`barca get`, `barca plan`, etc).

### 3.2 Python API
```python
import barca
barca.get(...)   # or whatever's new/changed
```

### 3.3 Decorator surface
If this adds/changes `@asset`, `@sensor`, `@task`, `@sink`, or freshness markers
(`Always`/`Manual`/`Schedule`) — show the decorated function and what changes about its behavior.

### 3.4 HTTP API
```
curl -XPOST localhost:8274/<endpoint>
```
Request/response shape. This should also become a diff to the [Server API](/reference/server-api/) reference.

### 3.5 Dev server / `--watch` / UI
What does `barca serve --watch` do differently? Any UI-visible change?

---

## 4. Reference-Level Explanation

### 4.1 Public API Surface
*(Load-bearing — anything a user's pipeline.py or an external HTTP client can depend on.)*

- **CLI:** flags, subcommands, exit codes, stdout/stderr contract (JSON summary shape, diagnostics-to-stderr convention)
- **Python:** signatures in `python/barca/__init__.py` and `api.py`; decorator kwargs; what's exported
- **HTTP:** endpoint paths, request/response JSON schema, status codes, the async run_id/poll contract
- **Wire/artifact format:** anything touching `.barca/metadata.db` schema or artifact serialization
  (json/pickle/parquet) that other tools might read directly

### 4.2 Implementation Details
*(Incidental — free to change without notice.)*

- `barca-core` internals: parser (ruff AST extraction), DAG construction (petgraph), phase/planner logic, dispatch, DB layer, cache
- `barca-cli`: how it shells out to `barca-core` (should stay thin — flag this RFC if it isn't)
- `python/barca/_worker.py`: how workers actually execute steps, LRU cache mechanics
- `python/barca/_artifacts.py`: serialization internals

### 4.3 Rust ↔ Python Boundary
This is barca's one truly load-bearing seam — be explicit:
- What crosses the boundary (subprocess args, stdin/stdout JSON lines, exit codes)?
- Does this change what the Rust binary assumes about worker behavior, or vice versa?
- Static-analysis invariant check: does this change require importing user code (it shouldn't)?

### 4.4 Node-Kind Semantics
If this touches asset/sensor/task behavior, confirm against the existing contract:
| Kind | Cached | Valid as input to |
|---|---|---|
| asset | Yes | assets, sensors, tasks |
| sensor | No | assets, sensors, tasks |
| task | No | tasks only |

Does this RFC preserve "tasks must not be inputs to assets/sensors" (cache-poisoning guard)? If not, justify.

### 4.5 Edge Cases

## 5. Determinism, Caching & Testing

- Does this affect cache-key computation or invalidation?
- Content-addressing (SHA-256) impact, if any
- Test plan: `cargo test` coverage, Python-side test coverage, and which `benchmarks/` scenario(s)
  should be re-run to check for regressions (esp. `trivial` for overhead-sensitive changes)

## 6. Performance

Barca's core value prop is near-zero overhead (38ms trivial-case baseline vs. Dagster/Prefect).
Any change touching the hot path (parse → DAG → plan → spawn → persist) needs a before/after
`hyperfine` number against the relevant `benchmarks/` scenario.

## 7. Drawbacks

## 8. Rationale & Alternatives

## 9. Prior Art

(Dagster/Prefect equivalents are good reference points given the [benchmark comparisons](/comparisons/framework-comparison/) already in the repo.)

## 10. Unresolved Questions

## 11. Future Possibilities

---

### Usage notes
- **Small change (single layer, < ~1 day):** Summary, relevant Guide-Level subsection only, §4.1/4.2 split, Drawbacks. Skip the rest.
- **Anything touching §4.3 (Rust↔Python boundary):** never skip that section — it's the seam most likely to break silently.
- **Anything on the hot path:** §6 benchmarks are not optional.
