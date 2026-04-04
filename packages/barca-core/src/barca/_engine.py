"""Core orchestration — reindex, refresh, materialize."""

from __future__ import annotations

import importlib
import json
import shutil
import sys
from pathlib import Path

from barca._config import configured_modules, load_config
from barca._hashing import (
    compute_codebase_hash,
    compute_definition_hash,
    compute_run_hash,
    now_ts,
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
    MaterializationRecord,
)
from barca._store import MetadataStore


# ------------------------------------------------------------------
# Build indexed asset from inspected asset (pure function)
# ------------------------------------------------------------------


def build_indexed_asset(
    repo_root: Path,
    inspected: InspectedAsset,
    codebase_hash: str,
) -> tuple[IndexedAsset, list[AssetInput]]:
    if inspected.kind != "asset":
        raise ValueError(f"unsupported node kind: {inspected.kind}")

    file_path = Path(inspected.file_path)
    relative_file = relative_path(repo_root, file_path)
    explicit_name = inspected.decorator_metadata.get("name")
    continuity_key = explicit_name or f"{relative_file}:{inspected.function_name}"
    logical_name = continuity_key
    filename = file_path.name or "asset.py"
    asset_slug = slugify([relative_file, filename, inspected.function_name])
    serializer_kind = inspected.decorator_metadata.get("serializer") or "json"
    decorator_json = json.dumps(inspected.decorator_metadata, separators=(",", ":"))

    # Use per-function dependency cone hash if available, fall back to codebase_hash
    effective_hash = inspected.dependency_cone_hash or codebase_hash

    # Log purity warnings
    for warning in inspected.purity_warnings:
        print(f"barca: WARNING [{inspected.function_name}] {warning}", file=sys.stderr)

    definition_hash = compute_definition_hash(
        dependency_cone_hash=effective_hash,
        function_source=inspected.function_source,
        decorator_metadata=inspected.decorator_metadata,
        serializer_kind=serializer_kind,
        python_version=inspected.python_version,
    )

    # Extract inputs from decorator metadata
    inputs: list[AssetInput] = []
    inputs_map = inspected.decorator_metadata.get("inputs")
    if inputs_map and isinstance(inputs_map, dict):
        for param_name, abs_ref in inputs_map.items():
            if isinstance(abs_ref, str):
                # Relativize: "{abs_path}:{func_name}" -> "{rel_path}:{func_name}"
                if ":" in abs_ref:
                    colon_pos = abs_ref.rfind(":")
                    abs_path = abs_ref[:colon_pos]
                    func_name = abs_ref[colon_pos + 1:]
                    rel = relative_path(repo_root, Path(abs_path))
                    canonical_ref = f"{rel}:{func_name}"
                else:
                    canonical_ref = abs_ref
                inputs.append(AssetInput(
                    parameter_name=param_name,
                    upstream_asset_ref=canonical_ref,
                ))

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


# ------------------------------------------------------------------
# Discover barca modules
# ------------------------------------------------------------------


def discover_barca_modules(root: Path) -> list[str]:
    """Walk .py files looking for barca imports, return dotted module names."""
    skip = {".venv", "__pycache__", ".git", ".barca", ".barcafiles",
            "build", "dist", "node_modules", "target", "tmp"}
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
        # Convert file path to dotted module name
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
# Reindex
# ------------------------------------------------------------------


def reindex(store: MetadataStore, repo_root: Path) -> list[AssetSummary]:
    """Discover assets, compute hashes, upsert to DB. Returns asset list."""
    config = load_config(repo_root)
    module_names = configured_modules(config)
    if not module_names:
        module_names = discover_barca_modules(repo_root)

    # Add project root to sys.path so imports work
    root_str = str(repo_root)
    if root_str not in sys.path:
        sys.path.insert(0, root_str)

    codebase_hash = compute_codebase_hash(repo_root)
    inspected = inspect_modules(module_names, project_root=root_str)

    seen: set[str] = set()
    assets_with_inputs: list[tuple[str, list[AssetInput]]] = []

    # First pass: upsert all assets
    for item in inspected:
        indexed, inputs = build_indexed_asset(repo_root, item, codebase_hash)
        if indexed.continuity_key in seen:
            raise ValueError(f"duplicate continuity key detected: {indexed.continuity_key}")
        seen.add(indexed.continuity_key)
        if inputs:
            assets_with_inputs.append((indexed.continuity_key, inputs))
        store.upsert_indexed_asset(indexed)

    # Second pass: resolve input upstream_asset_ids
    for continuity_key, inputs in assets_with_inputs:
        asset_id = store.asset_id_by_logical_name(continuity_key)
        if asset_id is None:
            raise ValueError(f"asset {continuity_key} not found after upsert")
        detail = store.asset_detail(asset_id)

        for inp in inputs:
            upstream_id = store.asset_id_by_logical_name(inp.upstream_asset_ref)
            if upstream_id is None:
                raise ValueError(
                    f"input '{inp.parameter_name}' on asset '{continuity_key}' "
                    f"references unknown asset '{inp.upstream_asset_ref}'"
                )
            inp.upstream_asset_id = upstream_id

        store.upsert_asset_inputs(detail.asset.definition_id, inputs)

    return store.list_assets()


# ------------------------------------------------------------------
# Refresh (materialize)
# ------------------------------------------------------------------


def refresh(store: MetadataStore, repo_root: Path, asset_id: int) -> AssetDetail:
    """Materialize an asset and its upstream deps recursively. Returns final detail."""
    detail = store.asset_detail(asset_id)

    # Resolve inputs
    asset_inputs = _resolve_asset_inputs(store, detail, repo_root)

    # Recursively ensure upstream deps are materialized with current definition
    for inp in asset_inputs:
        upstream_id = inp.upstream_asset_id
        if upstream_id is None:
            continue
        upstream_detail = store.asset_detail(upstream_id)
        existing = store.latest_successful_materialization(upstream_id)
        # Re-materialize if no successful materialization exists, or if the
        # existing one was produced by a different definition (stale).
        if existing is None or existing.definition_id != upstream_detail.asset.definition_id:
            refresh(store, repo_root, upstream_id)

    # Resolve partition values
    partition_values = _resolve_partition_values(detail)

    # Compute run_hash
    upstream_mat_ids: list[int] = []
    input_kwargs: dict = {}

    for inp in asset_inputs:
        upstream_id = inp.upstream_asset_id
        if upstream_id is None:
            continue
        upstream_mat = store.latest_successful_materialization(upstream_id)
        if upstream_mat is None:
            raise ValueError(f"upstream asset #{upstream_id} has no successful materialization")

        # Load upstream artifact
        artifact_path = upstream_mat.artifact_path
        if not artifact_path:
            raise ValueError(f"upstream asset #{upstream_id} has no artifact path")
        full_path = repo_root / artifact_path
        value = json.loads(full_path.read_text())

        input_kwargs[inp.parameter_name] = value
        upstream_mat_ids.append(upstream_mat.materialization_id)

    upstream_mat_ids.sort()

    if partition_values:
        return _refresh_partitioned(
            store, repo_root, detail, asset_inputs,
            input_kwargs, upstream_mat_ids, partition_values,
        )
    else:
        return _refresh_single(
            store, repo_root, detail, asset_inputs,
            input_kwargs, upstream_mat_ids,
        )


def _refresh_single(
    store: MetadataStore,
    repo_root: Path,
    detail: AssetDetail,
    asset_inputs: list[AssetInput],
    input_kwargs: dict,
    upstream_mat_ids: list[int],
) -> AssetDetail:
    has_inputs = len(asset_inputs) > 0
    run_hash = (
        compute_run_hash(detail.asset.definition_hash, upstream_mat_ids)
        if has_inputs
        else detail.asset.definition_hash
    )

    # Check cache
    existing = store.successful_materialization_for_run(detail.asset.asset_id, run_hash)
    if existing:
        return store.asset_detail(detail.asset.asset_id)

    # Enqueue and execute
    mat_id = store.insert_queued_materialization(
        detail.asset.asset_id, detail.asset.definition_id, run_hash,
    )

    try:
        _execute_materialization(
            store, repo_root, detail, mat_id, run_hash,
            input_kwargs, upstream_mat_ids,
        )
    except Exception as e:
        store.mark_materialization_failed(mat_id, str(e))
        raise

    return store.asset_detail(detail.asset.asset_id)


def _refresh_partitioned(
    store: MetadataStore,
    repo_root: Path,
    detail: AssetDetail,
    asset_inputs: list[AssetInput],
    base_input_kwargs: dict,
    upstream_mat_ids: list[int],
    partition_values: list[dict],
) -> AssetDetail:
    for pv in partition_values:
        pk_json = json.dumps(pv, separators=(",", ":"))
        run_hash = compute_run_hash(
            detail.asset.definition_hash, upstream_mat_ids, pk_json,
        )

        # Check cache
        existing = store.successful_materialization_for_run(detail.asset.asset_id, run_hash)
        if existing:
            continue

        mat_id = store.insert_queued_materialization(
            detail.asset.asset_id, detail.asset.definition_id, run_hash, pk_json,
        )

        # Merge partition key into kwargs
        merged_kwargs = {**base_input_kwargs}
        for k, v in pv.items():
            merged_kwargs[k] = v

        try:
            _execute_materialization(
                store, repo_root, detail, mat_id, run_hash,
                merged_kwargs, upstream_mat_ids,
                partition_key_json=pk_json,
            )
        except Exception as e:
            store.mark_materialization_failed(mat_id, str(e))
            raise

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
) -> None:
    # Update run_hash on the job
    store.update_materialization_run_hash(mat_id, run_hash)

    # Build artifact path
    artifact_base = Path(".barcafiles") / detail.asset.asset_slug / detail.asset.definition_hash
    if partition_key_json:
        pk = json.loads(partition_key_json)
        if isinstance(pk, dict):
            parts = sorted(
                f"{k}={v}" if isinstance(v, str) else f"{k}={json.dumps(v)}"
                for k, v in pk.items()
            )
            artifact_base = artifact_base / "partitions" / ",".join(parts)

    artifact_dir = repo_root / artifact_base
    artifact_dir.mkdir(parents=True, exist_ok=True)

    # Materialize
    value_path = materialize_asset(
        detail.asset.module_path,
        detail.asset.function_name,
        artifact_dir,
        input_kwargs if input_kwargs else None,
    )

    # Compute checksum
    value_bytes = value_path.read_bytes()
    artifact_checksum = sha256_hex(value_bytes)
    artifact_path_rel = relative_path(repo_root, value_path)

    # Write metadata files
    (artifact_dir / "code.txt").write_text(detail.asset.source_text)

    store.mark_materialization_success(
        mat_id, artifact_path_rel, "json", artifact_checksum,
    )


def materialize_asset(
    module_path: str,
    function_name: str,
    output_dir: Path,
    input_kwargs: dict | None = None,
) -> Path:
    """Import module, call function, save result as JSON. Returns value path."""
    mod = importlib.import_module(module_path)
    func = getattr(mod, function_name)
    original = getattr(func, "__barca_original__", func)

    if input_kwargs:
        result = original(**input_kwargs)
    else:
        result = original()

    value_path = output_dir / "value.json"
    value_path.write_text(json.dumps(result))
    return value_path


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

    # Fall back to decorator metadata
    try:
        meta = json.loads(detail.asset.decorator_metadata_json)
    except (json.JSONDecodeError, TypeError):
        return []

    inputs_obj = meta.get("inputs")
    if not inputs_obj or not isinstance(inputs_obj, dict):
        return []

    result: list[AssetInput] = []
    for param_name, abs_ref in inputs_obj.items():
        if not isinstance(abs_ref, str):
            continue
        # Relativize
        if ":" in abs_ref:
            colon_pos = abs_ref.rfind(":")
            abs_path = abs_ref[:colon_pos]
            func_name = abs_ref[colon_pos + 1:]
            rel = relative_path(repo_root, Path(abs_path))
            canonical_ref = f"{rel}:{func_name}"
        else:
            canonical_ref = abs_ref

        upstream_id = store.asset_id_by_logical_name(canonical_ref)
        result.append(AssetInput(
            parameter_name=param_name,
            upstream_asset_ref=canonical_ref,
            upstream_asset_id=upstream_id,
        ))

    if result:
        store.upsert_asset_inputs(detail.asset.definition_id, result)

    return result


def _resolve_partition_values(detail: AssetDetail) -> list[dict]:
    """Extract partition values from decorator metadata."""
    try:
        meta = json.loads(detail.asset.decorator_metadata_json)
    except (json.JSONDecodeError, TypeError):
        return []

    partitions_obj = meta.get("partitions")
    if not partitions_obj or not isinstance(partitions_obj, dict):
        return []

    result: list[dict] = []
    for dim_name, spec in partitions_obj.items():
        if not isinstance(spec, dict):
            continue
        if spec.get("kind") != "inline":
            continue
        values_json = spec.get("values_json")
        if not values_json:
            continue
        try:
            values = json.loads(values_json)
        except json.JSONDecodeError:
            continue
        for val in values:
            result.append({dim_name: val})

    return result


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
    """Remove generated files and caches. Returns summary."""
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
