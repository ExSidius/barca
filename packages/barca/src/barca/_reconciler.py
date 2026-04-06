"""Reconciler — single-pass staleness + eligibility + execution."""

from __future__ import annotations

import importlib
import json
import time
from collections import defaultdict, deque
from pathlib import Path

from barca._engine import materialize_asset, reindex
from barca._hashing import compute_run_hash, relative_path, sha256_hex
from barca._models import AssetInput, ReconcileResult
from barca._schedule import deserialize_schedule, is_schedule_eligible
from barca._store import MetadataStore


def reconcile(store: MetadataStore, repo_root: Path) -> ReconcileResult:
    """Single-pass reconciliation: reindex → topo-sort → staleness → execute."""

    # 1. Reindex to discover all nodes
    reindex(store, repo_root)

    # 2. Load all nodes and build adjacency
    all_assets = store.list_assets()
    id_to_summary = {a.asset_id: a for a in all_assets}

    # Build adjacency: asset_id -> list of downstream asset_ids
    downstream: dict[int, list[int]] = defaultdict(list)
    # upstream inputs per asset
    inputs_by_id: dict[int, list[AssetInput]] = {}

    for a in all_assets:
        detail = store.asset_detail(a.asset_id)
        inputs = store.get_asset_inputs(detail.asset.definition_id)
        inputs_by_id[a.asset_id] = inputs
        for inp in inputs:
            if inp.upstream_asset_id and inp.upstream_asset_id > 0:
                downstream[inp.upstream_asset_id].append(a.asset_id)

    # 3. Topological sort (Kahn's algorithm)
    in_degree: dict[int, int] = {a.asset_id: 0 for a in all_assets}
    for a in all_assets:
        for inp in inputs_by_id.get(a.asset_id, []):
            if inp.upstream_asset_id and inp.upstream_asset_id > 0:
                in_degree[a.asset_id] += 1

    queue: deque[int] = deque()
    for aid, deg in in_degree.items():
        if deg == 0:
            queue.append(aid)

    topo_order: list[int] = []
    while queue:
        aid = queue.popleft()
        topo_order.append(aid)
        for ds in downstream.get(aid, []):
            in_degree[ds] -= 1
            if in_degree[ds] == 0:
                queue.append(ds)

    # 4. Walk in topo order, computing staleness and executing eligible nodes
    result = ReconcileResult()
    now = time.time()

    # Track which nodes produced new output this pass
    refreshed_ids: set[int] = set()
    failed_ids: set[int] = set()
    # Sensor outputs for downstream consumption
    sensor_outputs: dict[int, object] = {}

    for aid in topo_order:
        summary = id_to_summary.get(aid)
        if summary is None:
            continue

        detail = store.asset_detail(aid)
        kind = summary.kind
        schedule = deserialize_schedule(summary.schedule)
        inputs = inputs_by_id.get(aid, [])

        # Check if any upstream failed — skip if so
        upstream_failed = any(inp.upstream_asset_id in failed_ids for inp in inputs if inp.upstream_asset_id and inp.upstream_asset_id > 0)
        if upstream_failed:
            result.failed += 1
            failed_ids.add(aid)
            continue

        if kind == "sensor":
            _handle_sensor(
                store,
                repo_root,
                detail,
                schedule,
                now,
                result,
                refreshed_ids,
                failed_ids,
                sensor_outputs,
            )
        elif kind == "asset":
            _handle_asset(
                store,
                repo_root,
                detail,
                inputs,
                schedule,
                now,
                result,
                refreshed_ids,
                failed_ids,
                sensor_outputs,
            )
        elif kind == "effect":
            _handle_effect(
                store,
                repo_root,
                detail,
                inputs,
                schedule,
                now,
                result,
                refreshed_ids,
                failed_ids,
                sensor_outputs,
            )

    return result


def _handle_sensor(store, repo_root, detail, schedule, now, result, refreshed_ids, failed_ids, sensor_outputs) -> None:
    """Execute a sensor if schedule-eligible. Records observation and mutates result/refreshed_ids/sensor_outputs."""
    aid = detail.asset.asset_id

    # Check schedule eligibility
    last_obs = store.latest_sensor_observation(aid)
    last_run_ts = last_obs.created_at if last_obs else None
    if not is_schedule_eligible(schedule, last_run_ts, now):
        result.fresh += 1
        return

    # Execute the sensor
    try:
        mod = importlib.import_module(detail.asset.module_path)
        func = getattr(mod, detail.asset.function_name)
        original = getattr(func, "__barca_original__", func)
        sensor_result = original()

        if not isinstance(sensor_result, tuple) or len(sensor_result) != 2:
            raise ValueError(f"sensor '{detail.asset.function_name}' must return (update_detected: bool, output), got {type(sensor_result).__name__}")

        update_detected, output = sensor_result
        output_json = json.dumps(output) if output is not None else None

        store.insert_sensor_observation(
            aid,
            detail.asset.definition_id,
            bool(update_detected),
            output_json,
        )

        if update_detected:
            refreshed_ids.add(aid)
            sensor_outputs[aid] = output

        result.executed_sensors += 1

    except Exception:
        store.insert_sensor_observation(aid, detail.asset.definition_id, False, None)
        failed_ids.add(aid)
        result.failed += 1


def _handle_asset(store, repo_root, detail, inputs, schedule, now, result, refreshed_ids, failed_ids, sensor_outputs) -> None:
    """Materialize an asset if stale (definition changed or upstream refreshed) and schedule-eligible. Checks cache before executing."""
    aid = detail.asset.asset_id

    # Compute staleness: definition changed, or upstream refreshed this pass
    latest_mat = store.latest_successful_materialization(aid)

    definition_stale = latest_mat is None or latest_mat.definition_id != detail.asset.definition_id

    upstream_refreshed = any(inp.upstream_asset_id in refreshed_ids for inp in inputs if inp.upstream_asset_id and inp.upstream_asset_id > 0)

    is_stale = definition_stale or upstream_refreshed

    if not is_stale:
        result.fresh += 1
        return

    # Check schedule eligibility
    last_run_ts = latest_mat.created_at if latest_mat else None
    if not is_schedule_eligible(schedule, last_run_ts, now):
        result.stale_waiting += 1
        return

    # Check upstream readiness — all upstreams must have successful materializations
    # (sensors provide output via sensor_outputs, not materializations)
    input_kwargs: dict = {}
    upstream_mat_ids: list[int] = []
    all_ready = True

    for inp in inputs:
        uid = inp.upstream_asset_id
        if uid is None or uid <= 0:
            continue
        upstream_summary = store.asset_detail(uid)
        if upstream_summary.asset.kind == "sensor":
            # For sensors, use the latest observation output
            if uid in sensor_outputs:
                input_kwargs[inp.parameter_name] = sensor_outputs[uid]
            else:
                obs = store.latest_sensor_observation(uid)
                if obs and obs.output_json:
                    input_kwargs[inp.parameter_name] = json.loads(obs.output_json)
                else:
                    all_ready = False
                    break
        else:
            upstream_mat = store.latest_successful_materialization(uid)
            if upstream_mat is None:
                all_ready = False
                break
            if upstream_mat.artifact_path:
                full_path = repo_root / upstream_mat.artifact_path
                try:
                    value = json.loads(full_path.read_text())
                    input_kwargs[inp.parameter_name] = value
                except Exception:
                    all_ready = False
                    break
            upstream_mat_ids.append(upstream_mat.materialization_id)

    if not all_ready:
        result.stale_waiting += 1
        return

    # Compute run_hash
    upstream_mat_ids.sort()
    has_inputs = len(inputs) > 0
    run_hash = compute_run_hash(detail.asset.definition_hash, upstream_mat_ids) if has_inputs else detail.asset.definition_hash

    # Check cache
    existing = store.successful_materialization_for_run(aid, run_hash)
    if existing:
        result.fresh += 1
        return

    # Materialize
    try:
        artifact_base = Path(".barcafiles") / detail.asset.asset_slug / detail.asset.definition_hash
        artifact_dir = repo_root / artifact_base
        artifact_dir.mkdir(parents=True, exist_ok=True)

        mat_id = store.insert_queued_materialization(
            aid,
            detail.asset.definition_id,
            run_hash,
        )
        store.update_materialization_run_hash(mat_id, run_hash)

        value_path = materialize_asset(
            detail.asset.module_path,
            detail.asset.function_name,
            artifact_dir,
            input_kwargs if input_kwargs else None,
        )

        value_bytes = value_path.read_bytes()
        artifact_checksum = sha256_hex(value_bytes)
        artifact_path_rel = relative_path(repo_root, value_path)
        (artifact_dir / "code.txt").write_text(detail.asset.source_text)

        store.mark_materialization_success(
            mat_id,
            artifact_path_rel,
            "json",
            artifact_checksum,
        )

        refreshed_ids.add(aid)
        result.executed_assets += 1

    except Exception as e:
        if "mat_id" in locals():
            store.mark_materialization_failed(mat_id, str(e))
        failed_ids.add(aid)
        result.failed += 1


def _handle_effect(store, repo_root, detail, inputs, schedule, now, result, refreshed_ids, failed_ids, sensor_outputs) -> None:
    """Execute an effect if upstream was refreshed this pass. Effects are never cached."""
    aid = detail.asset.asset_id

    # Effects always re-execute when upstream is refreshed (never cached)
    upstream_refreshed = any(inp.upstream_asset_id in refreshed_ids for inp in inputs if inp.upstream_asset_id and inp.upstream_asset_id > 0)

    # Also check schedule eligibility
    latest_exec = store.latest_effect_execution(aid)
    last_run_ts = latest_exec.created_at if latest_exec else None

    # Effects run if upstream refreshed OR if schedule says so and has no prior run
    if not upstream_refreshed:
        if not is_schedule_eligible(schedule, last_run_ts, now):
            result.fresh += 1
            return
        # If schedule eligible but no upstream change, only run if never executed
        if latest_exec is not None and not upstream_refreshed:
            result.fresh += 1
            return

    # Gather input values
    input_kwargs: dict = {}
    for inp in inputs:
        uid = inp.upstream_asset_id
        if uid is None or uid <= 0:
            continue
        upstream_detail = store.asset_detail(uid)
        if upstream_detail.asset.kind == "sensor":
            if uid in sensor_outputs:
                input_kwargs[inp.parameter_name] = sensor_outputs[uid]
            else:
                obs = store.latest_sensor_observation(uid)
                if obs and obs.output_json:
                    input_kwargs[inp.parameter_name] = json.loads(obs.output_json)
        else:
            upstream_mat = store.latest_successful_materialization(uid)
            if upstream_mat and upstream_mat.artifact_path:
                full_path = repo_root / upstream_mat.artifact_path
                try:
                    value = json.loads(full_path.read_text())
                    input_kwargs[inp.parameter_name] = value
                except Exception:
                    pass

    # Execute effect
    try:
        mod = importlib.import_module(detail.asset.module_path)
        func = getattr(mod, detail.asset.function_name)
        original = getattr(func, "__barca_original__", func)

        if input_kwargs:
            original(**input_kwargs)
        else:
            original()

        store.insert_effect_execution(aid, detail.asset.definition_id, "success")
        refreshed_ids.add(aid)
        result.executed_effects += 1

    except Exception as e:
        store.insert_effect_execution(aid, detail.asset.definition_id, "failed", str(e))
        failed_ids.add(aid)
        result.failed += 1
