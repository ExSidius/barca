export interface AssetSummary {
  asset_id: number;
  logical_name: string;
  kind: string;
  module_path: string;
  file_path: string;
  function_name: string;
  definition_hash: string;
  schedule: string;
  materialization_status: string | null;
  materialization_run_hash: string | null;
  materialization_created_at: number | null;
}

export interface MaterializationRecord {
  materialization_id: number;
  asset_id: number;
  definition_id: number;
  run_hash: string;
  status: string;
  artifact_path: string | null;
  artifact_format: string | null;
  artifact_checksum: string | null;
  last_error: string | null;
  partition_key_json: string | null;
  created_at: number;
}

export interface IndexedAsset {
  asset_id: number;
  logical_name: string;
  continuity_key: string;
  module_path: string;
  file_path: string;
  function_name: string;
  asset_slug: string;
  kind: string;
  definition_id: number;
  definition_hash: string;
  run_hash: string;
  source_text: string;
  module_source_text: string;
  decorator_metadata_json: string;
  return_type: string | null;
  serializer_kind: string;
  python_version: string;
  codebase_hash: string;
  dependency_cone_hash: string;
}

export interface SensorObservation {
  observation_id: number;
  asset_id: number;
  definition_id: number;
  update_detected: boolean;
  output_json: string | null;
  created_at: number;
}

export interface AssetDetail {
  asset: IndexedAsset;
  latest_materialization: MaterializationRecord | null;
  latest_observation: SensorObservation | null;
}

export interface JobDetail {
  job: MaterializationRecord;
  asset: AssetSummary;
}

export interface ReconcileResult {
  executed_assets: number;
  executed_sensors: number;
  executed_effects: number;
  stale_waiting: number;
  fresh: number;
  failed: number;
}
