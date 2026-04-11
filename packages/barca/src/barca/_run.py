"""barca run — production mode that maintains the DAG at declared freshness.

``run_pass(store, repo_root)`` executes a single reconciliation pass:

1. Call ``reindex()`` to pick up any file changes
2. Topologically sort the active DAG
3. Walk in topo order, materialising everything that is eligible:
   - Always + stale + upstreams fresh           → materialise
   - Schedule + cron tick elapsed + upstreams fresh → materialise
   - Manual                                     → skip (count as manual_skipped)
   - Any asset with a Manual upstream stale    → skip (count as stale_blocked)
   - Sensor + eligible                          → observe; propagate if update_detected
   - Effect + upstream freshened or first run   → execute
   - Sink + parent materialised                 → (handled inline by engine)

``run_loop(store, repo_root, stop_event)`` calls ``run_pass`` in a loop
until ``stop_event`` is set. Passes are strictly sequential — no two
passes ever overlap because we never spawn an inner thread to run a pass
from within the loop.
"""

from __future__ import annotations

import importlib
import logging
from collections import defaultdict, deque
from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING

from barca._engine import (
    _execute_materialization,
    _materialize_sinks,
    _resolve_asset_inputs,
    _resolve_partition_values,
    reindex,
    trigger_sensor,
)
from barca._freshness import Always, Manual, Schedule, deserialize, is_eligible
from barca._hashing import compute_run_hash, now_ts
from barca._models import (
    AssetDetail,
    AssetSummary,
    RunPassResult,
)

if TYPE_CHECKING:
    from barca._store import MetadataStore

logger = logging.getLogger(__name__)


def run_pass(store: MetadataStore, repo_root: Path) -> RunPassResult:
    """Execute a single reconciliation pass."""
    diff = reindex(store, repo_root)

    result = RunPassResult(
        added=diff.added,
        removed=diff.removed,
        renamed=diff.renamed,
    )

    # Load the active asset set
    summaries = store.list_assets()
    if not summaries:
        return result

    by_id: dict[int, AssetSummary] = {a.asset_id: a for a in summaries}

    # Build adjacency: asset_id -> list of upstream asset_ids
    upstream_map: dict[int, list[int]] = defaultdict(list)
    downstream_map: dict[int, list[int]] = defaultdict(list)
    for summary in summaries:
        try:
            detail = store.asset_detail(summary.asset_id)
        except ValueError:
            continue
        inputs = store.get_asset_inputs(detail.asset.definition_id)
        for inp in inputs:
            if inp.upstream_asset_id is None or inp.upstream_asset_id < 0:
                continue
            upstream_map[summary.asset_id].append(inp.upstream_asset_id)
            downstream_map[inp.upstream_asset_id].append(summary.asset_id)

    # For sinks, add an implicit upstream edge from the parent asset
    for summary in summaries:
        if summary.kind == "sink" and summary.parent_asset_id is not None:
            upstream_map[summary.asset_id].append(summary.parent_asset_id)
            downstream_map[summary.parent_asset_id].append(summary.asset_id)

    # Topological sort (Kahn's)
    indegree: dict[int, int] = defaultdict(int)
    for asset_id in by_id:
        indegree[asset_id] = 0
    for asset_id, ups in upstream_map.items():
        indegree[asset_id] = len(ups)

    queue = deque([aid for aid in by_id if indegree[aid] == 0])
    topo_order: list[int] = []
    while queue:
        aid = queue.popleft()
        topo_order.append(aid)
        for down in downstream_map.get(aid, []):
            indegree[down] -= 1
            if indegree[down] == 0:
                queue.append(down)

    if len(topo_order) < len(by_id):
        raise ValueError("cycle detected in asset DAG — cannot run_pass")

    # Per-pass tracking
    refreshed_ids: set[int] = set()
    failed_ids: set[int] = set()
    # Sensors that reported (False, ...) this pass — downstream assets waiting
    # on these should NOT run, because there's no new data. They stay in
    # whatever state they were in (fresh or untouched).
    sensors_no_update: set[int] = set()
    manual_blocks: dict[int, bool] = {}  # asset_id -> has_stale_manual_upstream

    def _has_stale_manual_upstream(asset_id: int) -> bool:
        if asset_id in manual_blocks:
            return manual_blocks[asset_id]
        for up_id in upstream_map.get(asset_id, []):
            up = by_id.get(up_id)
            if up is None:
                continue
            if up.freshness == "manual":
                up_latest = store.latest_successful_materialization(up_id)
                up_detail = store.asset_detail(up_id)
                if up_latest is None or up_latest.definition_id != up_detail.asset.definition_id:
                    manual_blocks[asset_id] = True
                    return True
            # Recurse: an Always asset blocked by a deeper manual is also blocked
            if _has_stale_manual_upstream(up_id):
                manual_blocks[asset_id] = True
                return True
        manual_blocks[asset_id] = False
        return False

    now = float(now_ts())

    for asset_id in topo_order:
        summary = by_id[asset_id]
        kind = summary.kind

        # Skip sinks — they run inline with their parent's materialization
        if kind == "sink":
            continue

        # Check for upstream failure cascade
        if any(up in failed_ids for up in upstream_map.get(asset_id, [])):
            failed_ids.add(asset_id)
            continue

        # Parse freshness
        try:
            freshness = deserialize(summary.freshness)
        except Exception:
            freshness = Always()

        # Manual assets are skipped
        if isinstance(freshness, Manual):
            result.manual_skipped += 1
            continue

        # Check upstream manual block (for Always/Schedule assets)
        if _has_stale_manual_upstream(asset_id):
            result.stale_blocked += 1
            continue

        # Check schedule eligibility
        try:
            detail = store.asset_detail(asset_id)
        except ValueError:
            continue

        # Check for a prior FAILED materialization at the current definition.
        # Spec: "No retries on failure. A failed asset blocks dependents
        # until fixed (via source change or explicit refresh)."
        if kind == "asset":
            # Peek at the most recent mat (any status)
            latest_any = store.list_materializations(asset_id, limit=1)
            if latest_any and latest_any[0].status == "failed" and latest_any[0].definition_id == detail.asset.definition_id:
                failed_ids.add(asset_id)
                result.failed += 1
                continue

        # For staleness/eligibility, use the right history record per kind
        if kind == "sensor":
            obs = store.latest_sensor_observation(asset_id)
            last_run_ts = obs.created_at if obs else None
            latest_def_id = obs.definition_id if obs else None
            latest_exists = obs is not None
        elif kind == "effect":
            effect_exec = store.latest_effect_execution(asset_id)
            last_run_ts = effect_exec.created_at if effect_exec else None
            latest_def_id = effect_exec.definition_id if effect_exec else None
            latest_exists = effect_exec is not None and effect_exec.status == "success"
        else:
            latest = store.latest_successful_materialization(asset_id)
            last_run_ts = latest.created_at if latest else None
            latest_def_id = latest.definition_id if latest else None
            latest_exists = latest is not None

        if isinstance(freshness, Schedule):
            if not is_eligible(freshness, last_run_ts, now):
                # Not yet eligible — treat as fresh
                result.fresh += 1
                continue

        # An asset is stale if: no successful run, definition changed,
        # or any upstream was refreshed this pass.
        definition_stale = not latest_exists or latest_def_id != detail.asset.definition_id
        upstream_refreshed = any(up in refreshed_ids for up in upstream_map.get(asset_id, []))

        # If an upstream sensor reported no update this pass, the downstream
        # should not run regardless of whether it has prior state. This lets
        # users write sensor-gated pipelines that only trigger on change.
        gated_by_sensor = any(up in sensors_no_update for up in upstream_map.get(asset_id, []))
        if gated_by_sensor and not upstream_refreshed:
            result.fresh += 1
            continue

        if not definition_stale and not upstream_refreshed:
            # Already fresh
            result.fresh += 1
            continue

        # Dispatch by kind
        if kind == "sensor":
            try:
                obs = trigger_sensor(store, repo_root, asset_id)
                result.executed_sensors += 1
                # Mark refreshed only if sensor reports update
                if obs.update_detected:
                    refreshed_ids.add(asset_id)
                else:
                    sensors_no_update.add(asset_id)
            except Exception as exc:
                logger.warning(f"sensor {summary.logical_name} failed: {exc}")
                failed_ids.add(asset_id)
                result.failed += 1
            continue

        if kind == "effect":
            first_run = not latest_exists
            if not upstream_refreshed and not first_run and not definition_stale:
                result.fresh += 1
                continue
            try:
                _execute_in_pass(store, repo_root, detail, upstream_map, failed_ids)
                result.executed_effects += 1
                refreshed_ids.add(asset_id)
            except Exception as exc:
                logger.warning(f"effect {summary.logical_name} failed: {exc}")
                failed_ids.add(asset_id)
                result.failed += 1
            continue

        if kind == "asset":
            try:
                n_executed = _execute_in_pass(store, repo_root, detail, upstream_map, failed_ids)
                if n_executed > 0:
                    result.executed_assets += n_executed
                    refreshed_ids.add(asset_id)
                else:
                    result.fresh += 1
            except Exception as exc:
                logger.warning(f"asset {summary.logical_name} failed: {exc}")
                failed_ids.add(asset_id)
                result.failed += 1
            continue

    # Count sinks that ran (from sink_executions this pass)
    # Simpler approach: count sinks whose parent is in refreshed_ids
    for summary in summaries:
        if summary.kind != "sink":
            continue
        parent = summary.parent_asset_id
        if parent in refreshed_ids:
            sink_exec = store.latest_sink_execution(summary.asset_id)
            if sink_exec is not None:
                if sink_exec.status == "success":
                    result.executed_sinks += 1
                else:
                    result.sink_failed += 1

    return result


def _execute_in_pass(
    store: MetadataStore,
    repo_root: Path,
    detail: AssetDetail,
    upstream_map: dict[int, list[int]],
    failed_ids: set[int],
) -> int:
    """Execute one asset/effect within a run_pass. Returns the number of
    materialisations produced (0 on cache hit, 1 for non-partitioned,
    N for partitioned — one per partition value)."""
    asset_inputs = _resolve_asset_inputs(store, detail, repo_root)

    # Fail fast if any upstream is known failed in this pass
    for inp in asset_inputs:
        if inp.upstream_asset_id is not None and inp.upstream_asset_id in failed_ids:
            raise RuntimeError(f"upstream {inp.upstream_asset_ref} failed")

    partition_values = _resolve_partition_values(store, detail, repo_root)

    if detail.asset.kind == "effect":
        # Effects don't produce an artifact — just call the function with
        # inputs gathered for the non-partitioned case.
        from barca._engine import _gather_inputs_for_partition

        input_kwargs, _ = _gather_inputs_for_partition(store, repo_root, asset_inputs, partition_key_json=None)
        mod = importlib.import_module(detail.asset.module_path)
        func = getattr(mod, detail.asset.function_name)
        original = getattr(func, "__barca_original__", func)
        try:
            if input_kwargs:
                original(**input_kwargs)
            else:
                original()
            store.insert_effect_execution(detail.asset.asset_id, detail.asset.definition_id, "success")
        except Exception as exc:
            store.insert_effect_execution(
                detail.asset.asset_id,
                detail.asset.definition_id,
                "failed",
                str(exc),
            )
            raise
        return 1

    # Regular asset
    if partition_values:
        # Partitioned materialization — input loading is deferred to the
        # per-partition loop so each partition gets its matching upstream.
        before_count = len(store.list_materializations(detail.asset.asset_id, limit=10000))
        from barca._engine import _refresh_partitioned

        _refresh_partitioned(
            store,
            repo_root,
            detail,
            asset_inputs,
            partition_values,
            max_workers=1,
        )
        after_count = len(store.list_materializations(detail.asset.asset_id, limit=10000))
        return max(after_count - before_count, 0)

    # Non-partitioned: load inputs once for the single materialisation
    from barca._engine import _gather_inputs_for_partition

    input_kwargs, upstream_mat_ids = _gather_inputs_for_partition(store, repo_root, asset_inputs, partition_key_json=None)

    # Single materialization path
    has_real_inputs = len(upstream_mat_ids) > 0
    run_hash = compute_run_hash(detail.asset.definition_hash, upstream_mat_ids) if has_real_inputs else detail.asset.definition_hash

    # Cache check
    existing = store.successful_materialization_for_run(detail.asset.asset_id, run_hash)
    if existing:
        return 0  # cache hit

    mat_id = store.insert_queued_materialization(
        detail.asset.asset_id,
        detail.asset.definition_id,
        run_hash,
    )
    try:
        _execute_materialization(
            store,
            repo_root,
            detail,
            mat_id,
            run_hash,
            input_kwargs,
            upstream_mat_ids,
        )
    except Exception as exc:
        store.mark_materialization_failed(mat_id, str(exc))
        raise

    # Run attached sinks
    _materialize_sinks(store, repo_root, detail, run_hash)
    return 1


def run_loop(
    store: MetadataStore,
    repo_root: Path,
    stop_event: Event | None = None,
    interval: float = 0.1,
) -> None:
    """Repeatedly call ``run_pass`` until ``stop_event`` is set.

    Passes are strictly sequential — the next pass only starts after the
    previous one returns. ``interval`` is the sleep between passes (keeps
    CPU low on idle passes).
    """
    if stop_event is None:
        stop_event = Event()
    while not stop_event.is_set():
        try:
            run_pass(store, repo_root)
        except Exception as exc:  # pragma: no cover
            logger.exception(f"run_pass error: {exc}")
        if stop_event.wait(timeout=interval):
            break
