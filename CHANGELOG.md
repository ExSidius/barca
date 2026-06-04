# Changelog

All notable changes to this project will be documented in this file.
## [0.1.1] - 2026-06-04

### Bug Fixes

- Fix 3 PR review issues: artifact key collision, binary caching, deterministic partition get

### Changes

- Correct __main__.py entry-point description in CLAUDE.md
- Clean up stale files from pre-Rust rewrite

### Polish

- Polish: Result errors, default subcommand, versions, test isolation, README

### Release

- V0.1.1: Engine refactor + clap CLI + Python API
- V0.1.1: Engine refactor + clap CLI + Python API

## [0.1.0] - 2026-06-04

### Bug Fixes

- Fix PyPI license metadata: use SPDX identifier instead of text table
- Fix all 11 PR review issues: CI, correctness, latent bugs, nits
- Fix all 3 staleness gaps: cross-file imports, sensor bypass, partition cascade
- Fix chain caching (ordered persist) + add cached benchmarks
- Fix run_hash consistency: 13/13 cache tests pass
- Fix: user print() statements no longer corrupt worker protocol
- Fix all clippy warnings, tighten idiomatic Rust
- Fix thread-safety in MetadataStore for concurrent per-thread usage (#27)
- Fix: make UI reactive to asset state changes after reset/reindex (#14)
- Fix: support bare @asset decorator (no parentheses) (#12)

### Changes

- File-based artifact persistence: replace JSON-over-stderr with format-aware artifacts
- Require Python >=3.12, build wheels for 3.12/3.13/3.14
- Restore full release workflow from prior working config
- Use PYPI_API_TOKEN secret for PyPI publish (already configured)
- Engine hardening: refactor, protocol, first-class partitions, P0 fixes, CI/CD
- Rewrite gap tests: cross-file, sensor, partition (drop ops concerns)
- Add gap tests documenting known cache/staleness limitations
- Add cache fuzz tests: 100 random DAGs × mutate × verify staleness
- Add dependency cone analysis for staleness detection
- Implement `barca get` with cache-hit detection and staleness tracking
- Add cache/staleness integration tests (TDD: all currently fail)
- Add dagster server-mode partition benchmarks
- Add dagster/prefect for partition benchmarks, run full comparison
- Add partition benchmarks with partition-aligned stream assignment
- Implement dynamic partition eval and partitions_from resolution
- Implement static partition expansion and dynamic partition parsing
- Clean up DagNode duplication, remove dead code, tighten types
- Comprehensive planner tests documenting DAG shape → execution plan mapping
- Make parser pure: return Result, add 10 edge case tests
- Add integration tests for CLI behavior
- Add Airflow benchmarks: trivial, chain_100, deep_diamond, fan_out_500_50ms
- Complete benchmark fairness: server-mode dagster, parallel prefect
- WIP: Add server-mode benchmark scaffolding for dagster
- Address benchmark fairness concerns (ExSidius/barca#35)
- Add ETL pipeline, wide join, and incremental backfill benchmarks
- Add large_payloads, map_reduce, and multi_file_discovery benchmarks
- Add deep_diamond, wide_layers, and mixed_io_cpu benchmarks
- Wire up multi-process dispatch: workers communicate via stdout, Rust owns DB
- Add execution planner: Dag → decompose → Topology → plan → ExecutionPlan
- Add spaceflights, fan_out_500, and fan_out_500_50ms benchmarks
- Add benchmark results README
- Use SmallVec for inputs/sinks/partition_keys (stack allocation for small collections)
- Add comprehensive grammar spec tests, fix int parsing
- Rewrite barca as Rust binary with Python execution runner
- Add AGENTS.md (#33)
- Replace Datastar/Jinja2 UI with React + shadcn/ui (#31)
- Add comprehensive tests for multicore asset execution (#30)
- Add learning path to README, show artifact path after refresh, fix broken examples (#29)
- Replace plain text CLI output with Rich-formatted tables and panels (#28)
- Consolidate three packages into one + GitHub install instructions (#25)
- Update documentation with GitHub installation and CLI options (#24)
- Switch from sqlite3 to libsql as primary DB driver for MVCC concurrency (#23)
- Update documentation: reflect notebook workflow, add MkDocs, improve code docs (#22)
- Add notebook workflow helpers: load_inputs, materialize, read_asset, list_versions (workflow 7) (#21)
- Make sensors first-class nodes with dedicated CLI, API, and display (workflow 6) (#20)
- Add spaceflights benchmark: 10-asset diamond DAG adapted from Kedro (#19)
- Add test coverage roadmap with detailed spec for 100+ new tests (#18)
- Improve dependency tracking with codebase merkle hash, snapshots, and batch worker (#17)
- Add benchmark suite: Barca vs Prefect vs Dagster orchestration performance (#16)

### Documentation

- Document Airflow 3 LocalExecutor limitations
- Document Airflow LocalExecutor limitation: requires PostgreSQL for parallelism
- Docs: fix datastar-reference for RC.8 syntax (#13)
- Docs: fix Quick Start so new users can actually run barca (#11)

### Refactor

- Refactor to align with barca.allium: freshness, sinks, run/dev/prune (#32)

### Release

- Release polish: metadata, changelog, serializer= parsing, __version__

### Removed

- Drop linux-arm64 from release matrix (simsimd NEON cross-compile failure)
- Remove dead code: plan.rs, NodeState, classify_shape, stats
- Remove AssetStatusBadge web component; use data-persist for dark mode (#15)

## [0.0.3] - 2026-03-14

### Changes

- Chore: bump version to 0.0.3
- Chore: use cross for Linux CLI builds in just release

## [0.0.3rc1] - 2026-03-14

### Bug Fixes

- Fix: use command -v to check for cargo-zigbuild
- Fix: use uv tool install ziglang + symlink to zig in setup recipe
- Fix: ignore untracked files in dirty check
- Fix: handle PEP 440 rc versions in just release (convert to semver for Cargo)

### Changes

- Chore: bump version to 0.0.3rc1
- Chore: sync uv.lock
- Ci: replace release workflow with local just release recipe
- Ci: use macos-latest for x86_64 macOS wheel (macos-13 unavailable)
- Chore: bump all crate versions to 0.0.3
- Ci: fix manylinux glibc compatibility for bundled CLI binary (#10)
- Init (#1)
- Initial commit
