"""Pydantic models for Barca data structures."""

from __future__ import annotations

from typing import Any

from pydantic import BaseModel, Field


class InspectedAsset(BaseModel):
    """Raw metadata extracted from a decorated function before indexing."""

    kind: str = Field(description="Node kind: 'asset', 'sensor', or 'effect'")
    module_path: str = Field(description="Dotted Python module path (e.g. 'my_project.assets')")
    file_path: str = Field(description="Absolute filesystem path to the source file")
    function_name: str = Field(description="Name of the decorated function")
    function_source: str = Field(description="Source code of the decorated function")
    module_source: str = Field(description="Full source code of the containing module")
    decorator_metadata: dict[str, Any] = Field(description="Decorator keyword arguments (inputs, partitions, schedule, etc.)")
    return_type: str | None = Field(default=None, description="String representation of the function's return type annotation")
    python_version: str = Field(description="Python version string used during inspection")
    dependency_cone_hash: str | None = Field(default=None, description="SHA-256 hash of the function's transitive dependency cone, or None if tracing failed")
    purity_warnings: list[str] = Field(default=[], description="Warnings from purity analysis (impure calls, nondeterministic imports, etc.)")


class IndexedAsset(BaseModel):
    """An asset/sensor/effect that has been indexed into the metadata database."""

    asset_id: int = Field(default=0, description="Primary key in the assets table")
    logical_name: str = Field(description="Display name (defaults to function_name, overridable via name= parameter)")
    continuity_key: str = Field(description="Stable identity key: '{relative_file}:{function_name}' or custom via name= parameter")
    module_path: str = Field(description="Dotted Python module path")
    file_path: str = Field(description="Relative filesystem path to the source file")
    function_name: str = Field(description="Name of the decorated function")
    asset_slug: str = Field(description="URL-safe slug derived from the continuity key")
    kind: str = Field(default="asset", description="Node kind: 'asset', 'sensor', or 'effect'")
    definition_id: int = Field(default=0, description="Primary key in the asset_definitions table for the current definition")
    definition_hash: str = Field(description="SHA-256 hash of source + metadata + dependency cone hash + protocol version")
    run_hash: str = Field(description="SHA-256 hash of definition_hash + upstream materialization IDs + partition key")
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


class MaterializationRecord(BaseModel):
    """A record of a single materialization attempt (queued, running, success, or failed)."""

    materialization_id: int = Field(description="Primary key in the materializations table")
    asset_id: int = Field(description="Foreign key to the assets table")
    definition_id: int = Field(description="Foreign key to the asset_definitions table")
    run_hash: str = Field(description="Content-address hash: identical run_hash means identical inputs and definition")
    status: str = Field(description="Lifecycle status: 'queued', 'running', 'success', or 'failed'")
    artifact_path: str | None = Field(default=None, description="Relative path to the artifact file (e.g. '.barcafiles/slug/hash/value.json')")
    artifact_format: str | None = Field(default=None, description="Serialization format of the artifact (e.g. 'json')")
    artifact_checksum: str | None = Field(default=None, description="SHA-256 checksum of the artifact file contents")
    last_error: str | None = Field(default=None, description="Error message if status is 'failed'")
    partition_key_json: str | None = Field(default=None, description="JSON-serialized partition key (e.g. '{\"ticker\": \"AAPL\"}'), or None for non-partitioned assets")
    created_at: int = Field(description="Unix timestamp when the materialization was created")


class AssetSummary(BaseModel):
    """Lightweight view of an asset used in list endpoints and table displays."""

    asset_id: int = Field(description="Primary key in the assets table")
    logical_name: str = Field(description="Display name of the asset")
    kind: str = Field(default="asset", description="Node kind: 'asset', 'sensor', or 'effect'")
    module_path: str = Field(description="Dotted Python module path")
    file_path: str = Field(description="Relative filesystem path to the source file")
    function_name: str = Field(description="Name of the decorated function")
    definition_hash: str = Field(description="SHA-256 hash of the current definition")
    schedule: str = Field(default="manual", description="Schedule type: 'manual', 'always', or a cron expression")
    materialization_status: str | None = Field(default=None, description="Status of the latest materialization, or None if never run")
    materialization_run_hash: str | None = Field(default=None, description="Run hash of the latest materialization")
    materialization_created_at: int | None = Field(default=None, description="Unix timestamp of the latest materialization")


class AssetDetail(BaseModel):
    """Full detail for a single asset, including definition and latest execution state."""

    asset: IndexedAsset = Field(description="The indexed asset with its current definition")
    latest_materialization: MaterializationRecord | None = Field(default=None, description="Most recent materialization record, or None if never materialized")
    latest_observation: SensorObservation | None = Field(default=None, description="Most recent sensor observation (only populated for sensor nodes)")


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
    output_json: str | None = Field(default=None, description="JSON-serialized sensor output value, or None if no output")
    created_at: int = Field(default=0, description="Unix timestamp when the observation was recorded")


class EffectExecution(BaseModel):
    """A recorded execution of an effect node."""

    execution_id: int = Field(default=0, description="Primary key in the effect_executions table")
    asset_id: int = Field(description="Foreign key to the assets table (the effect node)")
    definition_id: int = Field(description="Foreign key to the asset_definitions table")
    status: str = Field(description="Execution status: 'success' or 'failed'")
    last_error: str | None = Field(default=None, description="Error message if status is 'failed'")
    created_at: int = Field(default=0, description="Unix timestamp when the effect was executed")


class ReconcileResult(BaseModel):
    """Summary of a single reconciliation pass."""

    executed_assets: int = Field(default=0, description="Number of assets materialized this pass")
    executed_sensors: int = Field(default=0, description="Number of sensors executed this pass")
    executed_effects: int = Field(default=0, description="Number of effects executed this pass")
    stale_waiting: int = Field(default=0, description="Stale nodes skipped because their schedule is not yet eligible")
    fresh: int = Field(default=0, description="Nodes that were already up-to-date or cached")
    failed: int = Field(default=0, description="Nodes that failed during execution (includes downstream cascades)")
