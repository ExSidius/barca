use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use barca_core::models::{AssetDetail, AssetSummary, InspectedAsset, JobDetail, WorkerResponse};
use barca_server::config::{BarcaConfig, PythonConfig};
use barca_server::python_bridge::PythonBridge;
use barca_server::store::MetadataStore;
use barca_server::{AppState, JobLogEntry};
use tokio::sync::broadcast;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Mock PythonBridge
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct MockPythonBridge {
    assets: Vec<InspectedAsset>,
}

impl MockPythonBridge {
    fn empty() -> Self {
        Self { assets: vec![] }
    }

    fn with_one_asset() -> Self {
        Self {
            assets: vec![InspectedAsset {
                kind: "asset".into(),
                module_path: "example.assets".into(),
                file_path: "/tmp/barca-test/example/assets.py".into(),
                function_name: "my_asset".into(),
                function_source: "def my_asset(): return 42".into(),
                module_source: "from barca import asset\n\n@asset()\ndef my_asset(): return 42".into(),
                decorator_metadata: serde_json::json!({}),
                return_type: Some("int".into()),
                python_version: "3.12.0".into(),
            }],
        }
    }

    /// Two assets: `fruit` (no inputs) and `uppercased` (inputs: {"fruit": fruit}).
    /// The input ref uses an absolute path that will be relativized by build_indexed_asset.
    fn with_dependency_pair() -> Self {
        let file_path = "/tmp/barca-test/example/assets.py";
        let module_source = "from barca import asset\n\n@asset()\ndef fruit(): return 'banana'\n\n@asset(inputs={'fruit': fruit})\ndef uppercased(fruit): return fruit.upper()";
        Self {
            assets: vec![
                InspectedAsset {
                    kind: "asset".into(),
                    module_path: "example.assets".into(),
                    file_path: file_path.into(),
                    function_name: "fruit".into(),
                    function_source: "def fruit(): return 'banana'\n".into(),
                    module_source: module_source.into(),
                    decorator_metadata: serde_json::json!({"kind": "asset"}),
                    return_type: Some("str".into()),
                    python_version: "3.12.0".into(),
                },
                InspectedAsset {
                    kind: "asset".into(),
                    module_path: "example.assets".into(),
                    file_path: file_path.into(),
                    function_name: "uppercased".into(),
                    function_source: "def uppercased(fruit): return fruit.upper()\n".into(),
                    module_source: module_source.into(),
                    decorator_metadata: serde_json::json!({
                        "kind": "asset",
                        "inputs": {
                            "fruit": format!("{file_path}:fruit")
                        }
                    }),
                    return_type: Some("str".into()),
                    python_version: "3.12.0".into(),
                },
            ],
        }
    }

    /// One partitioned asset: `fetch_prices` with 3 static partition values.
    fn with_partitioned_asset() -> Self {
        let file_path = "/tmp/barca-test/example/assets.py";
        Self {
            assets: vec![InspectedAsset {
                kind: "asset".into(),
                module_path: "example.assets".into(),
                file_path: file_path.into(),
                function_name: "fetch_prices".into(),
                function_source: "def fetch_prices(ticker): return {'ticker': ticker}\n".into(),
                module_source: "from barca import asset, partitions\n\n@asset(partitions={'ticker': partitions(['AAPL', 'MSFT', 'GOOG'])})\ndef fetch_prices(ticker): return {'ticker': ticker}\n"
                    .into(),
                decorator_metadata: serde_json::json!({
                    "kind": "asset",
                    "partitions": {
                        "ticker": {
                            "kind": "inline",
                            "values_json": "[\"AAPL\",\"MSFT\",\"GOOG\"]"
                        }
                    }
                }),
                return_type: Some("dict".into()),
                python_version: "3.12.0".into(),
            }],
        }
    }
}

#[async_trait]
impl PythonBridge for MockPythonBridge {
    async fn inspect_modules(&self, _modules: &[String]) -> anyhow::Result<Vec<InspectedAsset>> {
        Ok(self.assets.clone())
    }

    async fn materialize_asset(
        &self,
        _module_path: &str,
        function_name: &str,
        output_dir: &std::path::Path,
        _job_id: i64,
        _log_tx: broadcast::Sender<JobLogEntry>,
        _asset_id: i64,
        input_kwargs_json: Option<&str>,
    ) -> anyhow::Result<WorkerResponse> {
        // Simulate Python worker: compute a value and write value.json + result.json
        let kwargs: serde_json::Value = input_kwargs_json.map(|s| serde_json::from_str(s).unwrap()).unwrap_or(serde_json::json!({}));

        let value = match function_name {
            "fruit" => serde_json::json!("banana"),
            "uppercased" => {
                let fruit_val = kwargs.get("fruit").and_then(|v| v.as_str()).unwrap_or("MISSING");
                serde_json::json!(fruit_val.to_uppercase())
            }
            "fetch_prices" => {
                let ticker = kwargs.get("ticker").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
                serde_json::json!({"ticker": ticker, "price": ticker.len() * 100})
            }
            _ => serde_json::json!(42),
        };

        std::fs::create_dir_all(output_dir).unwrap();
        let value_path = output_dir.join("value.json");
        std::fs::write(&value_path, serde_json::to_string(&value).unwrap()).unwrap();

        let result = serde_json::json!({
            "ok": true,
            "artifact_format": "json",
            "value_path": value_path.to_string_lossy(),
            "result_type": "str",
            "module_path": "example.assets",
            "function_name": function_name,
            "signature": "()",
        });
        std::fs::write(output_dir.join("result.json"), serde_json::to_string(&result).unwrap()).unwrap();

        Ok(WorkerResponse {
            ok: true,
            artifact_format: Some("json".into()),
            value_path: Some(value_path.to_string_lossy().into()),
            result_type: Some("str".into()),
            module_path: Some("example.assets".into()),
            function_name: Some(function_name.into()),
            signature: None,
            error: None,
            error_type: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

async fn test_app_state(mock: MockPythonBridge) -> (AppState, tempfile::TempDir) {
    let tmp_dir = tempfile::tempdir().unwrap();
    let repo_root = tmp_dir.path().to_path_buf();
    let config = BarcaConfig {
        python: PythonConfig {
            modules: vec!["example.assets".into()],
        },
    };
    let db_dir = repo_root.join(".barca");
    std::fs::create_dir_all(&db_dir).unwrap();
    let db_path = db_dir.join("metadata.db");
    let store = MetadataStore::open(&db_path).await.unwrap();
    let state = AppState::new(repo_root, config, store, Arc::new(mock));
    (state, tmp_dir)
}

fn test_router(state: AppState) -> axum::Router {
    barca_server::server::router().with_state(state)
}

async fn json_get(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).header("accept", "application/json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

async fn json_post(app: &axum::Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(Request::builder().method("POST").uri(uri).header("accept", "application/json").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_list_assets_empty() {
    let (state, _tmp) = test_app_state(MockPythonBridge::empty()).await;
    let app = test_router(state);
    let (status, body) = json_get(&app, "/api/assets").await;
    assert_eq!(status, StatusCode::OK);
    let assets: Vec<AssetSummary> = serde_json::from_str(&body).unwrap();
    assert!(assets.is_empty());
}

#[tokio::test]
async fn test_list_assets_after_reindex() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_one_asset()).await;
    barca_server::reindex(&state).await.unwrap();
    let app = test_router(state);
    let (status, body) = json_get(&app, "/api/assets").await;
    assert_eq!(status, StatusCode::OK);
    let assets: Vec<AssetSummary> = serde_json::from_str(&body).unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].function_name, "my_asset");
}

#[tokio::test]
async fn test_get_asset_not_found() {
    let (state, _tmp) = test_app_state(MockPythonBridge::empty()).await;
    let app = test_router(state);
    let (status, _body) = json_get(&app, "/api/assets/999").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_get_asset_exists() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_one_asset()).await;
    barca_server::reindex(&state).await.unwrap();
    let app = test_router(state);

    let (_, list_body) = json_get(&app, "/api/assets").await;
    let assets: Vec<AssetSummary> = serde_json::from_str(&list_body).unwrap();
    let id = assets[0].asset_id;

    let (status, body) = json_get(&app, &format!("/api/assets/{}", id)).await;
    assert_eq!(status, StatusCode::OK);
    let detail: AssetDetail = serde_json::from_str(&body).unwrap();
    assert_eq!(detail.asset.function_name, "my_asset");
}

#[tokio::test]
async fn test_reindex_api() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_one_asset()).await;
    let app = test_router(state);
    let (status, body) = json_post(&app, "/api/reindex").await;
    assert_eq!(status, StatusCode::OK);
    let assets: Vec<AssetSummary> = serde_json::from_str(&body).unwrap();
    assert_eq!(assets.len(), 1);
}

#[tokio::test]
async fn test_list_jobs_empty() {
    let (state, _tmp) = test_app_state(MockPythonBridge::empty()).await;
    let app = test_router(state);
    let (status, body) = json_get(&app, "/api/jobs").await;
    assert_eq!(status, StatusCode::OK);
    let jobs: Vec<JobDetail> = serde_json::from_str(&body).unwrap();
    assert!(jobs.is_empty());
}

#[tokio::test]
async fn test_get_job_not_found() {
    let (state, _tmp) = test_app_state(MockPythonBridge::empty()).await;
    let app = test_router(state);
    let (status, _body) = json_get(&app, "/api/jobs/999").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_openapi_spec_served() {
    let (state, _tmp) = test_app_state(MockPythonBridge::empty()).await;
    let app = test_router(state);
    let (status, body) = json_get(&app, "/api/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    let spec: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(spec.get("openapi").is_some());
    assert!(spec.get("paths").is_some());
}

#[tokio::test]
async fn test_swagger_ui_served() {
    let (state, _tmp) = test_app_state(MockPythonBridge::empty()).await;
    let app = test_router(state);
    let response = app.clone().oneshot(Request::builder().uri("/api/docs/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_materialize_asset() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_one_asset()).await;
    barca_server::reindex(&state).await.unwrap();
    let app = test_router(state);

    let (_, list_body) = json_get(&app, "/api/assets").await;
    let assets: Vec<AssetSummary> = serde_json::from_str(&list_body).unwrap();
    let id = assets[0].asset_id;

    let (status, body) = json_post(&app, &format!("/api/assets/{}/materialize", id)).await;
    assert_eq!(status, StatusCode::OK);
    let detail: AssetDetail = serde_json::from_str(&body).unwrap();
    assert_eq!(detail.asset.asset_id, id);
    assert!(detail.latest_materialization.is_some());
}

// ---------------------------------------------------------------------------
// Dependency tests (Workflow 2)
// ---------------------------------------------------------------------------

/// Helper: reindex the dependency pair and return (fruit_id, uppercased_id)
async fn setup_dependency_pair(state: &AppState) -> (i64, i64) {
    barca_server::reindex(state).await.unwrap();
    let store = state.store.lock().await;
    let assets = store.list_assets().await.unwrap();
    let fruit_id = assets.iter().find(|a| a.function_name == "fruit").unwrap().asset_id;
    let uppercased_id = assets.iter().find(|a| a.function_name == "uppercased").unwrap().asset_id;
    (fruit_id, uppercased_id)
}

/// Helper: run the queue worker until a specific asset completes `n` times (or timeout).
/// For non-partitioned assets use n=1.
async fn run_worker_until_n_completions(state: &AppState, target_asset_id: i64, n: usize) {
    let mut rx = state.job_completion_tx.subscribe();
    let worker_state = state.clone();
    let worker_handle = tokio::spawn(async move {
        barca_server::run_refresh_queue_worker(worker_state).await;
    });
    tokio::spawn(barca_server::run_log_persister(state.clone()));

    let timeout = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        let mut count = 0;
        loop {
            match rx.recv().await {
                Ok(id) if id == target_asset_id => {
                    count += 1;
                    if count >= n {
                        break;
                    }
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
    });
    timeout.await.ok();
    worker_handle.abort();
}

/// Convenience: run worker until one completion for a given asset.
async fn run_worker_until_complete(state: &AppState, target_asset_id: i64) {
    run_worker_until_n_completions(state, target_asset_id, 1).await;
}

#[tokio::test]
async fn test_reindex_with_dependency() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_dependency_pair()).await;
    barca_server::reindex(&state).await.unwrap();

    let store = state.store.lock().await;
    let assets = store.list_assets().await.unwrap();
    assert_eq!(assets.len(), 2);

    // Verify fruit has no inputs
    let fruit = assets.iter().find(|a| a.function_name == "fruit").unwrap();
    let fruit_detail = store.asset_detail(fruit.asset_id).await.unwrap();
    let fruit_inputs = store.get_asset_inputs(fruit_detail.asset.definition_id).await.unwrap();
    assert!(fruit_inputs.is_empty(), "fruit should have no inputs");

    // Verify uppercased has one input pointing to fruit
    let upper = assets.iter().find(|a| a.function_name == "uppercased").unwrap();
    let upper_detail = store.asset_detail(upper.asset_id).await.unwrap();
    let upper_inputs = store.get_asset_inputs(upper_detail.asset.definition_id).await.unwrap();
    assert_eq!(upper_inputs.len(), 1, "uppercased should have 1 input");
    assert_eq!(upper_inputs[0].parameter_name, "fruit");
    assert_eq!(upper_inputs[0].upstream_asset_id, Some(fruit.asset_id));
}

#[tokio::test]
async fn test_enqueue_downstream_enqueues_upstream_first() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_dependency_pair()).await;
    let (fruit_id, uppercased_id) = setup_dependency_pair(&state).await;

    // Enqueue uppercased — should auto-enqueue fruit first
    barca_server::enqueue_refresh_request(&state, uppercased_id).await.unwrap();

    // Verify two jobs: fruit (first) and uppercased (second)
    let store = state.store.lock().await;
    let jobs = store.list_recent_materializations(10).await.unwrap();
    assert_eq!(jobs.len(), 2, "should have 2 queued jobs");

    // Jobs are returned most-recent-first, so fruit (queued first) is last
    let fruit_job = jobs.iter().find(|j| j.1.asset_id == fruit_id).unwrap();
    let upper_job = jobs.iter().find(|j| j.1.asset_id == uppercased_id).unwrap();
    assert!(fruit_job.0.materialization_id < upper_job.0.materialization_id, "fruit should be queued before uppercased");
}

#[tokio::test]
async fn test_materialize_downstream_runs_upstream_first() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_dependency_pair()).await;
    let (_fruit_id, uppercased_id) = setup_dependency_pair(&state).await;

    // Enqueue and run
    barca_server::enqueue_refresh_request(&state, uppercased_id).await.unwrap();
    run_worker_until_complete(&state, uppercased_id).await;

    // Verify both assets have successful materializations
    let store = state.store.lock().await;
    let fruit_mat = store.latest_successful_materialization(_fruit_id).await.unwrap();
    assert!(fruit_mat.is_some(), "fruit should have a successful materialization");

    let upper_mat = store.latest_successful_materialization(uppercased_id).await.unwrap();
    assert!(upper_mat.is_some(), "uppercased should have a successful materialization");

    // Verify the artifact values
    let fruit_artifact = fruit_mat.unwrap().artifact_path.unwrap();
    let fruit_value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(state.repo_root.join(&fruit_artifact)).unwrap()).unwrap();
    assert_eq!(fruit_value, serde_json::json!("banana"));

    let upper_artifact = upper_mat.unwrap().artifact_path.unwrap();
    let upper_value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(state.repo_root.join(&upper_artifact)).unwrap()).unwrap();
    assert_eq!(upper_value, serde_json::json!("BANANA"));
}

#[tokio::test]
async fn test_materialize_downstream_reuses_existing_upstream() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_dependency_pair()).await;
    let (fruit_id, uppercased_id) = setup_dependency_pair(&state).await;

    // First: materialize fruit directly
    barca_server::enqueue_refresh_request(&state, fruit_id).await.unwrap();
    run_worker_until_complete(&state, fruit_id).await;

    let store = state.store.lock().await;
    let fruit_mat = store.latest_successful_materialization(fruit_id).await.unwrap();
    assert!(fruit_mat.is_some(), "fruit should be materialized");
    let fruit_job_id = fruit_mat.unwrap().materialization_id;
    drop(store);

    // Now materialize uppercased — should NOT re-enqueue fruit
    barca_server::enqueue_refresh_request(&state, uppercased_id).await.unwrap();

    let store = state.store.lock().await;
    let jobs = store.list_recent_materializations(10).await.unwrap();
    // Should have 2 total jobs: fruit (from step 1) + uppercased (from step 2)
    // NOT 3 (which would mean fruit was enqueued again)
    assert_eq!(jobs.len(), 2, "should not re-enqueue fruit");
    drop(store);

    run_worker_until_complete(&state, uppercased_id).await;

    // Verify uppercased succeeded using fruit's existing materialization
    let store = state.store.lock().await;
    let upper_mat = store.latest_successful_materialization(uppercased_id).await.unwrap();
    assert!(upper_mat.is_some(), "uppercased should succeed");

    // Verify fruit wasn't re-materialized (still same job ID)
    let fruit_mat_after = store.latest_successful_materialization(fruit_id).await.unwrap().unwrap();
    assert_eq!(fruit_mat_after.materialization_id, fruit_job_id, "fruit should not have been re-materialized");
}

// ---------------------------------------------------------------------------
// Partition tests (Workflow 3)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_enqueue_partitioned_creates_one_job_per_partition() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_partitioned_asset()).await;
    barca_server::reindex(&state).await.unwrap();

    let store = state.store.lock().await;
    let assets = store.list_assets().await.unwrap();
    assert_eq!(assets.len(), 1);
    let asset_id = assets[0].asset_id;
    drop(store);

    barca_server::enqueue_refresh_request(&state, asset_id).await.unwrap();

    let store = state.store.lock().await;
    let jobs = store.list_materializations_for_asset(asset_id, 10).await.unwrap();
    assert_eq!(jobs.len(), 3, "should have 3 queued jobs (one per partition)");

    // Each job should have a distinct partition_key_json
    let mut partition_keys: Vec<String> = jobs.iter().map(|j| j.partition_key_json.clone().unwrap_or_default()).collect();
    partition_keys.sort();
    assert_eq!(partition_keys, vec![r#"{"ticker":"AAPL"}"#, r#"{"ticker":"GOOG"}"#, r#"{"ticker":"MSFT"}"#,]);
}

#[tokio::test]
async fn test_materialize_all_partitions() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_partitioned_asset()).await;
    barca_server::reindex(&state).await.unwrap();

    let store = state.store.lock().await;
    let asset_id = store.list_assets().await.unwrap()[0].asset_id;
    drop(store);

    barca_server::enqueue_refresh_request(&state, asset_id).await.unwrap();
    run_worker_until_n_completions(&state, asset_id, 3).await;

    // All 3 jobs should be successful
    let store = state.store.lock().await;
    let jobs = store.list_materializations_for_asset(asset_id, 10).await.unwrap();
    let successful: Vec<_> = jobs.iter().filter(|j| j.status == "success").collect();
    assert_eq!(successful.len(), 3, "all 3 partitions should succeed");

    // Each partition should have its own artifact with correct value
    for job in &successful {
        let pk: serde_json::Value = serde_json::from_str(job.partition_key_json.as_deref().unwrap()).unwrap();
        let ticker = pk.get("ticker").unwrap().as_str().unwrap();

        let artifact_path = job.artifact_path.as_ref().unwrap();
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(state.repo_root.join(artifact_path)).unwrap()).unwrap();

        assert_eq!(value.get("ticker").unwrap().as_str().unwrap(), ticker, "artifact should contain the correct ticker");
    }
}

#[tokio::test]
async fn test_partition_run_hashes_are_distinct() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_partitioned_asset()).await;
    barca_server::reindex(&state).await.unwrap();

    let store = state.store.lock().await;
    let asset_id = store.list_assets().await.unwrap()[0].asset_id;
    drop(store);

    barca_server::enqueue_refresh_request(&state, asset_id).await.unwrap();
    run_worker_until_n_completions(&state, asset_id, 3).await;

    let store = state.store.lock().await;
    let jobs = store.list_materializations_for_asset(asset_id, 10).await.unwrap();
    let mut run_hashes: Vec<String> = jobs.iter().map(|j| j.run_hash.clone()).collect();
    run_hashes.sort();
    run_hashes.dedup();
    assert_eq!(run_hashes.len(), 3, "each partition should have a distinct run_hash");
}

#[tokio::test]
async fn test_partition_artifacts_stored_in_separate_dirs() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_partitioned_asset()).await;
    barca_server::reindex(&state).await.unwrap();

    let store = state.store.lock().await;
    let asset_id = store.list_assets().await.unwrap()[0].asset_id;
    drop(store);

    barca_server::enqueue_refresh_request(&state, asset_id).await.unwrap();
    run_worker_until_n_completions(&state, asset_id, 3).await;

    let store = state.store.lock().await;
    let jobs = store.list_materializations_for_asset(asset_id, 10).await.unwrap();

    let mut artifact_paths: Vec<String> = jobs.iter().filter_map(|j| j.artifact_path.clone()).collect();
    artifact_paths.sort();

    // All paths should be distinct
    let unique_count = {
        let mut v = artifact_paths.clone();
        v.dedup();
        v.len()
    };
    assert_eq!(unique_count, 3, "each partition should have a unique artifact path");

    // Each path should contain "partitions/ticker="
    for path in &artifact_paths {
        assert!(path.contains("partitions/ticker="), "artifact path should include partition key: {path}");
    }
}

#[tokio::test]
async fn test_second_run_caches_partitions() {
    let (state, _tmp) = test_app_state(MockPythonBridge::with_partitioned_asset()).await;
    barca_server::reindex(&state).await.unwrap();

    let store = state.store.lock().await;
    let asset_id = store.list_assets().await.unwrap()[0].asset_id;
    drop(store);

    // First run: materialize all 3 partitions
    barca_server::enqueue_refresh_request(&state, asset_id).await.unwrap();
    run_worker_until_n_completions(&state, asset_id, 3).await;

    let store = state.store.lock().await;
    let first_run_jobs = store.list_materializations_for_asset(asset_id, 10).await.unwrap();
    assert_eq!(first_run_jobs.len(), 3);
    drop(store);

    // Second run: should enqueue 3 more jobs but all should hit cache
    barca_server::enqueue_refresh_request(&state, asset_id).await.unwrap();
    run_worker_until_n_completions(&state, asset_id, 3).await;

    let store = state.store.lock().await;
    let all_jobs = store.list_materializations_for_asset(asset_id, 10).await.unwrap();
    // 3 from first run + 3 from second run = 6 total
    assert_eq!(all_jobs.len(), 6, "should have 6 total jobs");

    // All 6 should be successful (second run used cache)
    let all_success = all_jobs.iter().all(|j| j.status == "success");
    assert!(all_success, "all jobs should be successful (second run cached)");
}
