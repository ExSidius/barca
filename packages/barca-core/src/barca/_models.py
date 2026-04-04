"""Pydantic models for Barca data structures."""

from __future__ import annotations

from typing import Any

from pydantic import BaseModel


class InspectedAsset(BaseModel):
    kind: str
    module_path: str
    file_path: str
    function_name: str
    function_source: str
    module_source: str
    decorator_metadata: dict[str, Any]
    return_type: str | None = None
    python_version: str
    dependency_cone_hash: str | None = None
    purity_warnings: list[str] = []


class IndexedAsset(BaseModel):
    asset_id: int = 0
    logical_name: str
    continuity_key: str
    module_path: str
    file_path: str
    function_name: str
    asset_slug: str
    definition_id: int = 0
    definition_hash: str
    run_hash: str
    source_text: str
    module_source_text: str
    decorator_metadata_json: str
    return_type: str | None = None
    serializer_kind: str = "json"
    python_version: str
    codebase_hash: str
    dependency_cone_hash: str


class AssetInput(BaseModel):
    parameter_name: str
    upstream_asset_ref: str
    upstream_asset_id: int | None = None


class MaterializationRecord(BaseModel):
    materialization_id: int
    asset_id: int
    definition_id: int
    run_hash: str
    status: str  # queued, running, success, failed
    artifact_path: str | None = None
    artifact_format: str | None = None
    artifact_checksum: str | None = None
    last_error: str | None = None
    partition_key_json: str | None = None
    created_at: int


class AssetSummary(BaseModel):
    asset_id: int
    logical_name: str
    module_path: str
    file_path: str
    function_name: str
    definition_hash: str
    materialization_status: str | None = None
    materialization_run_hash: str | None = None
    materialization_created_at: int | None = None


class AssetDetail(BaseModel):
    asset: IndexedAsset
    latest_materialization: MaterializationRecord | None = None


class JobDetail(BaseModel):
    job: MaterializationRecord
    asset: AssetSummary
