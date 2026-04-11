"""Pydantic models for Barca data structures."""

from __future__ import annotations

from typing import Any

from pydantic import BaseModel, Field


class InspectedAsset(BaseModel):
    """Raw metadata extracted from a decorated function before indexing."""

    kind: str = Field(description="Node kind: 'asset', 'sensor', 'effect', or 'sink'")
    module_path: str = Field(description="Dotted Python module path (e.g. 'my_project.assets')")
    file_path: str = Field(description="Absolute filesystem path to the source file")
    function_name: str = Field(description="Name of the decorated function (or synthesised sink name)")
    function_source: str = Field(description="Source code of the decorated function")
    module_source: str = Field(description="Full source code of the containing module")
    decorator_metadata: dict[str, Any] = Field(description="Decorator keyword arguments (inputs, partitions, freshness, sinks, etc.)")
    return_type: str | None = Field(default=None, description="String representation of the return type annotation")
    python_version: str = Field(description="Python version string used during inspection")
    dependency_cone_hash: str | None = Field(
        default=None,
        description="SHA-256 hash of the function's transitive dependency cone, or None if tracing failed",
    )
    purity_warnings: list[str] = Field(default=[], description="Warnings from purity analysis")
    # Sinks carry extra metadata so the inspector can emit one InspectedAsset per sink.
    parent_function_name: str | None = Field(
        default=None,
        description="For sinks: the function_name of the parent asset this sink is attached to",
    )
    sink_path: str | None = Field(default=None, description="For sinks: fsspec-compatible destination path")
    sink_serializer: str | None = Field(default=None, description="For sinks: output serializer kind")


class IndexedAsset(BaseModel):
    """An asset/sensor/effect/sink that has been indexed into the metadata database."""

    asset_id: int = Field(default=0, description="Primary key in the assets table")
    logical_name: str = Field(description="Display name (defaults to function_name, overridable via name= parameter)")
    continuity_key: str = Field(description="Stable identity key: '{relative_file}:{function_name}' or custom via name= parameter")
    module_path: str = Field(description="Dotted Python module path")
    file_path: str = Field(description="Relative filesystem path to the source file")
    function_name: str = Field(description="Name of the decorated function")
    asset_slug: str = Field(description="URL-safe slug derived from the continuity key")
    kind: str = Field(default="asset", description="Node kind: 'asset', 'sensor', 'effect', or 'sink'")
    purity: str = Field(default="pure", description="'pure' or 'unsafe'")
    parent_asset_id: int | None = Field(
        default=None,
        description="For sinks: the asset_id of the parent regular asset",
    )
    sink_path: str | None = Field(default=None, description="For sinks: fsspec destination path")
    sink_serializer: str | None = Field(default=None, description="For sinks: output serializer")
    definition_id: int = Field(default=0, description="Primary key in the asset_definitions table for the current definition")
    definition_hash: str = Field(description="SHA-256 hash of source + metadata + dependency cone + protocol version")
    run_hash: str = Field(description="SHA-256 hash of definition_hash + sorted upstream materialization IDs + partition key")
    source_text: str = Field(description="Source code of the decorated function")
    module_source_text: str = Field(description="Full source code of the containing module")
    decorator_metadata_json: str = Field(description="JSON-serialized decorator keyword arguments")
    return_type: str | None = Field(default=None, description="String representation of the return type annotation")
    serializer_kind: str = Field(default="json", description="Artifact serialization format")
    python_version: str = Field(description="Python version string used during indexing")
    codebase_hash: str = Field(description="Merkle-tree hash of all .py files + uv.lock in the project")
    dependency_cone_hash: str = Field(description="SHA-256 hash of the function's transitive dependency cone")


class AssetInput(BaseModel):
    """A declared input dependency from one asset to an upstream asset or sensor."""

    parameter_name: str = Field(description="Function parameter name that receives the upstream value")
    upstream_asset_ref: str = Field(description="Reference string identifying the upstream node (continuity_key)")
    upstream_asset_id: int | None = Field(default=None, description="Resolved asset_id of the upstream node, or None if not yet resolved")
    collect_mode: bool = Field(
        default=False,
        description="True when the input was declared via collect() — downstream receives dict[tuple, T]",
    )
    is_partition_source: bool = Field(
        default=False,
        description="True for implicit edges from a dynamic-partition upstream to its partitioned downstream",
    )


class MaterializationRecord(BaseModel):
    """A record of a single materialization attempt (queued, running, success, or failed)."""

    materialization_id: int = Field(description="Primary key in the materializations table")
    asset_id: int = Field(description="Foreign key to the assets table")
    definition_id: int = Field(description="Foreign key to the asset_definitions table")
    run_hash: str = Field(description="Content-address hash: identical run_hash means identical inputs and definition")
    status: str = Field(description="Lifecycle status: 'queued', 'running', 'success', or 'failed'")
    artifact_path: str | None = Field(default=None, description="Relative path to the artifact file")
    artifact_format: str | None = Field(default=None, description="Serialization format of the artifact")
    artifact_checksum: str | None = Field(default=None, description="SHA-256 checksum of the artifact file")
    last_error: str | None = Field(default=None, description="Error message if status is 'failed'")
    partition_key_json: str | None = Field(default=None, description="JSON-serialized partition key")
    stale_inputs_used: bool = Field(
        default=False,
        description="True when materialized under stale_policy=warn|pass with stale upstream inputs",
    )
    created_at: int = Field(description="Unix timestamp when the materialization was created")


class AssetSummary(BaseModel):
    """Lightweight view of an asset used in list endpoints and table displays."""

    asset_id: int = Field(description="Primary key in the assets table")
    logical_name: str = Field(description="Display name of the asset")
    kind: str = Field(default="asset", description="Node kind: 'asset', 'sensor', 'effect', or 'sink'")
    module_path: str = Field(description="Dotted Python module path")
    file_path: str = Field(description="Relative filesystem path to the source file")
    function_name: str = Field(description="Name of the decorated function")
    definition_hash: str = Field(description="SHA-256 hash of the current definition")
    freshness: str = Field(default="always", description="Serialized freshness: 'always', 'manual', or 'schedule:<cron>'")
    purity: str = Field(default="pure", description="'pure' or 'unsafe'")
    parent_asset_id: int | None = Field(default=None, description="For sinks: the parent regular asset's id")
    partitions_state: str | None = Field(
        default=None,
        description="For dynamically-partitioned assets: 'resolved' or 'pending'",
    )
    materialization_status: str | None = Field(default=None, description="Status of the latest materialization")
    materialization_run_hash: str | None = Field(default=None, description="Run hash of the latest materialization")
    materialization_created_at: int | None = Field(default=None, description="Unix timestamp of the latest materialization")


class AssetDetail(BaseModel):
    """Full detail for a single asset, including definition and latest execution state."""

    asset: IndexedAsset = Field(description="The indexed asset with its current definition")
    latest_materialization: MaterializationRecord | None = Field(default=None, description="Most recent materialization record")
    latest_observation: SensorObservation | None = Field(default=None, description="Most recent sensor observation (sensors only)")


class JobDetail(BaseModel):
    """A materialization job paired with its asset summary."""

    job: MaterializationRecord = Field(description="The materialization record")
    asset: AssetSummary = Field(description="Summary of the asset this job belongs to")


class SensorObservation(BaseModel):
    """A recorded observation from a sensor execution."""

    observation_id: int = Field(default=0, description="Primary key in the sensor_observations table")
    asset_id: int = Field(description="Foreign key to the assets table (the sensor node)")
    definition_id: int = Field(description="Foreign key to the asset_definitions table")
    update_detected: bool = Field(description="Whether the sensor reported a change in external state")
    output_json: str | None = Field(default=None, description="JSON-serialized full (update_detected, output) tuple")
    created_at: int = Field(default=0, description="Unix timestamp when the observation was recorded")


class EffectExecution(BaseModel):
    """A recorded execution of an effect node."""

    execution_id: int = Field(default=0, description="Primary key in the effect_executions table")
    asset_id: int = Field(description="Foreign key to the assets table (the effect node)")
    definition_id: int = Field(description="Foreign key to the asset_definitions table")
    status: str = Field(description="Execution status: 'success' or 'failed'")
    last_error: str | None = Field(default=None, description="Error message if status is 'failed'")
    created_at: int = Field(default=0, description="Unix timestamp when the effect was executed")


class SinkExecution(BaseModel):
    """A recorded execution of a sink (file write)."""

    execution_id: int = Field(default=0, description="Primary key in the sink_executions table")
    asset_id: int = Field(description="Foreign key to the assets table (the sink node)")
    definition_id: int = Field(description="Foreign key to the asset_definitions table")
    run_hash: str = Field(description="Content-address hash matching the parent asset materialization")
    status: str = Field(description="'success' or 'failed'")
    destination_path: str = Field(description="Resolved fsspec path the sink wrote to")
    last_error: str | None = Field(default=None, description="Error message if status is 'failed'")
    created_at: int = Field(default=0, description="Unix timestamp when the sink was executed")


class ReindexDiff(BaseModel):
    """Three-way diff produced by reindex: added, removed, renamed."""

    added: list[str] = Field(default_factory=list, description="Continuity keys of newly-discovered assets")
    removed: list[str] = Field(default_factory=list, description="Continuity keys of assets no longer present in source")
    renamed: list[tuple[str, str]] = Field(
        default_factory=list,
        description="List of (old_continuity_key, new_continuity_key) pairs",
    )


class RunPassResult(BaseModel):
    """Summary of a single run_pass execution."""

    executed_assets: int = Field(default=0, description="Number of regular assets materialized this pass")
    executed_sensors: int = Field(default=0, description="Number of sensors executed this pass")
    executed_effects: int = Field(default=0, description="Number of effects executed this pass")
    executed_sinks: int = Field(default=0, description="Number of sinks written this pass")
    fresh: int = Field(default=0, description="Assets that were already fresh or cached")
    manual_skipped: int = Field(default=0, description="Manual-freshness assets skipped (not eligible)")
    stale_blocked: int = Field(default=0, description="Always assets blocked by a stale Manual upstream")
    failed: int = Field(default=0, description="Assets/sensors/effects that failed during this pass")
    sink_failed: int = Field(default=0, description="Sinks that failed during this pass (non-blocking)")
    added: list[str] = Field(default_factory=list, description="Reindex diff: newly-added assets")
    removed: list[str] = Field(default_factory=list, description="Reindex diff: removed assets")
    renamed: list[tuple[str, str]] = Field(default_factory=list, description="Reindex diff: renamed assets")


class PruneResult(BaseModel):
    """Summary of a barca prune operation."""

    removed_assets: int = Field(default=0, description="Number of asset rows deleted from the store")
    removed_materializations: int = Field(default=0, description="Number of materialization records deleted")
    removed_observations: int = Field(default=0, description="Number of sensor observation records deleted")
    removed_effect_executions: int = Field(default=0, description="Number of effect execution records deleted")
    removed_sink_executions: int = Field(default=0, description="Number of sink execution records deleted")
    removed_artifact_files: int = Field(default=0, description="Number of artifact files deleted from disk")


# Backwards-compat alias so anything still importing ReconcileResult keeps working
# until we delete the old reconciler in Layer 7. It's the same shape as RunPassResult
# for the fields that matter; we'll remove this alias once _reconciler.py is gone.
class ReconcileResult(BaseModel):
    """DEPRECATED: use RunPassResult. Kept temporarily for the legacy reconciler."""

    executed_assets: int = Field(default=0)
    executed_sensors: int = Field(default=0)
    executed_effects: int = Field(default=0)
    stale_waiting: int = Field(default=0)
    fresh: int = Field(default=0)
    failed: int = Field(default=0)


# ---------------------------------------------------------------------------
# Exceptions
# ---------------------------------------------------------------------------


class StaleUpstreamError(Exception):
    """Raised by refresh() when upstream is stale and stale_policy='error'."""

    def __init__(self, message: str, stale_upstreams: list[str] | None = None):
        super().__init__(message)
        self.stale_upstreams = stale_upstreams or []
