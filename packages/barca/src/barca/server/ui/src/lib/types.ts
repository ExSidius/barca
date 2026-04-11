export interface AssetSummary {
  asset_id: number;
  logical_name: string;
  kind: string; // "asset" | "sensor" | "effect" | "sink"
  module_path: string;
  file_path: string;
  function_name: string;
  definition_hash: string;
  freshness: string; // "always" | "manual" | "schedule:<cron>"
  purity: string; // "pure" | "unsafe"
  parent_asset_id: number | null;
  partitions_state: string | null; // "resolved" | "pending" | null
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
  stale_inputs_used: boolean;
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
  purity: string;
  parent_asset_id: number | null;
  sink_path: string | null;
  sink_serializer: string | null;
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

export interface SinkExecution {
  execution_id: number;
  asset_id: number;
  definition_id: number;
  run_hash: string;
  status: string;
  destination_path: string;
  last_error: string | null;
  created_at: number;
}

export interface ReindexDiff {
  added: string[];
  removed: string[];
  renamed: [string, string][];
}

export interface RunPassResult {
  executed_assets: number;
  executed_sensors: number;
  executed_effects: number;
  executed_sinks: number;
  fresh: number;
  manual_skipped: number;
  stale_blocked: number;
  failed: number;
  sink_failed: number;
  added: string[];
  removed: string[];
  renamed: [string, string][];
}

export interface PruneResult {
  removed_assets: number;
  removed_materializations: number;
  removed_observations: number;
  removed_effect_executions: number;
  removed_sink_executions: number;
  removed_artifact_files: number;
}

export interface GraphNode {
  asset_id: number;
  logical_name: string;
  kind: string; // "asset" | "sensor" | "effect" | "sink"
  module_path: string;
  file_path: string;
  function_name: string;
  freshness: string;
  purity: string;
  materialization_status: string | null;
  materialization_created_at: number | null;
  parent_asset_id: number | null;
}

export interface GraphEdge {
  source_asset_id: number;
  target_asset_id: number;
  parameter_name: string;
  collect_mode: boolean;
  is_partition_source: boolean;
}

export interface GraphResponse {
  nodes: GraphNode[];
  edges: GraphEdge[];
}
