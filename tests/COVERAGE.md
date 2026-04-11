# Barca Spec Coverage Matrix

This document maps every rule, invariant, and entity definition in
`barca.allium` to the test file(s) that exercise it. It's a living document —
update it whenever you add or remove tests.

**Status legend**:
- ✅ Covered (hand-written test + scenario)
- 🟡 Partially covered (core case tested, edge cases missing)
- ❌ Not covered

**Current state**: 266 tests collected, 128 passing, 135 failing (red for Phase 3),
3 skipped. Every failing test is an executable specification of what Phase 3
must deliver.

---

## Invariants

| Spec invariant | Status | Test(s) |
|---|---|---|
| `EffectsAreLeafNodes` (effect half) | ✅ | `test_effect.py::test_effect_cannot_be_used_as_input` |
| `EffectsAreLeafNodes` (sink half) | ✅ | `test_sink.py::test_sink_rejected_as_input_to_other_asset` |
| `SensorsAreSourceNodes` | ✅ | `test_sensor_freshness.py::test_sensor_rejects_inputs_kwarg` |
| `SensorFreshnessIsNotAlways` | ✅ | `test_sensor_freshness.py::test_sensor_rejects_always_freshness` |
| `NoCyclesInDAG` | ✅ | `test_no_cycles.py` (4 tests: self-ref, 2-node, 3-node indirect, partial-alongside-healthy) |
| `ManualBlocksDownstream` | ✅ | `test_manual_blocks_downstream.py` (4 tests) + `scenarios/02_manual_blocks_downstream.yaml` |

## Hashing rules

| Spec rule | Status | Test(s) |
|---|---|---|
| `DefinitionHashForPureAsset` | ✅ | `test_cache_stability.py` (8 tests on the hash function itself) + `test_deps.py`, `test_codebase_hash.py`, `test_trace.py` |
| `DefinitionHashForUnsafeAsset` | ✅ | `test_trace.py::test_unsafe_skips_tracing`, `test_unsafe_no_cache_kwarg.py`, `scenarios/25_unsafe_dep_cone_isolated.yaml` |

## Staleness rules

| Spec rule | Status | Test(s) |
|---|---|---|
| `AssetBecomesStaleOnDefinitionChange` | ✅ | `scenarios/06_definition_change_cascades.yaml`, `scenarios/24_deep_dep_chain_change.yaml` |
| `AssetBecomesStaleOnUpstreamRefresh` | ✅ | `scenarios/06_definition_change_cascades.yaml` (explicit cascade assertion) |
| `AssetBlockedOnUpstreamFailure` | ✅ | `scenarios/07_upstream_failure_blocks_downstream.yaml`, `test_run_pass.py::test_failure_cascades_to_downstream` |

## Materialisation rules

| Spec rule | Status | Test(s) |
|---|---|---|
| `CacheHitSkipsMaterialisation` | ✅ | `test_run_pass.py::test_second_run_pass_is_fresh`, `scenarios/04_cache_hit_after_revert.yaml` (revert round-trip), `scenarios/25_unsafe_dep_cone_isolated.yaml`, `scenarios/26_kwarg_order_stability.yaml`, `test_unsafe_no_cache_kwarg.py::test_unsafe_asset_caches_like_pure` |
| `MaterialiseAsset` | ✅ | `test_run_pass.py`, `test_notebook.py` |
| `MaterialiseSensor` (full tuple downstream) | ✅ | `test_sensor.py`, `test_sensor_freshness.py`, `test_run_pass.py::test_sensor_full_tuple_passed_to_downstream`, `scenarios/12_sensor_raises_exception.yaml`, `scenarios/13_sensor_wrong_shape.yaml` |
| `MaterialiseEffect` | ✅ | `test_effect.py::test_effect_executes_after_upstream`, `test_effect.py::test_inputless_always_effect_runs_once` (D2), `scenarios/14_effect_raises_exception.yaml`, `scenarios/20_inputless_always_effect.yaml` |
| `MaterialiseSinks` | ✅ | `test_sink.py` (10 tests), `scenarios/15_sink_writes_to_disk.yaml`, `scenarios/16_multiple_sinks_stacked.yaml`, `scenarios/17_sink_failure_parent_survives.yaml` |

## Partition rules

| Spec rule | Status | Test(s) |
|---|---|---|
| `DynamicPartitionsFromUpstream` | ✅ | `test_dynamic_partitions.py`, `scenarios/21_empty_dynamic_partitions.yaml` (edge case) |
| `PartitionInheritance` (the default case) | ✅ | `scenarios/08_partition_inheritance_default.yaml`, `scenarios/09_multi_level_partition_chain.yaml` (3-level) |
| `ExplicitPartitionSubset` | 🟡 | `test_dynamic_partitions.py::test_explicit_partition_subset_validated` (shallow) |
| `CollectPartitions` | ✅ | `test_collect.py` (6 tests: marker, dict shape, tuple keys, multi-dim, partial failure, unblock) |
| `PartitionSetResolvedLazily` | ✅ | `test_dynamic_partitions.py::test_partition_set_pending_before_upstream_materializes` |

## Explicit refresh

| Spec rule | Status | Test(s) |
|---|---|---|
| `ExplicitRefresh` with 3 policies | ✅ | `test_explicit_refresh.py` (6 tests) |
| Does not cascade downstream | ✅ | `test_explicit_refresh.py::test_refresh_does_not_cascade_downstream` |
| Rejects effects | ✅ | `test_explicit_refresh.py::test_refresh_rejects_effect` |
| Rejects sinks | ✅ | `test_sink.py::test_sink_cannot_be_directly_refreshed`, `scenarios/28_refresh_rejects_sink.yaml` |

## Dev mode

| Spec rule | Status | Test(s) |
|---|---|---|
| `DevModeTracksstaleness` (unit — pure function) | ✅ | `test_dev_mode.py::test_handle_file_change_marks_assets_stale`, `test_dev_mode.py::test_handle_file_change_does_not_materialize` |
| `DevModeTracksstaleness` (integration — real watcher) | ✅ | `test_dev_integration.py` (5 tests: edit, new file, deletion, never-materialises, barca.toml edit) |

## Run mode

| Spec rule | Status | Test(s) |
|---|---|---|
| `RunMaintainsFreshness` | ✅ | `test_run_loop.py` (5 tests: multiple passes, manual-never, failed-stays-failed, clean-stop, schedule-catchup) |
| run_pass unit behavior | ✅ | `test_run_pass.py` (7 tests) + 25+ YAML scenarios |

## Reindex diff

| Spec rule | Status | Test(s) |
|---|---|---|
| `ReindexShowsDiff` (added / removed / renamed) | ✅ | `test_reindex_diff.py` (7 tests), `test_cli.py::test_cli_reindex_shows_added_section`, `test_cli.py::test_cli_reindex_shows_removed_section` |
| Rename by AST match | ✅ | `test_reindex_diff.py::test_rename_detected_by_ast_match` |
| Rename by `name=` kwarg | ✅ | `test_reindex_diff.py::test_rename_detected_by_name_kwarg` |
| History preserved across rename | ✅ | `test_reindex_diff.py::test_rename_preserves_materialization_history` |

## History & pruning

| Spec rule | Status | Test(s) |
|---|---|---|
| `HistoryPreservedAcrossReindex` | ✅ | `test_reindex_diff.py::test_rename_preserves_materialization_history`, `scenarios/18_history_survives_remove.yaml` |
| `PruneRemovesUnreachableHistory` | ✅ | `test_prune.py` (3 tests), `test_cli.py::test_cli_prune_with_yes_flag`, `test_cli.py::test_cli_prune_empty_project_is_noop` |

## CLI visual distinction

| Spec rule | Status | Test(s) |
|---|---|---|
| `UnsafeAssetsDistinguishedInListing` | ✅ | `test_cli.py::test_cli_assets_list_shows_unsafe_badge` (subprocess test asserting the [unsafe] badge is visible in stdout) |

## Sink failure

| Spec rule | Status | Test(s) |
|---|---|---|
| `SinkFailureIsProminent` | ✅ | `test_sink.py::test_sink_failure_does_not_fail_parent` (non-blocking half), `scenarios/17_sink_failure_parent_survives.yaml` |

---

## Pathological interaction scenarios

These test what happens when rules collide under pressure. They're
specifically the "messy real-world stuff" the spec doesn't directly describe.

| Scenario | File |
|---|---|
| Two threads call `refresh()` on same asset simultaneously | `test_interactions.py::test_concurrent_refresh_same_asset` |
| `barca assets refresh` during `run_loop` | `test_interactions.py::test_refresh_during_run_loop` |
| File rename while `run_pass` is in-flight | `test_interactions.py::test_rename_mid_run_pass_completes_cleanly` |
| `barca prune` during `run_pass` | `test_interactions.py::test_prune_during_run_pass` |
| Edit upstream source while downstream is mid-materialisation | `test_interactions.py::test_edit_upstream_while_downstream_runs` |
| Dev mode + refresh simultaneously | `test_interactions.py::test_dev_mode_sees_refresh_as_db_change` |
| Edit `barca.toml` mid-session (add module) | `test_interactions.py::test_barca_toml_edit_picked_up_on_next_reindex` |
| Rename partition-defining asset with pending downstream | `test_interactions.py::test_rename_partition_source_with_pending_downstream` |
| `barca reset --db` during `run_pass` | `test_interactions.py::test_reset_db_while_pass_running` |

## Cache / hash stability (the "subtle edge cases that break caching")

| Property | File |
|---|---|
| Identical source → same hash | `test_cache_stability.py::test_identical_decorator_metadata_same_hash` |
| Source change → different hash | `test_cache_stability.py::test_source_change_changes_hash` |
| Decorator metadata change → different hash | `test_cache_stability.py::test_decorator_metadata_change_changes_hash` |
| **Kwarg order in metadata → stable** (currently FAILS, Phase 3 TODO: add sort_keys=True) | `test_cache_stability.py::test_kwarg_order_in_metadata_does_not_matter` |
| Dep cone change → different hash | `test_cache_stability.py::test_dep_cone_change_changes_hash` |
| Python version → different hash | `test_cache_stability.py::test_python_version_change_changes_hash` |
| Serializer kind → different hash | `test_cache_stability.py::test_serializer_kind_change_changes_hash` |
| PROTOCOL_VERSION change → different hash | `test_cache_stability.py::test_protocol_version_baked_into_hash` |
| run_hash stable for same inputs | `test_cache_stability.py::test_run_hash_stable_for_same_inputs` |
| run_hash changes with upstream | `test_cache_stability.py::test_run_hash_changes_with_upstream` |
| **run_hash upstream order → stable** (currently FAILS, Phase 3 TODO: sort before hash) | `test_cache_stability.py::test_run_hash_upstream_order_should_be_stable` |
| Partition key affects run_hash | `test_cache_stability.py::test_run_hash_partition_key_affects_hash` |
| Comment-only edit behavior (pinned at "busts cache") | `test_cache_stability.py::test_comment_only_edit_decision_pending` |
| Whitespace-only edit behavior (pinned at "busts cache") | `test_cache_stability.py::test_whitespace_only_edit_decision_pending` |

## CLI subprocess tests

| Test | Purpose |
|---|---|
| `test_cli_help_lists_run_command` | Command discoverability |
| `test_cli_help_lists_dev_command` | Command discoverability |
| `test_cli_help_lists_prune_command` | Command discoverability |
| `test_cli_refresh_help_documents_stale_policy` | Flag discoverability |
| `test_cli_reindex_shows_added_section` | Three-way diff output format |
| `test_cli_reindex_shows_removed_section` | Three-way diff output format |
| `test_cli_reindex_empty_project_exits_zero` | Empty-project smoke |
| `test_cli_assets_list_shows_unsafe_badge` | Spec `UnsafeAssetsDistinguishedInListing` |
| `test_cli_refresh_stale_policy_error_nonzero_exit` | Default error policy |
| `test_cli_refresh_stale_policy_pass_succeeds` | --stale-policy=pass |
| `test_cli_prune_with_yes_flag` | `--yes` skips prompt |
| `test_cli_prune_empty_project_is_noop` | Empty-project prune smoke |
| `test_cli_run_command_exists` | Phase 3: new command |
| `test_cli_dev_command_exists` | Phase 3: new command |
| `test_cli_reconcile_command_removed` | Phase 3: old command removed |

---

## YAML scenarios (28 total)

| # | Name | Rule(s) covered |
|---|---|---|
| 01 | `simple_reindex` | Basic reindex, ReindexShowsDiff |
| 02 | `manual_blocks_downstream` | ManualBlocksDownstream |
| 03 | `cache_hit_on_second_pass` | CacheHitSkipsMaterialisation |
| 04 | `cache_hit_after_revert` | CacheHit round-trip ("re-typing identical source") |
| 05 | `fail_then_fix_recovery` | Recovery loop (failed → stale → fresh) |
| 06 | `definition_change_cascades` | AssetBecomesStaleOnDefinitionChange + AssetBecomesStaleOnUpstreamRefresh |
| 07 | `upstream_failure_blocks_downstream` | AssetBlockedOnUpstreamFailure |
| 08 | `partition_inheritance_default` | PartitionInheritance |
| 09 | `multi_level_partition_chain` | PartitionInheritance (deep) |
| 10 | `schedule_cron_catchup` | Schedule catch-up semantics (fires once per tick period) |
| 11 | `schedule_cron_not_yet_eligible` | Cron eligibility boundary |
| 12 | `sensor_raises_exception` | Sensor error path |
| 13 | `sensor_wrong_shape` | Sensor returns non-tuple |
| 14 | `effect_raises_exception` | Effect error path (isolated from parent) |
| 15 | `sink_writes_to_disk` | MaterialiseSinks happy path |
| 16 | `multiple_sinks_stacked` | MaterialiseSinks with multiple sinks |
| 17 | `sink_failure_parent_survives` | SinkFailureIsProminent |
| 18 | `history_survives_remove` | HistoryPreservedAcrossReindex |
| 19 | `schedule_with_manual_upstream` | Freshness interaction |
| 20 | `inputless_always_effect` | D2 — input-less Always effect runs once |
| 21 | `empty_dynamic_partitions` | Partition edge case (empty) |
| 22 | `purity_flip_invalidates` | Adding @unsafe busts cache |
| 23 | `freshness_flip_to_manual` | Changing freshness busts cache + new status |
| 24 | `deep_dep_chain_change` | Dep cone tracing end-to-end |
| 25 | `unsafe_dep_cone_isolated` | @unsafe skips dep cone tracing |
| 26 | `kwarg_order_stability` | Decorator metadata order stability |
| 27 | `multiple_independent_dags` | Discovery of multiple DAGs |
| 28 | `refresh_rejects_sink` | ExplicitRefresh rejects sink |

---

## Final count

| Bucket | Files | Tests |
|---|---|---|
| Baseline (passing pre-refactor) | `test_basic`, `test_codebase_hash`, `test_deps`, `test_multicore`, `test_partitions`, `test_store_concurrency`, `test_trace` | 32 |
| Freshness primitive | `test_freshness` | 29 |
| BarcaTestContext infrastructure | `test_context_smoke` | 13 |
| Cache stability | `test_cache_stability` | 14 |
| Run pass | `test_run_pass`, `test_run_pass_migrated` | 10 |
| Run loop | `test_run_loop` | 5 |
| Sensors | `test_sensor`, `test_sensor_freshness` | 17 |
| Effects | `test_effect` | 5 |
| Sinks | `test_sink` | 11 |
| Collect & partitions | `test_collect`, `test_dynamic_partitions` | 10 |
| Refresh | `test_explicit_refresh`, `test_manual_blocks_downstream` | 10 |
| Prune | `test_prune` | 3 |
| Reindex diff | `test_reindex_diff` | 7 |
| Dev mode | `test_dev_mode`, `test_dev_integration` | 7 |
| Cycle detection | `test_no_cycles` | 4 |
| Unsafe | `test_unsafe_no_cache_kwarg` | 6 |
| CLI subprocess | `test_cli` | 15 |
| Pathological interactions | `test_interactions` | 10 |
| Server | `test_server` | 16 |
| Notebook | `test_notebook` | 15 |
| YAML scenarios | `test_scenarios` | 28 |
| **Total** | 27 files + 28 YAML | **266** |

Current run: **128 passing, 135 failing, 3 skipped**. Every failing test
is an executable specification Phase 3 must make green.

## Phase 3 TODO list (extracted from failing tests)

Implementing the source to turn all these green is the Phase 3 plan. Key items:

1. **`run_pass` / `run_loop`** — the entire orchestration core
2. **`dev_watch` / `handle_file_change`** — file watcher + pure reindex dispatch
3. **`prune`** — reachability analysis + filesystem + DB cleanup
4. **`@sink` wiring** — parent-asset linkage, sink inspection, artifact writing via fsspec
5. **Reindex three-way diff with rename detection** (AST match + `name=` match)
6. **`freshness=` decorator kwarg** across `@asset`, `@sensor`, `@effect`
7. **Default freshness = `Always`** for `@asset` and `@effect`
8. **Sensor requires explicit freshness** at decoration time; rejects `Always`
9. **`stale_policy` on `refresh()`** with error/warn/pass semantics
10. **`ManualBlocksDownstream`** invariant enforcement in run_pass
11. **`collect()` input resolution** — dict[tuple[str,...], T]
12. **Dynamic partitions lazy resolution** + implicit upstream edge
13. **`@unsafe` — remove `cache=` parameter** completely
14. **Protocol version bump to 0.4.0** (invalidate all old caches)
15. **JSON serialization sort_keys=True** in `compute_definition_hash`
16. **Sort upstream_materialization_ids** in `compute_run_hash`
17. **CLI commands**: `run`, `dev`, `prune`; remove `reconcile`
18. **`--stale-policy` option** on `assets refresh`
19. **`[unsafe]` badge** in `assets list` CLI output
20. **Sink failure prominence** in `assets list` and job logs
