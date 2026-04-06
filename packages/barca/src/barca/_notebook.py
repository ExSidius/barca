"""Notebook helpers — load_inputs, materialize, read_asset, list_versions."""

from __future__ import annotations

import inspect
import json
from pathlib import Path

from barca._engine import refresh, reindex
from barca._hashing import relative_path
from barca._store import MetadataStore

# ------------------------------------------------------------------
# Internal helpers
# ------------------------------------------------------------------


def _find_project_root(fn) -> Path:
    """Walk up from the function's source file (or CWD) to find barca.toml."""
    original = getattr(fn, "__barca_original__", fn)
    source_file = inspect.getsourcefile(original)

    start_dirs = []
    if source_file:
        start_dirs.append(Path(source_file).resolve().parent)
    start_dirs.append(Path.cwd())

    for start in start_dirs:
        d = start
        while True:
            if (d / "barca.toml").exists():
                return d
            parent = d.parent
            if parent == d:
                break
            d = parent

    raise FileNotFoundError("Could not find barca.toml — are you inside a Barca project?")


def _bootstrap(fn) -> tuple[MetadataStore, Path, int]:
    """Find project root, create store, reindex, resolve asset_id."""
    repo_root = _find_project_root(fn)
    store = MetadataStore(str(repo_root / ".barca" / "metadata.db"))
    reindex(store, repo_root)
    asset_id = _resolve_asset_id(store, repo_root, fn)
    return store, repo_root, asset_id


def _resolve_asset_id(store: MetadataStore, repo_root: Path, fn) -> int:
    """Resolve a decorated function to its asset_id in the store."""
    original = getattr(fn, "__barca_original__", fn)
    meta = getattr(fn, "__barca_metadata__", None) or {}
    explicit_name = meta.get("name")

    if explicit_name:
        continuity_key = explicit_name
    else:
        source_file = inspect.getsourcefile(original)
        if source_file is None:
            raise ValueError(f"Cannot determine source file for {original}")
        rel = relative_path(repo_root, Path(source_file).resolve())
        continuity_key = f"{rel}:{original.__name__}"

    asset_id = store.asset_id_by_logical_name(continuity_key)
    if asset_id is None:
        raise ValueError(f"Asset '{continuity_key}' not found in the index. Has reindex been run?")
    return asset_id


def _load_upstream_value(
    store: MetadataStore,
    repo_root: Path,
    upstream_id: int,
    partition: dict | None = None,
) -> object:
    """Load the latest value for an upstream asset or sensor."""
    detail = store.asset_detail(upstream_id)

    if detail.asset.kind == "sensor":
        obs = store.latest_sensor_observation(upstream_id)
        if obs is None or obs.output_json is None:
            raise ValueError(f"Sensor '{detail.asset.logical_name}' has no observations yet")
        return json.loads(obs.output_json)

    # Asset — load from materialization artifact
    if partition is not None:
        pk_json = json.dumps(partition, separators=(",", ":"))
        mat = store.latest_successful_materialization_for_partition(
            upstream_id,
            pk_json,
        )
    else:
        mat = store.latest_successful_materialization(upstream_id)

    if mat is None:
        raise ValueError(f"Asset '{detail.asset.logical_name}' has no successful materialization")
    if not mat.artifact_path:
        raise ValueError(f"Asset '{detail.asset.logical_name}' has no artifact path")
    return json.loads((repo_root / mat.artifact_path).read_text())


def _resolve_inputs(store, repo_root, detail):
    """Get asset inputs from DB, falling back to decorator metadata."""
    inputs = store.get_asset_inputs(detail.asset.definition_id)
    if inputs:
        return inputs

    from barca._models import AssetInput

    try:
        meta = json.loads(detail.asset.decorator_metadata_json)
    except (json.JSONDecodeError, TypeError):
        return []

    inputs_obj = meta.get("inputs")
    if not inputs_obj or not isinstance(inputs_obj, dict):
        return []

    result = []
    for param_name, abs_ref in inputs_obj.items():
        if not isinstance(abs_ref, str):
            continue
        if ":" in abs_ref:
            colon_pos = abs_ref.rfind(":")
            abs_path = abs_ref[:colon_pos]
            func_name = abs_ref[colon_pos + 1 :]
            rel = relative_path(repo_root, Path(abs_path))
            canonical_ref = f"{rel}:{func_name}"
        else:
            canonical_ref = abs_ref

        upstream_id = store.asset_id_by_logical_name(canonical_ref)
        result.append(
            AssetInput(
                parameter_name=param_name,
                upstream_asset_ref=canonical_ref,
                upstream_asset_id=upstream_id,
            )
        )

    return result


# ------------------------------------------------------------------
# Public API
# ------------------------------------------------------------------


def load_inputs(asset_fn, *, partition: dict | None = None) -> dict[str, object]:
    """Load upstream inputs for an asset/effect as a kwargs dict.

    Usage::

        from barca import load_inputs
        kwargs = load_inputs(my_asset)
        result = my_asset(**kwargs)
    """
    store, repo_root, asset_id = _bootstrap(asset_fn)
    detail = store.asset_detail(asset_id)
    inputs = _resolve_inputs(store, repo_root, detail)

    if not inputs:
        return {}

    kwargs: dict[str, object] = {}
    for inp in inputs:
        uid = inp.upstream_asset_id
        if uid is None or uid <= 0:
            continue
        kwargs[inp.parameter_name] = _load_upstream_value(
            store,
            repo_root,
            uid,
            partition,
        )
    return kwargs


def materialize(asset_fn) -> object:
    """Materialize an asset (with caching) and return its deserialized value.

    Upstream dependencies are refreshed automatically if stale.
    Cached results are reused when the run_hash matches.
    """
    store, repo_root, asset_id = _bootstrap(asset_fn)
    detail = store.asset_detail(asset_id)

    if detail.asset.kind == "sensor":
        raise ValueError("Cannot materialize a sensor — use read_asset() after triggering")
    if detail.asset.kind == "effect":
        raise ValueError("Cannot materialize an effect — effects are side-effects, not cached values")

    refresh(store, repo_root, asset_id)

    mat = store.latest_successful_materialization(asset_id)
    if mat is None or not mat.artifact_path:
        raise ValueError("Materialization produced no artifact")
    return json.loads((repo_root / mat.artifact_path).read_text())


def read_asset(asset_or_sensor_fn) -> object:
    """Read the latest materialized value for an asset or sensor observation."""
    store, repo_root, asset_id = _bootstrap(asset_or_sensor_fn)
    detail = store.asset_detail(asset_id)

    if detail.asset.kind == "effect":
        raise ValueError("Cannot read an effect — effects have no stored output")

    return _load_upstream_value(store, repo_root, asset_id)


def list_versions(asset_or_sensor_fn) -> list[dict]:
    """List historical materializations or sensor observations."""
    store, _repo_root, asset_id = _bootstrap(asset_or_sensor_fn)
    detail = store.asset_detail(asset_id)

    if detail.asset.kind == "sensor":
        observations = store.list_sensor_observations(asset_id)
        return [obs.model_dump() for obs in observations]

    materializations = store.list_materializations(asset_id)
    return [mat.model_dump() for mat in materializations]
