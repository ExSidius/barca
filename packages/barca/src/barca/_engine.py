"""Core orchestration — reindex, refresh, materialize, sinks.

This layer contains the bulk of Barca's business logic:

- ``reindex()`` — discovers assets, computes hashes, upserts to the store,
  detects added/removed/renamed assets, and returns a ``ReindexDiff``.
- ``refresh(asset_id, stale_policy=...)`` — explicit on-demand materialization
  with a configurable staleness policy (error/warn/pass). Does NOT cascade
  downstream.
- ``trigger_sensor(asset_id)`` — one-shot sensor execution.
- ``materialize_asset(...)`` — low-level function invocation that writes an
  artifact to disk.
- ``_materialize_sinks(...)`` — fan-out from a successful parent
  materialization to all attached sinks.

Key spec rules implemented here:

- DefinitionHashForPureAsset / DefinitionHashForUnsafeAsset
- ReindexShowsDiff (AST-match rename detection)
- ExplicitRefresh (stale_policy semantics)
- CacheHitSkipsMaterialisation
- MaterialiseAsset / MaterialiseSinks
- DynamicPartitionsFromUpstream (implicit input edge)
- PartitionSetResolvedLazily
- HistoryPreservedAcrossReindex (via deactivate_asset, rename_asset)
"""

from __future__ import annotations

import hashlib
import importlib
import json
import logging
import os
import shutil
import sys
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path
from typing import Any

from barca._config import configured_modules, load_config
from barca._hashing import (
    compute_codebase_hash,
    compute_definition_hash,
    compute_run_hash,
    relative_path,
    sha256_hex,
    slugify,
)
from barca._inspector import inspect_modules
from barca._models import (
    AssetDetail,
    AssetInput,
    AssetSummary,
    IndexedAsset,
    InspectedAsset,
    ReindexDiff,
    SensorObservation,
    StaleUpstreamError,
)
from barca._store import MetadataStore

logger = logging.getLogger(__name__)


# ------------------------------------------------------------------
# Build indexed asset from inspected asset (pure function)
# ------------------------------------------------------------------


def build_indexed_asset(
    repo_root: Path,
    inspected: InspectedAsset,
    codebase_hash: str,
) -> tuple[IndexedAsset, list[AssetInput]]:
    """Build an IndexedAsset + its inputs from an InspectedAsset.

    Handles regular assets, sensors, effects, and sinks. For sinks, the
    parent_asset_id is left None here and resolved in a second pass after
    the parent has been upserted.
    """
    file_path = Path(inspected.file_path)
    relative_file = relative_path(repo_root, file_path)
    explicit_name = inspected.decorator_metadata.get("name")

    # Sinks use a composite continuity key
    if inspected.kind == "sink":
        parent_fn = inspected.parent_function_name or ""
        continuity_key = f"{relative_file}:{parent_fn}::sink::{inspected.sink_path}"
        logical_name = continuity_key
    else:
        continuity_key = explicit_name or f"{relative_file}:{inspected.function_name}"
        logical_name = continuity_key

    filename = file_path.name or "asset.py"
    asset_slug = slugify([relative_file, filename, inspected.function_name])
    serializer_kind = inspected.decorator_metadata.get("serializer") or "json"
    decorator_json = json.dumps(inspected.decorator_metadata, separators=(",", ":"), sort_keys=True)

    # Use per-function dependency cone hash if available, fall back to codebase_hash
    effective_hash = inspected.dependency_cone_hash or codebase_hash

    # Log purity warnings
    for warning in inspected.purity_warnings:
        logger.warning(f"[{inspected.function_name}] {warning}")

    definition_hash = compute_definition_hash(
        dependency_cone_hash=effective_hash,
        function_source=inspected.function_source,
        decorator_metadata=inspected.decorator_metadata,
        serializer_kind=serializer_kind,
        python_version=inspected.python_version,
    )

    # Determine purity from original function's __unsafe__ flag.
    # The inspector has already applied the unsafe-short-circuit for dep_hash,
    # so we only need to look at the inspected decorator_metadata + a flag.
    purity = "unsafe" if _is_decorator_metadata_unsafe(inspected) else "pure"

    # Extract inputs from decorator metadata
    inputs: list[AssetInput] = []
    inputs_map = inspected.decorator_metadata.get("inputs")
    if inputs_map and isinstance(inputs_map, dict):
        for param_name, ref_value in inputs_map.items():
            if isinstance(ref_value, str):
                canonical_ref = _canonicalize_ref(ref_value, repo_root)
                inputs.append(
                    AssetInput(
                        parameter_name=param_name,
                        upstream_asset_ref=canonical_ref,
                    )
                )
            elif isinstance(ref_value, dict) and ref_value.get("kind") == "collect":
                inner_ref = ref_value.get("ref", "")
                canonical_ref = _canonicalize_ref(inner_ref, repo_root)
                inputs.append(
                    AssetInput(
                        parameter_name=param_name,
                        upstream_asset_ref=canonical_ref,
                        collect_mode=True,
                    )
                )

    # Handle dynamic partitions: add an implicit input edge
    partitions_map = inspected.decorator_metadata.get("partitions")
    if partitions_map and isinstance(partitions_map, dict):
        for dim_name, spec in partitions_map.items():
            if isinstance(spec, dict) and spec.get("kind") == "dynamic":
                upstream_ref = spec.get("upstream_ref", "")
                canonical_ref = _canonicalize_ref(upstream_ref, repo_root)
                inputs.append(
                    AssetInput(
                        parameter_name=f"__partition_source__{dim_name}",
                        upstream_asset_ref=canonical_ref,
                        is_partition_source=True,
                    )
                )

    has_inputs = len(inputs) > 0
    run_hash = "" if has_inputs else definition_hash

    indexed = IndexedAsset(
        asset_id=0,
        logical_name=logical_name,
        continuity_key=continuity_key,
        module_path=inspected.module_path,
        file_path=relative_file,
        function_name=inspected.function_name,
        asset_slug=asset_slug,
        kind=inspected.kind,
        purity=purity,
        parent_asset_id=None,  # resolved in reindex second pass for sinks
        sink_path=inspected.sink_path,
        sink_serializer=inspected.sink_serializer,
        definition_id=0,
        definition_hash=definition_hash,
        run_hash=run_hash,
        source_text=inspected.function_source,
        module_source_text=inspected.module_source,
        decorator_metadata_json=decorator_json,
        return_type=inspected.return_type,
        serializer_kind=serializer_kind,
        python_version=inspected.python_version,
        codebase_hash=codebase_hash,
        dependency_cone_hash=effective_hash,
    )
    return indexed, inputs


def _canonicalize_ref(abs_ref: str, repo_root: Path) -> str:
    """Relativize ``"{abs_path}:{func_name}"`` to ``"{rel_path}:{func_name}"``."""
    if ":" not in abs_ref:
        return abs_ref
    colon_pos = abs_ref.rfind(":")
    abs_path = abs_ref[:colon_pos]
    func_name = abs_ref[colon_pos + 1 :]
    if abs_path and Path(abs_path).is_absolute():
        try:
            rel = relative_path(repo_root, Path(abs_path))
            return f"{rel}:{func_name}"
        except Exception:
            pass
    return abs_ref


def _is_decorator_metadata_unsafe(inspected: InspectedAsset) -> bool:
    """Detect if a function is @unsafe by inspecting its AST for the marker.

    We can't reach the original function here, so we use a heuristic: no
    purity_warnings and no dep_hash means the inspector took the unsafe
    short-circuit. A cleaner approach would be to thread the flag through
    InspectedAsset — which we do as a follow-up if needed.
    """
    # The inspector sets purity_warnings=[] for unsafe. But pure functions
    # also sometimes have empty warnings. Use a separate marker instead —
    # we'll look at the original function's __unsafe__ attribute by
    # re-importing if needed. Fastest reliable check: look at function_source
    # for "@unsafe" prefix.
    src = inspected.function_source or ""
    return "@unsafe" in src


# ------------------------------------------------------------------
# Discover barca modules
# ------------------------------------------------------------------


def discover_barca_modules(root: Path) -> list[str]:
    """Walk .py files looking for barca imports, return dotted module names."""
    skip = {
        ".venv",
        "__pycache__",
        ".git",
        ".barca",
        ".barcafiles",
        "build",
        "dist",
        "node_modules",
        "target",
        "tmp",
    }
    modules = []
    for py_file in root.rglob("*.py"):
        if any(part in skip for part in py_file.parts):
            continue
        try:
            text = py_file.read_text()
        except OSError:
            continue
        if "barca" not in text:
            continue
        if "from barca" not in text and "import barca" not in text:
            continue
        rel = py_file.relative_to(root)
        parts = list(rel.parts)
        if parts[-1] == "__init__.py":
            parts = parts[:-1]
        else:
            parts[-1] = parts[-1].removesuffix(".py")
        if parts:
            modules.append(".".join(parts))
    return sorted(modules)


# ------------------------------------------------------------------
# Reindex with rename detection
# ------------------------------------------------------------------


def reindex(store: MetadataStore, repo_root: Path) -> ReindexDiff:
    """Discover assets, compute hashes, detect added/removed/renamed, upsert.

    Returns a ``ReindexDiff`` describing what changed since the last reindex.
    Spec rule: ReindexShowsDiff.
    """
    config = load_config(repo_root)
    module_names = configured_modules(config)
    if not module_names:
        module_names = discover_barca_modules(repo_root)

    root_str = str(repo_root)
    if root_str not in sys.path:
        sys.path.insert(0, root_str)

    codebase_hash = compute_codebase_hash(repo_root)
    inspected = inspect_modules(module_names, project_root=root_str)

    # Step 1: build all indexed assets (pure computation)
    new_assets: list[tuple[IndexedAsset, list[AssetInput], InspectedAsset]] = []
    seen: set[str] = set()
    for item in inspected:
        indexed, inputs = build_indexed_asset(repo_root, item, codebase_hash)
        if indexed.continuity_key in seen:
            raise ValueError(f"duplicate continuity key detected: {indexed.continuity_key}")
        seen.add(indexed.continuity_key)
        new_assets.append((indexed, inputs, item))

    new_cks = {ia.continuity_key for ia, _, _ in new_assets}

    # Step 2: load the current active set from the store
    old_summaries = store.list_assets()
    old_cks = {a.logical_name for a in old_summaries}  # logical_name == continuity_key for now
    old_by_ck: dict[str, AssetSummary] = {a.logical_name: a for a in old_summaries}

    # Step 3: detect adds and removes (raw)
    raw_added_cks = new_cks - old_cks
    raw_removed_cks = old_cks - new_cks

    # Step 4: detect renames via AST match
    # Build a lookup of added assets by normalized function source
    added_by_ast: dict[str, IndexedAsset] = {}
    for ia, _, _ in new_assets:
        if ia.continuity_key in raw_added_cks:
            ast_key = _ast_normalized_source(ia.source_text)
            added_by_ast.setdefault(ast_key, ia)

    # Build a lookup of removed assets by AST (via the store)
    renamed_pairs: list[tuple[str, str]] = []
    matched_new_cks: set[str] = set()
    matched_old_cks: set[str] = set()

    for old_ck in raw_removed_cks:
        old_id = old_by_ck[old_ck].asset_id
        try:
            old_detail = store.asset_detail(old_id)
        except Exception:
            continue
        old_ast = _ast_normalized_source(old_detail.asset.source_text)
        candidate = added_by_ast.get(old_ast)
        if candidate and candidate.continuity_key not in matched_new_cks:
            # Rename detected: update the old row in place
            store.rename_asset(
                old_asset_id=old_id,
                new_continuity_key=candidate.continuity_key,
                new_logical_name=candidate.logical_name,
                new_module_path=candidate.module_path,
                new_file_path=candidate.file_path,
                new_function_name=candidate.function_name,
            )
            renamed_pairs.append((old_ck, candidate.continuity_key))
            matched_old_cks.add(old_ck)
            matched_new_cks.add(candidate.continuity_key)

    # Step 4b: detect renames via explicit name= (secondary signal)
    # Currently the AST heuristic already handles most cases; name= match
    # is a fallback for when the body changed but the explicit name is stable.
    # Since continuity_key already incorporates name=, a rename via name=
    # would show up as neither added nor removed — there's nothing to detect.

    final_added_cks = raw_added_cks - matched_new_cks
    final_removed_cks = raw_removed_cks - matched_old_cks

    # Step 5: deactivate removed assets (history preserved)
    for removed_ck in final_removed_cks:
        old_id = old_by_ck[removed_ck].asset_id
        store.deactivate_asset(old_id)

    # Step 6: upsert all new/renamed assets, tracking sink parents
    assets_with_inputs: list[tuple[str, list[AssetInput]]] = []
    sinks_to_link: list[tuple[str, str]] = []  # (sink_ck, parent_ck)

    for indexed, inputs, inspected_item in new_assets:
        if inputs:
            assets_with_inputs.append((indexed.continuity_key, inputs))
        if indexed.kind == "sink":
            parent_fn = inspected_item.parent_function_name or ""
            parent_ck = f"{indexed.file_path}:{parent_fn}"
            sinks_to_link.append((indexed.continuity_key, parent_ck))
        store.upsert_indexed_asset(indexed)

    # Step 7: resolve sink parent ids (after parents are upserted)
    for sink_ck, parent_ck in sinks_to_link:
        sink_id = store.asset_id_by_logical_name(sink_ck)
        parent_id = store.asset_id_by_logical_name(parent_ck)
        if sink_id is None or parent_id is None:
            continue
        # Update via direct SQL since we don't have a dedicated method
        store.conn.execute(
            "UPDATE assets SET parent_asset_id = ? WHERE id = ?",
            (parent_id, sink_id),
        )
    store.conn.commit()

    # Step 8: resolve input upstream_asset_ids
    for continuity_key, inputs in assets_with_inputs:
        asset_id = store.asset_id_by_logical_name(continuity_key)
        if asset_id is None:
            raise ValueError(f"asset {continuity_key} not found after upsert")
        detail = store.asset_detail(asset_id)

        for inp in inputs:
            upstream_id = store.asset_id_by_logical_name(inp.upstream_asset_ref)
            if upstream_id is None:
                raise ValueError(f"input '{inp.parameter_name}' on asset '{continuity_key}' references unknown asset '{inp.upstream_asset_ref}'")
            inp.upstream_asset_id = upstream_id

        store.upsert_asset_inputs(detail.asset.definition_id, inputs)

    return ReindexDiff(
        added=sorted(final_added_cks),
        removed=sorted(final_removed_cks),
        renamed=sorted(renamed_pairs),
    )


def _ast_normalized_source(source: str) -> str:
    """Return a normalized form of function source for AST-based matching.

    Strips the function header (def ...) and decorator lines so that moving
    a function + renaming decorators doesn't defeat the match. Hash the
    result for stable comparison.
    """
    lines = source.splitlines()
    body_lines = [line for line in lines if not line.strip().startswith("@") and not line.strip().startswith("def ")]
    body = "\n".join(line.strip() for line in body_lines if line.strip())
    return hashlib.sha256(body.encode()).hexdigest()


# ------------------------------------------------------------------
# Explicit refresh with stale_policy
# ------------------------------------------------------------------


def refresh(
    store: MetadataStore,
    repo_root: Path,
    asset_id: int,
    *,
    max_workers: int | None = None,
    stale_policy: str = "error",
) -> AssetDetail:
    """Explicit refresh of a single asset.

    Spec rule ExplicitRefresh: materialises ``asset_id`` using whatever
    upstream materialisations exist right now. Does NOT cascade upstream.
    If any upstream is stale, ``stale_policy`` governs behaviour:

    - ``error`` (default): raise ``StaleUpstreamError`` with the list of
      stale upstreams.
    - ``warn``: proceed with stale inputs, record ``stale_inputs_used=True``,
      emit a warning via ``logging``.
    - ``pass``: proceed silently with stale inputs, record ``stale_inputs_used=True``.
    """
    detail = store.asset_detail(asset_id)

    if detail.asset.kind == "sensor":
        raise ValueError(f"asset #{asset_id} is a sensor — use trigger_sensor() instead of refresh()")
    if detail.asset.kind in ("effect", "sink"):
        raise ValueError(f"asset #{asset_id} is a {detail.asset.kind} — {detail.asset.kind}s cannot be directly refreshed. They materialise automatically when their upstream does.")

    asset_inputs = _resolve_asset_inputs(store, detail, repo_root)

    # Check which upstreams are fresh
    stale_upstreams: list[str] = []
    for inp in asset_inputs:
        if inp.upstream_asset_id is None or inp.is_partition_source:
            continue
        upstream_detail = store.asset_detail(inp.upstream_asset_id)
        existing = store.latest_successful_materialization(inp.upstream_asset_id)
        if existing is None or existing.definition_id != upstream_detail.asset.definition_id:
            stale_upstreams.append(inp.upstream_asset_ref)

    stale_inputs_used = False
    if stale_upstreams:
        if stale_policy == "error":
            raise StaleUpstreamError(
                f"upstream assets are stale: {', '.join(stale_upstreams)} — refresh upstream first, or use --stale-policy=warn/pass.",
                stale_upstreams=stale_upstreams,
            )
        elif stale_policy == "warn":
            logger.warning(f"refreshing {detail.asset.continuity_key} with stale upstream inputs: {', '.join(stale_upstreams)}")
            stale_inputs_used = True
        elif stale_policy == "pass":
            stale_inputs_used = True
        else:
            raise ValueError(f"unknown stale_policy: {stale_policy!r}")

    partition_values = _resolve_partition_values(store, detail, repo_root)

    if partition_values:
        # Partitioned materialisation — input loading is deferred to the
        # per-partition loop inside _refresh_partitioned so each partition
        # picks up the matching upstream partition (inheritance).
        return _refresh_partitioned(
            store,
            repo_root,
            detail,
            asset_inputs,
            partition_values,
            max_workers=max_workers,
            stale_inputs_used=stale_inputs_used,
        )

    # Non-partitioned: load inputs once for the single materialisation
    input_kwargs, upstream_mat_ids = _gather_inputs_for_partition(store, repo_root, asset_inputs, partition_key_json=None)
    return _refresh_single(
        store,
        repo_root,
        detail,
        asset_inputs,
        input_kwargs,
        upstream_mat_ids,
        stale_inputs_used=stale_inputs_used,
    )


def trigger_sensor(store: MetadataStore, repo_root: Path, asset_id: int) -> SensorObservation:
    """Execute a sensor and record its observation."""
    detail = store.asset_detail(asset_id)

    if detail.asset.kind != "sensor":
        raise ValueError(f"asset #{asset_id} is not a sensor")

    try:
        mod = importlib.import_module(detail.asset.module_path)
        func = getattr(mod, detail.asset.function_name)
        original = getattr(func, "__barca_original__", func)
        sensor_result = original()
    except Exception as exc:
        # Record a failed observation so the failure is visible
        store.insert_sensor_observation(
            asset_id,
            detail.asset.definition_id,
            False,
            json.dumps({"error": str(exc)}),
        )
        raise

    if not isinstance(sensor_result, tuple) or len(sensor_result) != 2:
        error_msg = f"sensor '{detail.asset.function_name}' must return (update_detected: bool, output), got {type(sensor_result).__name__}"
        store.insert_sensor_observation(
            asset_id,
            detail.asset.definition_id,
            False,
            json.dumps({"error": error_msg}),
        )
        raise ValueError(error_msg)

    update_detected, _output = sensor_result
    # Store the full tuple so downstream assets receive (bool, value)
    output_json = json.dumps(list(sensor_result))

    store.insert_sensor_observation(
        asset_id,
        detail.asset.definition_id,
        bool(update_detected),
        output_json,
    )

    obs = store.latest_sensor_observation(asset_id)
    if obs is None:
        raise RuntimeError(f"sensor observation not found after insert for asset {asset_id}")
    return obs


def _refresh_single(
    store: MetadataStore,
    repo_root: Path,
    detail: AssetDetail,
    asset_inputs: list[AssetInput],
    input_kwargs: dict,
    upstream_mat_ids: list[int],
    *,
    stale_inputs_used: bool = False,
) -> AssetDetail:
    has_real_inputs = any(not inp.is_partition_source for inp in asset_inputs if inp.upstream_asset_id is not None)
    run_hash = compute_run_hash(detail.asset.definition_hash, upstream_mat_ids) if has_real_inputs else detail.asset.definition_hash

    existing = store.successful_materialization_for_run(detail.asset.asset_id, run_hash)
    if existing:
        return store.asset_detail(detail.asset.asset_id)

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
            stale_inputs_used=stale_inputs_used,
        )
    except Exception as e:
        store.mark_materialization_failed(mat_id, str(e))
        raise

    # After a successful regular-asset materialization, run its attached sinks
    if detail.asset.kind == "asset":
        _materialize_sinks(store, repo_root, detail, run_hash)

    return store.asset_detail(detail.asset.asset_id)


def _refresh_partitioned(
    store: MetadataStore,
    repo_root: Path,
    detail: AssetDetail,
    asset_inputs: list[AssetInput],
    partition_values: list[dict],
    *,
    max_workers: int | None = None,
    stale_inputs_used: bool = False,
) -> AssetDetail:
    """Materialise a partitioned asset.

    For each partition value, loads inputs with partition-matched lookup
    (so a downstream of a partitioned upstream picks the corresponding
    upstream partition), computes a per-partition run_hash, and executes.
    """
    # Determine whether partition values should be merged into the function
    # kwargs. When the asset has an explicit ``partitions=`` declaration,
    # its function signature typically accepts the partition dim as a
    # parameter (e.g. ``def sales(region: str)``). When inheriting from
    # upstream, the function's signature only knows its declared inputs.
    try:
        _meta = json.loads(detail.asset.decorator_metadata_json)
    except (json.JSONDecodeError, TypeError):
        _meta = {}
    has_explicit_partitions = bool(_meta.get("partitions") and isinstance(_meta["partitions"], dict))

    work_items: list[tuple[int, str, str, dict, Path]] = []
    for pv in partition_values:
        pk_json = json.dumps(pv, separators=(",", ":"), sort_keys=True)

        # Load inputs for this specific partition. Partition-matched lookup
        # lets a downstream pick its upstream's matching partition value.
        try:
            per_partition_kwargs, upstream_mat_ids = _gather_inputs_for_partition(store, repo_root, asset_inputs, pk_json)
        except StaleUpstreamError as exc:
            logger.warning(f"skipping partition {pv}: {exc}")
            continue

        run_hash = compute_run_hash(
            detail.asset.definition_hash,
            upstream_mat_ids,
            pk_json,
        )

        existing = store.successful_materialization_for_run(detail.asset.asset_id, run_hash)
        if existing:
            continue

        mat_id = store.insert_queued_materialization(
            detail.asset.asset_id,
            detail.asset.definition_id,
            run_hash,
            pk_json,
        )

        # Only merge partition key values into kwargs when the asset has
        # an explicit partitions= declaration. Inherited partitions don't
        # appear in the downstream's function signature.
        merged_kwargs = {**per_partition_kwargs}
        if has_explicit_partitions:
            for k, v in pv.items():
                if k not in merged_kwargs:
                    merged_kwargs[k] = v

        artifact_base = Path(".barcafiles") / detail.asset.asset_slug / detail.asset.definition_hash
        if isinstance(pv, dict):
            parts = sorted(f"{k_}={v_}" if isinstance(v_, str) else f"{k_}={json.dumps(v_)}" for k_, v_ in pv.items())
            artifact_base = artifact_base / "partitions" / ",".join(parts)

        artifact_dir = repo_root / artifact_base
        artifact_dir.mkdir(parents=True, exist_ok=True)

        work_items.append((mat_id, run_hash, pk_json, merged_kwargs, artifact_dir))

    if not work_items:
        return store.asset_detail(detail.asset.asset_id)

    effective_workers = max_workers if max_workers is not None else os.cpu_count() or 1

    if effective_workers <= 1:
        results: list[tuple[int, str, str, Path]] = []
        for mat_id, run_hash, pk_json, kwargs, artifact_dir in work_items:
            try:
                value_path = materialize_asset(
                    detail.asset.module_path,
                    detail.asset.function_name,
                    artifact_dir,
                    kwargs if kwargs else None,
                )
                results.append((mat_id, run_hash, pk_json, value_path))
            except Exception as e:
                store.mark_materialization_failed(mat_id, str(e))
                # Don't raise — let other partitions complete
                continue
    else:
        results = []
        with ThreadPoolExecutor(max_workers=effective_workers) as pool:
            future_to_meta = {}
            for mat_id, run_hash, pk_json, kwargs, artifact_dir in work_items:
                future = pool.submit(
                    materialize_asset,
                    detail.asset.module_path,
                    detail.asset.function_name,
                    artifact_dir,
                    kwargs if kwargs else None,
                )
                future_to_meta[future] = (mat_id, run_hash, pk_json)

            for future in as_completed(future_to_meta):
                mat_id, run_hash, pk_json = future_to_meta[future]
                try:
                    value_path = future.result()
                    results.append((mat_id, run_hash, pk_json, value_path))
                except Exception as e:
                    store.mark_materialization_failed(mat_id, str(e))

    for mat_id, _run_hash, _pk_json, value_path in results:
        value_bytes = value_path.read_bytes()
        artifact_checksum = sha256_hex(value_bytes)
        artifact_path_rel = relative_path(repo_root, value_path)
        artifact_dir = value_path.parent
        (artifact_dir / "code.txt").write_text(detail.asset.source_text)
        store.mark_materialization_success(
            mat_id,
            artifact_path_rel,
            "json",
            artifact_checksum,
            stale_inputs_used=stale_inputs_used,
        )

    # Run sinks if any partitions succeeded
    if detail.asset.kind == "asset" and results:
        _materialize_sinks(store, repo_root, detail, results[0][1])

    return store.asset_detail(detail.asset.asset_id)


def _execute_materialization(
    store: MetadataStore,
    repo_root: Path,
    detail: AssetDetail,
    mat_id: int,
    run_hash: str,
    input_kwargs: dict,
    upstream_mat_ids: list[int],
    partition_key_json: str | None = None,
    *,
    stale_inputs_used: bool = False,
) -> None:
    store.update_materialization_run_hash(mat_id, run_hash)

    artifact_base = Path(".barcafiles") / detail.asset.asset_slug / detail.asset.definition_hash
    if partition_key_json:
        pk = json.loads(partition_key_json)
        if isinstance(pk, dict):
            parts = sorted(f"{k}={v}" if isinstance(v, str) else f"{k}={json.dumps(v)}" for k, v in pk.items())
            artifact_base = artifact_base / "partitions" / ",".join(parts)

    artifact_dir = repo_root / artifact_base
    artifact_dir.mkdir(parents=True, exist_ok=True)

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
        stale_inputs_used=stale_inputs_used,
    )


def materialize_asset(
    module_path: str,
    function_name: str,
    output_dir: Path,
    input_kwargs: dict | None = None,
) -> Path:
    """Import module, call function, save result as JSON."""
    mod = importlib.import_module(module_path)
    func = getattr(mod, function_name)
    original = getattr(func, "__barca_original__", func)

    if input_kwargs:
        result = original(**input_kwargs)
    else:
        result = original()

    value_path = output_dir / "value.json"
    value_path.write_text(json.dumps(result, default=_json_default))
    return value_path


def _json_default(obj):
    if isinstance(obj, tuple):
        return list(obj)
    raise TypeError(f"Object of type {type(obj).__name__} is not JSON serializable")


# ------------------------------------------------------------------
# Sink materialisation
# ------------------------------------------------------------------


def _materialize_sinks(
    store: MetadataStore,
    repo_root: Path,
    parent_detail: AssetDetail,
    parent_run_hash: str,
) -> list[dict]:
    """Run all sinks attached to a freshly-materialised parent asset.

    Returns a list of ``{"status": "success"|"failed", "sink_id": int}``
    for each sink executed.
    """
    parent_id = parent_detail.asset.asset_id
    # Find all sinks whose parent is this asset
    rows = store.conn.execute(
        """SELECT a.id, a.logical_name, a.sink_path, a.sink_serializer,
                  d.id, d.definition_hash, d.source_text
           FROM assets a
           JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
           WHERE a.kind = 'sink' AND a.parent_asset_id = ? AND a.active = 1""",
        (parent_id,),
    ).fetchall()

    results: list[dict] = []

    # Load the parent's output value (it was just written)
    parent_mat = store.latest_successful_materialization(parent_id)
    if parent_mat is None or not parent_mat.artifact_path:
        return results
    parent_value = json.loads((repo_root / parent_mat.artifact_path).read_text())

    for sink_row in rows:
        sink_id = sink_row[0]
        sink_path = sink_row[2]
        serializer = sink_row[3] or "json"
        definition_id = sink_row[4]

        # Cache check: has this exact (sink_id, parent_run_hash) already succeeded?
        existing = store.successful_sink_execution_for_run(sink_id, parent_run_hash)
        if existing:
            results.append({"status": "success", "sink_id": sink_id, "cached": True})
            continue

        try:
            _write_sink(sink_path, parent_value, serializer, repo_root)
            store.insert_sink_execution(
                asset_id=sink_id,
                definition_id=definition_id,
                run_hash=parent_run_hash,
                status="success",
                destination_path=sink_path,
            )
            results.append({"status": "success", "sink_id": sink_id})
        except Exception as exc:
            store.insert_sink_execution(
                asset_id=sink_id,
                definition_id=definition_id,
                run_hash=parent_run_hash,
                status="failed",
                destination_path=sink_path,
                last_error=str(exc),
            )
            logger.warning(f"sink {sink_path} failed: {exc}")
            results.append({"status": "failed", "sink_id": sink_id, "error": str(exc)})

    return results


def _write_sink(path: str, value: Any, serializer: str, repo_root: Path) -> None:
    """Write ``value`` to ``path`` using the given serializer.

    Path handling:
      - Relative paths are resolved relative to ``repo_root``
      - Absolute paths are used as-is
      - ``file://``, ``s3://``, ``gs://``, etc. go through fsspec when available
    """
    if path.startswith(("s3://", "gs://", "file://", "memory://")):
        try:
            import fsspec

            with fsspec.open(path, "w") as f:
                if serializer == "json":
                    f.write(json.dumps(value))
                elif serializer == "text":
                    f.write(str(value))
                else:
                    f.write(json.dumps(value))
            return
        except ImportError as exc:  # pragma: no cover
            raise RuntimeError(f"sink path {path!r} requires fsspec — install with `uv add fsspec`") from exc

    # Local path
    target = Path(path)
    if not target.is_absolute():
        target = repo_root / target
    target.parent.mkdir(parents=True, exist_ok=True)
    if serializer == "json":
        target.write_text(json.dumps(value))
    elif serializer == "text":
        target.write_text(str(value))
    else:
        target.write_text(json.dumps(value))


# ------------------------------------------------------------------
# Resolve helpers
# ------------------------------------------------------------------


def _resolve_asset_inputs(
    store: MetadataStore,
    detail: AssetDetail,
    repo_root: Path,
) -> list[AssetInput]:
    """Resolve inputs: try asset_inputs table first, fall back to decorator metadata."""
    inputs = store.get_asset_inputs(detail.asset.definition_id)
    if inputs:
        return inputs

    try:
        meta = json.loads(detail.asset.decorator_metadata_json)
    except (json.JSONDecodeError, TypeError):
        return []

    inputs_obj = meta.get("inputs")
    if not inputs_obj or not isinstance(inputs_obj, dict):
        return []

    result: list[AssetInput] = []
    for param_name, ref_value in inputs_obj.items():
        if isinstance(ref_value, str):
            canonical_ref = _canonicalize_ref(ref_value, repo_root)
            upstream_id = store.asset_id_by_logical_name(canonical_ref)
            result.append(
                AssetInput(
                    parameter_name=param_name,
                    upstream_asset_ref=canonical_ref,
                    upstream_asset_id=upstream_id,
                )
            )
        elif isinstance(ref_value, dict) and ref_value.get("kind") == "collect":
            inner = ref_value.get("ref", "")
            canonical_ref = _canonicalize_ref(inner, repo_root)
            upstream_id = store.asset_id_by_logical_name(canonical_ref)
            result.append(
                AssetInput(
                    parameter_name=param_name,
                    upstream_asset_ref=canonical_ref,
                    upstream_asset_id=upstream_id,
                    collect_mode=True,
                )
            )

    if result:
        store.upsert_asset_inputs(detail.asset.definition_id, result)

    return result


def _load_input_value(
    store: MetadataStore,
    repo_root: Path,
    inp: AssetInput,
    partition_key_json: str | None = None,
) -> tuple[Any, int]:
    """Load the value for a single input.

    Handles all input kinds:
      - Sensor upstream: returns the latest observation as a full tuple
      - Collect mode: returns dict[tuple, T] of all partitions
      - Partition-matched: returns the upstream's matching partition
      - Regular: returns the upstream's latest successful materialization

    Returns ``(value, mat_id)``. ``mat_id`` is -1 for collect mode
    (which has no single upstream mat id).
    """
    up_id = inp.upstream_asset_id
    if up_id is None:
        raise StaleUpstreamError(f"input {inp.parameter_name!r} has no upstream_asset_id")

    try:
        up_detail = store.asset_detail(up_id)
    except ValueError as exc:
        raise StaleUpstreamError(f"upstream asset #{up_id} not found") from exc

    # Sensors store observations, not materializations
    if up_detail.asset.kind == "sensor":
        obs = store.latest_sensor_observation(up_id)
        if obs is None:
            raise StaleUpstreamError(f"upstream sensor #{up_id} has no observation")
        tup = json.loads(obs.output_json) if obs.output_json else [False, None]
        value = tuple(tup) if isinstance(tup, list) else tup
        return (value, obs.observation_id)

    # Collect mode: load all partitions as a dict
    if inp.collect_mode:
        value = _load_collect_input(store, repo_root, up_id)
        return (value, -1)

    # Try partition-matched lookup first (for partition inheritance)
    if partition_key_json is not None:
        mat = store.latest_successful_materialization_for_partition(up_id, partition_key_json)
        if mat is not None and mat.artifact_path:
            value = json.loads((repo_root / mat.artifact_path).read_text())
            return (value, mat.materialization_id)

    # Fall back to the latest successful materialization
    mat = store.latest_successful_materialization(up_id)
    if mat is None:
        raise StaleUpstreamError(f"upstream asset #{up_id} has no successful materialization")
    if not mat.artifact_path:
        raise StaleUpstreamError(f"upstream asset #{up_id} has no artifact path")
    value = json.loads((repo_root / mat.artifact_path).read_text())
    return (value, mat.materialization_id)


def _gather_inputs_for_partition(
    store: MetadataStore,
    repo_root: Path,
    asset_inputs: list[AssetInput],
    partition_key_json: str | None,
) -> tuple[dict, list[int]]:
    """Load all inputs for a single partition (or the non-partitioned case).

    Returns ``(kwargs, upstream_mat_ids)``. Raises ``StaleUpstreamError`` if
    any input cannot be loaded.
    """
    kwargs: dict = {}
    mat_ids: list[int] = []
    for inp in asset_inputs:
        if inp.upstream_asset_id is None or inp.is_partition_source:
            continue
        value, mat_id = _load_input_value(store, repo_root, inp, partition_key_json)
        kwargs[inp.parameter_name] = value
        if mat_id >= 0:
            mat_ids.append(mat_id)
    return kwargs, mat_ids


def _load_collect_input(store: MetadataStore, repo_root: Path, upstream_asset_id: int) -> dict[tuple, Any]:
    """Load all partition materializations for an upstream as dict[tuple, T].

    Spec rule CollectPartitions: all-or-nothing failure handling. If any
    partition of the upstream has a failed materialization at the current
    definition, this raises ``StaleUpstreamError`` so the downstream does
    not materialise.
    """
    mats = store.list_materializations(upstream_asset_id, limit=10000)
    # Check for any failed partition at the current definition
    failed_keys: list[str] = []
    for mat in mats:
        if mat.status == "failed":
            failed_keys.append(mat.partition_key_json or "<unpartitioned>")
    if failed_keys:
        raise StaleUpstreamError(f"collect() upstream asset #{upstream_asset_id} has failed partitions: {failed_keys}. All partitions must succeed.")

    result: dict[tuple, Any] = {}
    for mat in mats:
        if mat.status != "success" or not mat.artifact_path:
            continue
        if mat.partition_key_json is None:
            value = json.loads((repo_root / mat.artifact_path).read_text())
            result[()] = value
            continue
        pk = json.loads(mat.partition_key_json)
        if isinstance(pk, dict):
            key = tuple(str(pk[k]) for k in sorted(pk.keys()))
        else:
            key = (str(pk),)
        if key not in result:
            value = json.loads((repo_root / mat.artifact_path).read_text())
            result[key] = value
    return result


def _resolve_partition_values(
    store: MetadataStore,
    detail: AssetDetail,
    repo_root: Path | None = None,
) -> list[dict]:
    """Extract partition values for an asset.

    Priority order:
    1. Explicit ``partitions=`` declaration (static or dynamic)
    2. Inheritance from partitioned upstream assets (spec rule PartitionInheritance)
    3. No partitioning — return []

    Spec rule PartitionSetResolvedLazily: if a dynamic partition's upstream
    has not materialised yet, return [] so the asset is skipped on this pass.
    """
    try:
        meta = json.loads(detail.asset.decorator_metadata_json)
    except (json.JSONDecodeError, TypeError):
        meta = {}

    partitions_obj = meta.get("partitions")

    # Path 1: explicit partitions (static or dynamic)
    if partitions_obj and isinstance(partitions_obj, dict):
        return _resolve_explicit_partitions(store, detail, repo_root, partitions_obj)

    # Path 2: inherit from partitioned upstreams
    return _inherit_partitions_from_upstreams(store, detail)


def _resolve_explicit_partitions(
    store: MetadataStore,
    detail: AssetDetail,
    repo_root: Path | None,
    partitions_obj: dict,
) -> list[dict]:
    """Resolve the partition set for an asset with an explicit ``partitions=``."""
    effective_root = repo_root if repo_root is not None else Path.cwd()

    dim_values: dict[str, list] = {}
    any_dynamic_pending = False
    for dim_name, spec in partitions_obj.items():
        if not isinstance(spec, dict):
            continue
        if spec.get("kind") == "inline":
            values_json = spec.get("values_json")
            if not values_json:
                return []
            try:
                dim_values[dim_name] = json.loads(values_json)
            except json.JSONDecodeError:
                return []
        elif spec.get("kind") == "dynamic":
            upstream_id = None
            for inp in store.get_asset_inputs(detail.asset.definition_id):
                if inp.is_partition_source and inp.parameter_name == f"__partition_source__{dim_name}":
                    upstream_id = inp.upstream_asset_id
                    break
            if upstream_id is None:
                any_dynamic_pending = True
                continue
            upstream_mat = store.latest_successful_materialization(upstream_id)
            if upstream_mat is None or not upstream_mat.artifact_path:
                any_dynamic_pending = True
                continue
            try:
                values = json.loads((effective_root / upstream_mat.artifact_path).read_text())
                if not isinstance(values, list):
                    any_dynamic_pending = True
                    continue
                dim_values[dim_name] = values
            except Exception:
                any_dynamic_pending = True
                continue

    if any_dynamic_pending or not dim_values:
        return []

    from itertools import product as _product

    dim_names = list(dim_values.keys())
    value_lists = [dim_values[name] for name in dim_names]
    result: list[dict] = []
    for combo in _product(*value_lists):
        result.append(dict(zip(dim_names, combo, strict=False)))
    return result


def _inherit_partitions_from_upstreams(
    store: MetadataStore,
    detail: AssetDetail,
) -> list[dict]:
    """Infer partition values from the partitioned upstreams of this asset.

    Spec rule PartitionInheritance: "A downstream asset that declares no
    partitions of its own, but depends on a partitioned upstream, automatically
    inherits the same partition set and runs once per partition value
    (1:1 matching by key)."

    Returns [] if:
      - No upstreams are partitioned
      - No upstream has been successfully materialised yet (pending)
      - Partitioned upstreams disagree on their partition set
    """
    inputs = store.get_asset_inputs(detail.asset.definition_id)

    upstream_partition_sets: list[set[tuple]] = []
    for inp in inputs:
        if inp.collect_mode or inp.is_partition_source:
            continue
        if inp.upstream_asset_id is None:
            continue

        mats = store.list_materializations(inp.upstream_asset_id, limit=10000)
        partition_keys: set[tuple] = set()
        has_partitioned_mat = False
        for mat in mats:
            if mat.status != "success":
                continue
            if not mat.partition_key_json:
                continue
            has_partitioned_mat = True
            try:
                pk = json.loads(mat.partition_key_json)
            except json.JSONDecodeError:
                continue
            if isinstance(pk, dict):
                key_tuple = tuple(sorted(pk.items()))
                partition_keys.add(key_tuple)

        if has_partitioned_mat:
            upstream_partition_sets.append(partition_keys)

    if not upstream_partition_sets:
        # No partitioned upstreams — fall through to single materialization
        return []

    # All partitioned upstreams must agree on the partition set
    first = upstream_partition_sets[0]
    for other in upstream_partition_sets[1:]:
        if other != first:
            # Mismatch — don't inherit. Could log a warning.
            return []

    return [dict(key_tuple) for key_tuple in first]


# ------------------------------------------------------------------
# Reset
# ------------------------------------------------------------------


_RESET_TARGETS = [
    (".barca", "db"),
    (".barcafiles", "artifacts"),
    ("tmp", "tmp"),
]


def reset(
    repo_root: Path,
    *,
    db: bool = False,
    artifacts: bool = False,
    tmp: bool = False,
) -> str:
    """Remove generated files and caches."""
    all_targets = not db and not artifacts and not tmp
    flags = [db, artifacts, tmp]

    lines: list[str] = []
    for i, (dirname, _label) in enumerate(_RESET_TARGETS):
        if not all_targets and not flags[i]:
            continue
        path = repo_root / dirname
        if path.exists():
            shutil.rmtree(path)
            lines.append(f"removed {dirname}/")

    if not lines:
        lines.append("nothing to reset")

    return "\n".join(lines) + "\n"
