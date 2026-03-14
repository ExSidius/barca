use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Deserialize)]
pub struct InspectResponse {
    pub assets: Vec<InspectedAsset>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InspectedAsset {
    pub kind: String,
    pub module_path: String,
    pub file_path: String,
    pub function_name: String,
    pub function_source: String,
    pub module_source: String,
    pub decorator_metadata: serde_json::Value,
    pub return_type: Option<String>,
    pub python_version: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WorkerResponse {
    pub ok: bool,
    pub artifact_format: Option<String>,
    pub value_path: Option<String>,
    pub result_type: Option<String>,
    pub module_path: Option<String>,
    pub function_name: Option<String>,
    pub signature: Option<String>,
    pub error: Option<String>,
    pub error_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct IndexedAsset {
    pub asset_id: i64,
    pub logical_name: String,
    pub continuity_key: String,
    pub module_path: String,
    pub file_path: String,
    pub function_name: String,
    pub asset_slug: String,
    pub definition_id: i64,
    pub definition_hash: String,
    pub run_hash: String,
    pub source_text: String,
    pub module_source_text: String,
    pub decorator_metadata_json: String,
    pub return_type: Option<String>,
    pub serializer_kind: String,
    pub python_version: String,
    pub uv_lock_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AssetSummary {
    pub asset_id: i64,
    pub logical_name: String,
    pub module_path: String,
    pub file_path: String,
    pub function_name: String,
    pub definition_hash: String,
    pub materialization_status: Option<String>,
    pub materialization_run_hash: Option<String>,
    pub materialization_created_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct MaterializationRecord {
    pub materialization_id: i64,
    pub asset_id: i64,
    pub definition_id: i64,
    pub run_hash: String,
    pub status: String,
    pub artifact_path: Option<String>,
    pub artifact_format: Option<String>,
    pub artifact_checksum: Option<String>,
    pub last_error: Option<String>,
    pub partition_key_json: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct AssetDetail {
    pub asset: IndexedAsset,
    pub latest_materialization: Option<MaterializationRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JobDetail {
    pub job: MaterializationRecord,
    pub asset: AssetSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct JobLogRecord {
    pub id: i64,
    pub materialization_id: i64,
    pub asset_id: i64,
    pub level: String,
    pub message: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetInput {
    pub parameter_name: String,
    pub upstream_asset_ref: String,
    pub upstream_asset_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterializationInput {
    pub parameter_name: String,
    pub upstream_materialization_id: i64,
    pub upstream_asset_id: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArtifactMetadata<'a> {
    pub asset_name: &'a str,
    pub module_path: &'a str,
    pub file_path: &'a str,
    pub function_name: &'a str,
    pub definition_hash: &'a str,
    pub run_hash: &'a str,
    pub serializer_kind: &'a str,
    pub python_version: &'a str,
    pub return_type: Option<&'a str>,
    pub inputs: Vec<serde_json::Value>,
    pub barca_version: &'static str,
}
