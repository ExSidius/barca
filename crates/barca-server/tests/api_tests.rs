use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use barca_core::models::{AssetDetail, AssetSummary, InspectedAsset, JobDetail, WorkerResponse};
use barca_server::python_bridge::{BatchJob, BatchJobResult, PythonBridge};
use barca_server::store::MetadataStore;
use barca_server::{AppState, JobLogEntry};
use serde_json::Value;
use tokio::sync::{broadcast, Mutex};
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// DynamicMockPythonBridge — mutable mock
// ---------------------------------------------------------------------------

type MaterializeFn = Box<dyn Fn(&str, &serde_json::Value, &std::path::Path) -> WorkerResponse + Send + Sync>;

#[derive(Clone)]
struct DynamicMockPythonBridge {
    assets: Arc<Mutex<Vec<InspectedAsset>>>,
    materialize_fn: Arc<MaterializeFn>,
}

impl DynamicMockPythonBridge {
    fn new(assets: Vec<InspectedAsset>, f: MaterializeFn) -> Self {
        Self {
            assets: Arc::new(Mutex::new(assets)),
            materialize_fn: Arc::new(f),
        }
    }

    fn empty() -> Self {
        Self::new(vec![], default_materialize_fn())
    }

    fn with_one_asset() -> Self {
        Self::new(vec![make_simple_asset()], default_materialize_fn())
    }

    fn with_dependency_pair() -> Self {
        Self::new(make_dependency_pair_assets(), default_materialize_fn())
    }

    fn with_partitioned_asset() -> Self {
        Self::new(vec![make_partitioned_asset()], default_materialize_fn())
    }

    async fn set_assets(&self, assets: Vec<InspectedAsset>) {
        let mut guard = self.assets.lock().await;
        *guard = assets;
    }
}

#[async_trait]
impl PythonBridge for DynamicMockPythonBridge {
    async fn inspect_modules(&self, _modules: &[String]) -> anyhow::Result<Vec<InspectedAsset>> {
        let guard = self.assets.lock().await;
        Ok(guard.clone())
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
        _working_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<WorkerResponse> {
        let kwargs: serde_json::Value = input_kwargs_json.map(|s| serde_json::from_str(s).unwrap()).unwrap_or(serde_json::json!({}));

        let response = (self.materialize_fn)(function_name, &kwargs, output_dir);
        Ok(response)
    }

    async fn materialize_batch(
        &self,
        jobs: &[BatchJob],
        _log_tx: broadcast::Sender<JobLogEntry>,
        _working_dir: Option<&std::path::Path>,
    ) -> anyhow::Result<Vec<BatchJobResult>> {
        let mut results = Vec::new();
        for job in jobs {
            let kwargs: serde_json::Value = job
                .input_kwargs_json
                .as_ref()
                .map(|s| serde_json::from_str(s).unwrap())
                .unwrap_or(serde_json::json!({}));
            let response = (self.materialize_fn)(&job.function_name, &kwargs, &job.output_dir);
            results.push(BatchJobResult {
                job_id: job.job_id,
                result: Ok(response),
            });
        }
        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Asset factories
// ---------------------------------------------------------------------------

fn make_simple_asset() -> InspectedAsset {
    InspectedAsset {
        kind: "asset".into(),
        module_path: "example.assets".into(),
        file_path: "/tmp/barca-test/example/assets.py".into(),
        function_name: "my_asset".into(),
        function_source: "def my_asset(): return 42".into(),
        module_source: "from barca import asset\n\n@asset()\ndef my_asset(): return 42".into(),
        decorator_metadata: serde_json::json!({}),
        return_type: Some("int".into()),
        python_version: "3.12.0".into(),
    }
}

fn make_dependency_pair_assets() -> Vec<InspectedAsset> {
    let file_path = "/tmp/barca-test/example/assets.py";
    let module_source = "from barca import asset\n\n@asset()\ndef fruit(): return 'banana'\n\n@asset(inputs={'fruit': fruit})\ndef uppercased(fruit): return fruit.upper()";
    vec![
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
    ]
}

fn make_partitioned_asset() -> InspectedAsset {
    let file_path = "/tmp/barca-test/example/assets.py";
    InspectedAsset {
        kind: "asset".into(),
        module_path: "example.assets".into(),
        file_path: file_path.into(),
        function_name: "fetch_prices".into(),
        function_source: "def fetch_prices(ticker): return {'ticker': ticker}\n".into(),
        module_source: "from barca import asset, partitions\n\n@asset(partitions={'ticker': partitions(['AAPL', 'MSFT', 'GOOG'])})\ndef fetch_prices(ticker): return {'ticker': ticker}\n".into(),
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
    }
}

/// Default materialize function that handles all standard test assets.
fn default_materialize_fn() -> MaterializeFn {
    Box::new(|function_name, kwargs, output_dir| {
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

        WorkerResponse {
            ok: true,
            artifact_format: Some("json".into()),
            value_path: Some(value_path.to_string_lossy().into()),
            result_type: Some("str".into()),
            module_path: Some("example.assets".into()),
            function_name: Some(function_name.into()),
            signature: None,
            error: None,
            error_type: None,
        }
    })
}

// ---------------------------------------------------------------------------
// Scenario builder
// ---------------------------------------------------------------------------

struct Scenario {
    state: AppState,
    _tmp: tempfile::TempDir,
    mock: DynamicMockPythonBridge,
}

impl Scenario {
    async fn new(mock: DynamicMockPythonBridge) -> Self {
        let tmp_dir = tempfile::tempdir().unwrap();
        let repo_root = tmp_dir.path().to_path_buf();
        let db_dir = repo_root.join(".barca");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db_path = db_dir.join("metadata.db");
        let store = MetadataStore::open(&db_path).await.unwrap();
        let state = AppState::new(repo_root, store, Arc::new(mock.clone()));
        Self { state, _tmp: tmp_dir, mock }
    }

    async fn reindex(&self) {
        barca_server::reindex(&self.state).await.unwrap();
    }

    fn repo_root(&self) -> &std::path::Path {
        &self.state.repo_root
    }

    /// Enqueue a refresh and run the worker until the target asset completes.
    async fn refresh(&self, asset_id: i64) {
        barca_server::enqueue_refresh_request(&self.state, asset_id).await.unwrap();
        self.run_worker_until_complete(asset_id).await;
    }

    /// Enqueue a refresh for a partitioned asset and run the worker until `n` partition jobs complete.
    async fn refresh_partitioned(&self, asset_id: i64, n: usize) {
        barca_server::enqueue_refresh_request(&self.state, asset_id).await.unwrap();
        self.run_worker_until_n_completions(asset_id, n).await;
    }

    /// Look up asset id by function name after reindex.
    async fn asset_id_by_name(&self, name: &str) -> i64 {
        let store = self.state.store.lock().await;
        let assets = store.list_assets().await.unwrap();
        assets.iter().find(|a| a.function_name == name).unwrap_or_else(|| panic!("asset '{}' not found", name)).asset_id
    }

    fn router(&self) -> axum::Router {
        barca_server::server::router().with_state(self.state.clone())
    }

    // --- Assertions ---

    async fn assert_fresh(&self, asset_id: i64) {
        let store = self.state.store.lock().await;
        let mat = store.latest_successful_materialization(asset_id).await.unwrap();
        assert!(mat.is_some(), "asset {} should have a successful materialization (be fresh)", asset_id);
    }

    async fn assert_stale(&self, asset_id: i64) {
        let store = self.state.store.lock().await;
        let mat = store.latest_successful_materialization(asset_id).await.unwrap();
        assert!(mat.is_none(), "asset {} should have no successful materialization (be stale)", asset_id);
    }

    async fn assert_artifact_eq(&self, asset_id: i64, expected: Value) {
        let store = self.state.store.lock().await;
        let mat = store
            .latest_successful_materialization(asset_id)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("asset {} has no successful materialization", asset_id));
        let artifact_path = mat.artifact_path.unwrap();
        let value: Value = serde_json::from_str(&std::fs::read_to_string(self.state.repo_root.join(&artifact_path)).unwrap()).unwrap();
        assert_eq!(value, expected, "artifact for asset {} doesn't match expected value", asset_id);
    }

    async fn assert_job_count(&self, asset_id: i64, expected: usize) {
        let store = self.state.store.lock().await;
        let jobs = store.list_materializations_for_asset(asset_id, 100).await.unwrap();
        assert_eq!(jobs.len(), expected, "asset {} should have {} jobs, got {}", asset_id, expected, jobs.len());
    }

    async fn assert_latest_job_status(&self, asset_id: i64, status: &str) {
        let store = self.state.store.lock().await;
        let jobs = store.list_materializations_for_asset(asset_id, 1).await.unwrap();
        assert!(!jobs.is_empty(), "asset {} has no jobs", asset_id);
        assert_eq!(jobs[0].status, status, "asset {} latest job status should be '{}', got '{}'", asset_id, status, jobs[0].status);
    }

    async fn definition_hash(&self, asset_id: i64) -> String {
        let store = self.state.store.lock().await;
        let detail = store.asset_detail(asset_id).await.unwrap();
        detail.asset.definition_hash
    }

    async fn codebase_hash(&self) -> String {
        self.state.current_codebase_hash.lock().await.clone()
    }

    async fn latest_run_hash(&self, asset_id: i64) -> Option<String> {
        let store = self.state.store.lock().await;
        let mat = store.latest_successful_materialization(asset_id).await.unwrap();
        mat.map(|m| m.run_hash)
    }

    async fn assert_all_partitions_succeeded(&self, asset_id: i64, n: usize) {
        let store = self.state.store.lock().await;
        let jobs = store.list_materializations_for_asset(asset_id, 1000).await.unwrap();
        let successful: Vec<_> = jobs.iter().filter(|j| j.status == "success").collect();
        assert_eq!(successful.len(), n, "asset {} should have {} successful partitions, got {}", asset_id, n, successful.len());
    }

    // --- Internal helpers ---

    async fn run_worker_until_complete(&self, target_asset_id: i64) {
        self.run_worker_until_n_completions(target_asset_id, 1).await;
    }

    async fn run_worker_until_n_completions(&self, target_asset_id: i64, n: usize) {
        self.run_worker_until_n_completions_timeout(target_asset_id, n, 5).await;
    }

    async fn run_worker_until_n_completions_timeout(&self, target_asset_id: i64, n: usize, timeout_secs: u64) {
        // Snapshot current terminal count so we wait for n NEW completions
        let baseline = {
            let store = self.state.store.lock().await;
            let jobs = store.list_materializations_for_asset(target_asset_id, 1000).await.unwrap();
            jobs.iter().filter(|j| j.status == "success" || j.status == "failed").count()
        };

        let worker_state = self.state.clone();
        let worker_handle = tokio::spawn(async move {
            barca_server::run_refresh_queue_worker(worker_state).await;
        });
        tokio::spawn(barca_server::run_log_persister(self.state.clone()));

        // Poll the store for terminal jobs (success or failed) rather than
        // relying on the broadcast channel, which can lag under high concurrency.
        let poll_state = self.state.clone();
        let target = baseline + n;
        let timeout = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), async move {
            loop {
                let store = poll_state.store.lock().await;
                let jobs = store.list_materializations_for_asset(target_asset_id, 1000).await.unwrap();
                let done = jobs.iter().filter(|j| j.status == "success" || j.status == "failed").count();
                drop(store);
                if done >= target {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        });
        timeout.await.ok();
        worker_handle.abort();
    }
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

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

// ===========================================================================
// Existing tests — migrated to Scenario
// ===========================================================================

#[tokio::test]
async fn test_list_assets_empty() {
    let s = Scenario::new(DynamicMockPythonBridge::empty()).await;
    let app = s.router();
    let (status, body) = json_get(&app, "/api/assets").await;
    assert_eq!(status, StatusCode::OK);
    let assets: Vec<AssetSummary> = serde_json::from_str(&body).unwrap();
    assert!(assets.is_empty());
}

#[tokio::test]
async fn test_list_assets_after_reindex() {
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;
    s.reindex().await;
    let app = s.router();
    let (status, body) = json_get(&app, "/api/assets").await;
    assert_eq!(status, StatusCode::OK);
    let assets: Vec<AssetSummary> = serde_json::from_str(&body).unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].function_name, "my_asset");
}

#[tokio::test]
async fn test_get_asset_not_found() {
    let s = Scenario::new(DynamicMockPythonBridge::empty()).await;
    let app = s.router();
    let (status, _body) = json_get(&app, "/api/assets/999").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_get_asset_exists() {
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;
    s.reindex().await;
    let app = s.router();

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
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;
    let app = s.router();
    let (status, body) = json_post(&app, "/api/reindex").await;
    assert_eq!(status, StatusCode::OK);
    let assets: Vec<AssetSummary> = serde_json::from_str(&body).unwrap();
    assert_eq!(assets.len(), 1);
}

#[tokio::test]
async fn test_list_jobs_empty() {
    let s = Scenario::new(DynamicMockPythonBridge::empty()).await;
    let app = s.router();
    let (status, body) = json_get(&app, "/api/jobs").await;
    assert_eq!(status, StatusCode::OK);
    let jobs: Vec<JobDetail> = serde_json::from_str(&body).unwrap();
    assert!(jobs.is_empty());
}

#[tokio::test]
async fn test_get_job_not_found() {
    let s = Scenario::new(DynamicMockPythonBridge::empty()).await;
    let app = s.router();
    let (status, _body) = json_get(&app, "/api/jobs/999").await;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_openapi_spec_served() {
    let s = Scenario::new(DynamicMockPythonBridge::empty()).await;
    let app = s.router();
    let (status, body) = json_get(&app, "/api/openapi.json").await;
    assert_eq!(status, StatusCode::OK);
    let spec: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(spec.get("openapi").is_some());
    assert!(spec.get("paths").is_some());
}

#[tokio::test]
async fn test_swagger_ui_served() {
    let s = Scenario::new(DynamicMockPythonBridge::empty()).await;
    let app = s.router();
    let response = app.clone().oneshot(Request::builder().uri("/api/docs/").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_materialize_asset() {
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;
    s.reindex().await;
    let app = s.router();

    let (_, list_body) = json_get(&app, "/api/assets").await;
    let assets: Vec<AssetSummary> = serde_json::from_str(&list_body).unwrap();
    let id = assets[0].asset_id;

    let (status, body) = json_post(&app, &format!("/api/assets/{}/materialize", id)).await;
    assert_eq!(status, StatusCode::OK);
    let detail: AssetDetail = serde_json::from_str(&body).unwrap();
    assert_eq!(detail.asset.asset_id, id);
    assert!(detail.latest_materialization.is_some());
}

// ===========================================================================
// Workflow 1: Single asset, no inputs
// ===========================================================================

#[tokio::test]
async fn test_w1_reindex_discovers_asset() {
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;
    s.reindex().await;

    let store = s.state.store.lock().await;
    let assets = store.list_assets().await.unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].function_name, "my_asset");
}

#[tokio::test]
async fn test_w1_refresh_produces_artifact() {
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;
    s.reindex().await;
    let id = s.asset_id_by_name("my_asset").await;

    s.refresh(id).await;

    s.assert_fresh(id).await;
    s.assert_artifact_eq(id, serde_json::json!(42)).await;
}

#[tokio::test]
async fn test_w1_second_refresh_is_cached() {
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;
    s.reindex().await;
    let id = s.asset_id_by_name("my_asset").await;

    s.refresh(id).await;
    s.assert_fresh(id).await;

    // Second enqueue — should resolve from cache without creating a new job
    barca_server::enqueue_refresh_request(&s.state, id).await.unwrap();
    s.assert_job_count(id, 1).await;
}

// ===========================================================================
// Workflow 2: Dependencies
// ===========================================================================

#[tokio::test]
async fn test_w2_reindex_stores_input_linkage() {
    let s = Scenario::new(DynamicMockPythonBridge::with_dependency_pair()).await;
    s.reindex().await;

    let store = s.state.store.lock().await;
    let assets = store.list_assets().await.unwrap();
    assert_eq!(assets.len(), 2);

    let fruit = assets.iter().find(|a| a.function_name == "fruit").unwrap();
    let fruit_detail = store.asset_detail(fruit.asset_id).await.unwrap();
    let fruit_inputs = store.get_asset_inputs(fruit_detail.asset.definition_id).await.unwrap();
    assert!(fruit_inputs.is_empty(), "fruit should have no inputs");

    let upper = assets.iter().find(|a| a.function_name == "uppercased").unwrap();
    let upper_detail = store.asset_detail(upper.asset_id).await.unwrap();
    let upper_inputs = store.get_asset_inputs(upper_detail.asset.definition_id).await.unwrap();
    assert_eq!(upper_inputs.len(), 1, "uppercased should have 1 input");
    assert_eq!(upper_inputs[0].parameter_name, "fruit");
    assert_eq!(upper_inputs[0].upstream_asset_id, Some(fruit.asset_id));
}

#[tokio::test]
async fn test_w2_refresh_downstream_enqueues_upstream_first() {
    let s = Scenario::new(DynamicMockPythonBridge::with_dependency_pair()).await;
    s.reindex().await;
    let fruit_id = s.asset_id_by_name("fruit").await;
    let uppercased_id = s.asset_id_by_name("uppercased").await;

    barca_server::enqueue_refresh_request(&s.state, uppercased_id).await.unwrap();

    let store = s.state.store.lock().await;
    let jobs = store.list_recent_materializations(10).await.unwrap();
    assert_eq!(jobs.len(), 2, "should have 2 queued jobs");

    let fruit_job = jobs.iter().find(|j| j.1.asset_id == fruit_id).unwrap();
    let upper_job = jobs.iter().find(|j| j.1.asset_id == uppercased_id).unwrap();
    assert!(fruit_job.0.materialization_id < upper_job.0.materialization_id, "fruit should be queued before uppercased");
}

#[tokio::test]
async fn test_w2_downstream_receives_upstream_artifact() {
    let s = Scenario::new(DynamicMockPythonBridge::with_dependency_pair()).await;
    s.reindex().await;
    let fruit_id = s.asset_id_by_name("fruit").await;
    let uppercased_id = s.asset_id_by_name("uppercased").await;

    // Enqueue uppercased (auto-enqueues fruit) and run both
    barca_server::enqueue_refresh_request(&s.state, uppercased_id).await.unwrap();
    s.run_worker_until_complete(uppercased_id).await;

    s.assert_fresh(fruit_id).await;
    s.assert_fresh(uppercased_id).await;
    s.assert_artifact_eq(fruit_id, serde_json::json!("banana")).await;
    s.assert_artifact_eq(uppercased_id, serde_json::json!("BANANA")).await;
}

#[tokio::test]
async fn test_w2_upstream_already_materialized_is_reused() {
    let s = Scenario::new(DynamicMockPythonBridge::with_dependency_pair()).await;
    s.reindex().await;
    let fruit_id = s.asset_id_by_name("fruit").await;
    let uppercased_id = s.asset_id_by_name("uppercased").await;

    // Materialize fruit first
    s.refresh(fruit_id).await;
    let store = s.state.store.lock().await;
    let fruit_mat = store.latest_successful_materialization(fruit_id).await.unwrap();
    let fruit_job_id = fruit_mat.unwrap().materialization_id;
    drop(store);

    // Now materialize uppercased — should NOT re-enqueue fruit
    barca_server::enqueue_refresh_request(&s.state, uppercased_id).await.unwrap();
    let store = s.state.store.lock().await;
    let jobs = store.list_recent_materializations(10).await.unwrap();
    assert_eq!(jobs.len(), 2, "should not re-enqueue fruit");
    drop(store);

    s.run_worker_until_complete(uppercased_id).await;

    s.assert_fresh(uppercased_id).await;

    // Verify fruit wasn't re-materialized
    let store = s.state.store.lock().await;
    let fruit_mat_after = store.latest_successful_materialization(fruit_id).await.unwrap().unwrap();
    assert_eq!(fruit_mat_after.materialization_id, fruit_job_id, "fruit should not have been re-materialized");
}

#[tokio::test]
async fn test_w2_change_upstream_code_invalidates_downstream() {
    let s = Scenario::new(DynamicMockPythonBridge::with_dependency_pair()).await;
    s.reindex().await;
    let fruit_id = s.asset_id_by_name("fruit").await;
    let uppercased_id = s.asset_id_by_name("uppercased").await;

    // Materialize both
    barca_server::enqueue_refresh_request(&s.state, uppercased_id).await.unwrap();
    s.run_worker_until_complete(uppercased_id).await;
    s.assert_fresh(fruit_id).await;
    s.assert_fresh(uppercased_id).await;

    // Collect run_hashes from first run
    let store = s.state.store.lock().await;
    let first_run_jobs = store.list_materializations_for_asset(uppercased_id, 10).await.unwrap();
    let first_run_hashes: Vec<String> = first_run_jobs.iter().filter(|j| j.status == "success").map(|j| j.run_hash.clone()).collect();
    assert_eq!(first_run_hashes.len(), 1);
    let old_upper_run_hash = first_run_hashes[0].clone();
    drop(store);

    // Change upstream (fruit) source code → reindex
    let file_path = "/tmp/barca-test/example/assets.py";
    let new_module_source = "from barca import asset\n\n@asset()\ndef fruit(): return 'apple'\n\n@asset(inputs={'fruit': fruit})\ndef uppercased(fruit): return fruit.upper()";
    s.mock
        .set_assets(vec![
            InspectedAsset {
                kind: "asset".into(),
                module_path: "example.assets".into(),
                file_path: file_path.into(),
                function_name: "fruit".into(),
                function_source: "def fruit(): return 'apple'\n".into(),
                module_source: new_module_source.into(),
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
                module_source: new_module_source.into(),
                decorator_metadata: serde_json::json!({
                    "kind": "asset",
                    "inputs": {
                        "fruit": format!("{file_path}:fruit")
                    }
                }),
                return_type: Some("str".into()),
                python_version: "3.12.0".into(),
            },
        ])
        .await;
    s.reindex().await;

    // After reindex with changed upstream code, fruit's definition hash changes.
    // We must refresh fruit first (new definition → new materialization),
    // then refreshing uppercased should produce a different run_hash.
    // Sleep 1s to ensure different created_at timestamp (seconds precision).
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    s.refresh(fruit_id).await;

    barca_server::enqueue_refresh_request(&s.state, uppercased_id).await.unwrap();
    s.run_worker_until_complete(uppercased_id).await;

    // Find the newest uppercased materialization by highest ID
    let store = s.state.store.lock().await;
    let all_upper_jobs = store.list_materializations_for_asset(uppercased_id, 100).await.unwrap();
    let new_upper_mat = all_upper_jobs
        .iter()
        .filter(|j| j.status == "success")
        .max_by_key(|j| j.materialization_id)
        .expect("uppercased should have a new successful materialization");
    assert_ne!(new_upper_mat.run_hash, old_upper_run_hash, "downstream run_hash should change when upstream code changes");
}

#[tokio::test]
async fn test_w2_revert_upstream_reuses_old_downstream_cache() {
    let s = Scenario::new(DynamicMockPythonBridge::with_dependency_pair()).await;
    s.reindex().await;
    let fruit_id = s.asset_id_by_name("fruit").await;
    let uppercased_id = s.asset_id_by_name("uppercased").await;

    // Materialize both with original code
    barca_server::enqueue_refresh_request(&s.state, uppercased_id).await.unwrap();
    s.run_worker_until_complete(uppercased_id).await;
    s.assert_fresh(fruit_id).await;
    s.assert_fresh(uppercased_id).await;

    let store = s.state.store.lock().await;
    let original_upper_mat = store.latest_successful_materialization(uppercased_id).await.unwrap().unwrap();
    let original_run_hash = original_upper_mat.run_hash.clone();
    drop(store);

    // Change upstream code → reindex → materialize
    let file_path = "/tmp/barca-test/example/assets.py";
    let changed_module = "from barca import asset\n\n@asset()\ndef fruit(): return 'kiwi'\n\n@asset(inputs={'fruit': fruit})\ndef uppercased(fruit): return fruit.upper()";
    s.mock
        .set_assets(vec![
            InspectedAsset {
                kind: "asset".into(),
                module_path: "example.assets".into(),
                file_path: file_path.into(),
                function_name: "fruit".into(),
                function_source: "def fruit(): return 'kiwi'\n".into(),
                module_source: changed_module.into(),
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
                module_source: changed_module.into(),
                decorator_metadata: serde_json::json!({
                    "kind": "asset",
                    "inputs": { "fruit": format!("{file_path}:fruit") }
                }),
                return_type: Some("str".into()),
                python_version: "3.12.0".into(),
            },
        ])
        .await;
    s.reindex().await;
    barca_server::enqueue_refresh_request(&s.state, uppercased_id).await.unwrap();
    s.run_worker_until_complete(uppercased_id).await;

    // Now revert back to original code → reindex → materialize
    s.mock.set_assets(make_dependency_pair_assets()).await;
    s.reindex().await;
    barca_server::enqueue_refresh_request(&s.state, uppercased_id).await.unwrap();
    s.run_worker_until_complete(uppercased_id).await;

    // The run_hash should match the original (same upstream materialization)
    let store = s.state.store.lock().await;
    let reverted_upper_mat = store.latest_successful_materialization(uppercased_id).await.unwrap().unwrap();
    assert_eq!(
        reverted_upper_mat.run_hash, original_run_hash,
        "reverting upstream code should produce the same run_hash as the original"
    );
}

#[tokio::test]
async fn test_w2_upstream_fails_downstream_not_run() {
    // Create a mock where fruit fails
    let failing_mock = DynamicMockPythonBridge::new(
        make_dependency_pair_assets(),
        Box::new(|function_name, kwargs, output_dir| {
            if function_name == "fruit" {
                // Write a failing result
                std::fs::create_dir_all(output_dir).unwrap();
                let result = serde_json::json!({
                    "ok": false,
                    "error": "fruit computation failed",
                    "error_type": "RuntimeError",
                });
                std::fs::write(output_dir.join("result.json"), serde_json::to_string(&result).unwrap()).unwrap();
                return WorkerResponse {
                    ok: false,
                    artifact_format: None,
                    value_path: None,
                    result_type: None,
                    module_path: Some("example.assets".into()),
                    function_name: Some("fruit".into()),
                    signature: None,
                    error: Some("fruit computation failed".into()),
                    error_type: Some("RuntimeError".into()),
                };
            }
            // uppercased should not be called, but handle it anyway
            let value = kwargs.get("fruit").and_then(|v| v.as_str()).unwrap_or("MISSING");
            std::fs::create_dir_all(output_dir).unwrap();
            let value_path = output_dir.join("value.json");
            std::fs::write(&value_path, serde_json::to_string(&serde_json::json!(value.to_uppercase())).unwrap()).unwrap();
            std::fs::write(
                output_dir.join("result.json"),
                serde_json::to_string(&serde_json::json!({
                    "ok": true, "artifact_format": "json", "value_path": value_path.to_string_lossy(),
                }))
                .unwrap(),
            )
            .unwrap();
            WorkerResponse {
                ok: true,
                artifact_format: Some("json".into()),
                value_path: Some(value_path.to_string_lossy().into()),
                result_type: Some("str".into()),
                module_path: Some("example.assets".into()),
                function_name: Some(function_name.into()),
                signature: None,
                error: None,
                error_type: None,
            }
        }),
    );
    let s = Scenario::new(failing_mock).await;
    s.reindex().await;
    let fruit_id = s.asset_id_by_name("fruit").await;
    let uppercased_id = s.asset_id_by_name("uppercased").await;

    // Enqueue uppercased (which auto-enqueues fruit) and try to run
    barca_server::enqueue_refresh_request(&s.state, uppercased_id).await.unwrap();

    // Run the worker — fruit will fail
    // We listen for fruit's completion (even failure sends a completion signal)
    s.run_worker_until_complete(fruit_id).await;

    // Give a brief moment for any downstream processing
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // fruit should have failed
    s.assert_latest_job_status(fruit_id, "failed").await;

    // uppercased should NOT have a successful materialization
    s.assert_stale(uppercased_id).await;
}

// ===========================================================================
// Workflow 3: Partitions
// ===========================================================================

#[tokio::test]
async fn test_w3_enqueue_creates_one_job_per_partition() {
    let s = Scenario::new(DynamicMockPythonBridge::with_partitioned_asset()).await;
    s.reindex().await;
    let id = s.asset_id_by_name("fetch_prices").await;

    barca_server::enqueue_refresh_request(&s.state, id).await.unwrap();

    let store = s.state.store.lock().await;
    let jobs = store.list_materializations_for_asset(id, 10).await.unwrap();
    assert_eq!(jobs.len(), 3, "should have 3 queued jobs (one per partition)");

    let mut partition_keys: Vec<String> = jobs.iter().map(|j| j.partition_key_json.clone().unwrap_or_default()).collect();
    partition_keys.sort();
    assert_eq!(partition_keys, vec![r#"{"ticker":"AAPL"}"#, r#"{"ticker":"GOOG"}"#, r#"{"ticker":"MSFT"}"#]);
}

#[tokio::test]
async fn test_w3_all_partitions_execute_with_correct_kwargs() {
    let s = Scenario::new(DynamicMockPythonBridge::with_partitioned_asset()).await;
    s.reindex().await;
    let id = s.asset_id_by_name("fetch_prices").await;

    s.refresh_partitioned(id, 3).await;

    s.assert_all_partitions_succeeded(id, 3).await;

    // Verify each partition got the right ticker value in its artifact
    let store = s.state.store.lock().await;
    let jobs = store.list_materializations_for_asset(id, 10).await.unwrap();
    for job in &jobs {
        let pk: serde_json::Value = serde_json::from_str(job.partition_key_json.as_deref().unwrap()).unwrap();
        let ticker = pk.get("ticker").unwrap().as_str().unwrap();
        let artifact_path = job.artifact_path.as_ref().unwrap();
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(s.state.repo_root.join(artifact_path)).unwrap()).unwrap();
        assert_eq!(value.get("ticker").unwrap().as_str().unwrap(), ticker);
    }
}

#[tokio::test]
async fn test_w3_partition_run_hashes_are_distinct() {
    let s = Scenario::new(DynamicMockPythonBridge::with_partitioned_asset()).await;
    s.reindex().await;
    let id = s.asset_id_by_name("fetch_prices").await;

    s.refresh_partitioned(id, 3).await;

    let store = s.state.store.lock().await;
    let jobs = store.list_materializations_for_asset(id, 10).await.unwrap();
    let mut run_hashes: Vec<String> = jobs.iter().map(|j| j.run_hash.clone()).collect();
    run_hashes.sort();
    run_hashes.dedup();
    assert_eq!(run_hashes.len(), 3, "each partition should have a distinct run_hash");
}

#[tokio::test]
async fn test_w3_artifacts_in_separate_partition_dirs() {
    let s = Scenario::new(DynamicMockPythonBridge::with_partitioned_asset()).await;
    s.reindex().await;
    let id = s.asset_id_by_name("fetch_prices").await;

    s.refresh_partitioned(id, 3).await;

    let store = s.state.store.lock().await;
    let jobs = store.list_materializations_for_asset(id, 10).await.unwrap();

    let mut artifact_paths: Vec<String> = jobs.iter().filter_map(|j| j.artifact_path.clone()).collect();
    artifact_paths.sort();

    let unique_count = {
        let mut v = artifact_paths.clone();
        v.dedup();
        v.len()
    };
    assert_eq!(unique_count, 3, "each partition should have a unique artifact path");

    for path in &artifact_paths {
        assert!(path.contains("partitions/ticker="), "artifact path should include partition key: {path}");
    }
}

#[tokio::test]
async fn test_w3_second_run_caches_all_partitions() {
    let s = Scenario::new(DynamicMockPythonBridge::with_partitioned_asset()).await;
    s.reindex().await;
    let id = s.asset_id_by_name("fetch_prices").await;

    // First run
    s.refresh_partitioned(id, 3).await;
    s.assert_all_partitions_succeeded(id, 3).await;

    // Second run — should enqueue 3 more but all hit cache
    s.refresh_partitioned(id, 3).await;

    let store = s.state.store.lock().await;
    let all_jobs = store.list_materializations_for_asset(id, 100).await.unwrap();
    assert_eq!(all_jobs.len(), 6, "should have 6 total jobs (3+3)");

    let all_success = all_jobs.iter().all(|j| j.status == "success");
    assert!(all_success, "all jobs should be successful (second run cached)");
}

#[tokio::test]
async fn test_w3_parallel_partition_execution() {
    // This test verifies all 3 partitions can spawn concurrently
    let s = Scenario::new(DynamicMockPythonBridge::with_partitioned_asset()).await;
    s.reindex().await;
    let id = s.asset_id_by_name("fetch_prices").await;

    barca_server::enqueue_refresh_request(&s.state, id).await.unwrap();

    // Verify all 3 are queued before running worker
    let store = s.state.store.lock().await;
    let jobs = store.list_materializations_for_asset(id, 10).await.unwrap();
    assert_eq!(jobs.len(), 3);
    assert!(jobs.iter().all(|j| j.status == "queued"), "all 3 should be queued initially");
    drop(store);

    // Run worker — all 3 should complete
    s.run_worker_until_n_completions(id, 3).await;
    s.assert_all_partitions_succeeded(id, 3).await;
}

#[tokio::test]
async fn test_w3_large_partition_set_100() {
    // Generate a partitioned asset with 100 partition values
    let values: Vec<String> = (0..100).map(|i| format!("p{i:03}")).collect();
    let values_json = serde_json::to_string(&values).unwrap();
    let file_path = "/tmp/barca-test/example/assets.py";
    let asset = InspectedAsset {
        kind: "asset".into(),
        module_path: "example.assets".into(),
        file_path: file_path.into(),
        function_name: "wide_asset".into(),
        function_source: "def wide_asset(key): return {'key': key}\n".into(),
        module_source: format!("from barca import asset, partitions\n\n@asset(partitions={{'key': partitions({values_json})}})\ndef wide_asset(key): return {{'key': key}}\n"),
        decorator_metadata: serde_json::json!({
            "kind": "asset",
            "partitions": {
                "key": {
                    "kind": "inline",
                    "values_json": values_json
                }
            }
        }),
        return_type: Some("dict".into()),
        python_version: "3.12.0".into(),
    };

    let materialize_fn: MaterializeFn = Box::new(|function_name, kwargs, output_dir| {
        let key = kwargs.get("key").and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
        let value = serde_json::json!({"key": key});

        std::fs::create_dir_all(output_dir).unwrap();
        let value_path = output_dir.join("value.json");
        std::fs::write(&value_path, serde_json::to_string(&value).unwrap()).unwrap();
        std::fs::write(
            output_dir.join("result.json"),
            serde_json::to_string(&serde_json::json!({
                "ok": true, "artifact_format": "json",
                "value_path": value_path.to_string_lossy(),
                "result_type": "dict", "module_path": "example.assets",
                "function_name": function_name,
            }))
            .unwrap(),
        )
        .unwrap();

        WorkerResponse {
            ok: true,
            artifact_format: Some("json".into()),
            value_path: Some(value_path.to_string_lossy().into()),
            result_type: Some("dict".into()),
            module_path: Some("example.assets".into()),
            function_name: Some(function_name.into()),
            signature: None,
            error: None,
            error_type: None,
        }
    });

    let s = Scenario::new(DynamicMockPythonBridge::new(vec![asset], materialize_fn)).await;
    s.reindex().await;
    let id = s.asset_id_by_name("wide_asset").await;

    // Enqueue — should create 100 jobs
    barca_server::enqueue_refresh_request(&s.state, id).await.unwrap();

    let store = s.state.store.lock().await;
    let jobs = store.list_materializations_for_asset(id, 1000).await.unwrap();
    assert_eq!(jobs.len(), 100, "should have 100 queued jobs");
    drop(store);

    // Run all 100 partitions (longer timeout)
    s.run_worker_until_n_completions_timeout(id, 100, 30).await;

    // All 100 should succeed
    s.assert_all_partitions_succeeded(id, 100).await;

    // Verify distinct partition keys and run_hashes
    let store = s.state.store.lock().await;
    let jobs = store.list_materializations_for_asset(id, 1000).await.unwrap();

    let mut partition_keys: Vec<String> = jobs.iter().filter_map(|j| j.partition_key_json.clone()).collect();
    partition_keys.sort();
    partition_keys.dedup();
    assert_eq!(partition_keys.len(), 100, "100 distinct partition keys");

    let mut run_hashes: Vec<String> = jobs.iter().map(|j| j.run_hash.clone()).collect();
    run_hashes.sort();
    run_hashes.dedup();
    assert_eq!(run_hashes.len(), 100, "100 distinct run_hashes");

    // Spot-check a few artifacts
    for job in jobs.iter().take(3) {
        let pk: serde_json::Value = serde_json::from_str(job.partition_key_json.as_deref().unwrap()).unwrap();
        let key = pk.get("key").unwrap().as_str().unwrap();
        let artifact_path = job.artifact_path.as_ref().unwrap();
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(s.state.repo_root.join(artifact_path)).unwrap()).unwrap();
        assert_eq!(value.get("key").unwrap().as_str().unwrap(), key);
    }
}

// ===========================================================================
// Workflow 4: Asset Continuity (rename/move)
// ===========================================================================

/// Helper: create an unnamed asset at a given file path.
fn make_unnamed_asset_at(file_path: &str, function_name: &str) -> InspectedAsset {
    let source = format!("def {function_name}(): return 42");
    let module_source = format!("from barca import asset\n\n@asset()\n{source}");
    InspectedAsset {
        kind: "asset".into(),
        module_path: "example.assets".into(),
        file_path: file_path.into(),
        function_name: function_name.into(),
        function_source: source,
        module_source,
        decorator_metadata: serde_json::json!({}),
        return_type: Some("int".into()),
        python_version: "3.12.0".into(),
    }
}

/// Helper: create a named asset (`@asset(name="...")`) at a given file path.
fn make_named_asset_at(file_path: &str, function_name: &str, name: &str) -> InspectedAsset {
    let source = format!("def {function_name}(): return 42");
    let module_source = format!("from barca import asset\n\n@asset(name=\"{name}\")\n{source}");
    InspectedAsset {
        kind: "asset".into(),
        module_path: "example.assets".into(),
        file_path: file_path.into(),
        function_name: function_name.into(),
        function_source: source,
        module_source,
        decorator_metadata: serde_json::json!({"name": name}),
        return_type: Some("int".into()),
        python_version: "3.12.0".into(),
    }
}

#[tokio::test]
async fn test_w4_unnamed_asset_rename_creates_new_lineage() {
    // Unnamed asset: continuity_key = "relative_file:function_name".
    // Renaming the file produces a different continuity_key → new asset row.
    let original = make_unnamed_asset_at("/tmp/barca-test/example/assets.py", "my_asset");
    let s = Scenario::new(DynamicMockPythonBridge::new(vec![original], default_materialize_fn())).await;
    s.reindex().await;

    let store = s.state.store.lock().await;
    let assets = store.list_assets().await.unwrap();
    assert_eq!(assets.len(), 1);
    let old_id = assets[0].asset_id;
    let old_continuity_key = assets[0].logical_name.clone();
    drop(store);

    // "Rename" the file
    let renamed = make_unnamed_asset_at("/tmp/barca-test/example/renamed_assets.py", "my_asset");
    s.mock.set_assets(vec![renamed]).await;
    s.reindex().await;

    let store = s.state.store.lock().await;
    let assets = store.list_assets().await.unwrap();
    // Old asset still in DB (reindex only upserts, doesn't delete) + new asset
    assert_eq!(assets.len(), 2, "renamed unnamed asset should create a new lineage");

    let new_asset = assets.iter().find(|a| a.asset_id != old_id).unwrap();
    assert_ne!(new_asset.logical_name, old_continuity_key, "new asset should have a different continuity_key");
    assert!(new_asset.logical_name.contains("renamed_assets.py"), "new continuity_key should reflect the new file path");
}

#[tokio::test]
async fn test_w4_named_asset_rename_preserves_lineage() {
    // Named asset: continuity_key = explicit name.
    // Renaming the function preserves the continuity_key → same asset row.
    let original = make_named_asset_at("/tmp/barca-test/example/assets.py", "get_prices", "prices");
    let s = Scenario::new(DynamicMockPythonBridge::new(vec![original], default_materialize_fn())).await;
    s.reindex().await;

    let store = s.state.store.lock().await;
    let assets = store.list_assets().await.unwrap();
    assert_eq!(assets.len(), 1);
    let original_id = assets[0].asset_id;
    assert_eq!(assets[0].logical_name, "prices");
    assert_eq!(assets[0].function_name, "get_prices");
    drop(store);

    // Rename the function (but keep the same @asset(name="prices"))
    let renamed = make_named_asset_at("/tmp/barca-test/example/assets.py", "fetch_prices", "prices");
    s.mock.set_assets(vec![renamed]).await;
    s.reindex().await;

    let store = s.state.store.lock().await;
    let assets = store.list_assets().await.unwrap();
    assert_eq!(assets.len(), 1, "named asset should keep the same lineage after function rename");
    assert_eq!(assets[0].asset_id, original_id, "asset_id should be preserved");
    assert_eq!(assets[0].logical_name, "prices", "continuity_key should be preserved");
    assert_eq!(assets[0].function_name, "fetch_prices", "function_name should be updated");
}

#[tokio::test]
async fn test_w4_named_asset_move_preserves_materializations() {
    // Named asset: moving to a different file preserves the asset and its materializations.
    let original = make_named_asset_at("/tmp/barca-test/example/assets.py", "get_prices", "prices");
    let s = Scenario::new(DynamicMockPythonBridge::new(vec![original], default_materialize_fn())).await;
    s.reindex().await;
    let id = s.asset_id_by_name("get_prices").await;

    // Materialize
    s.refresh(id).await;
    s.assert_fresh(id).await;
    s.assert_artifact_eq(id, serde_json::json!(42)).await;

    let store = s.state.store.lock().await;
    let mat_before = store.latest_successful_materialization(id).await.unwrap().unwrap();
    let mat_id_before = mat_before.materialization_id;
    drop(store);

    // Move the asset to a different file (same name)
    let moved = make_named_asset_at("/tmp/barca-test/example/new_module.py", "get_prices", "prices");
    s.mock.set_assets(vec![moved]).await;
    s.reindex().await;

    // Asset should still exist with same id
    let store = s.state.store.lock().await;
    let assets = store.list_assets().await.unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].asset_id, id, "asset_id preserved after file move");
    assert!(assets[0].file_path.contains("new_module.py"), "file_path updated");

    // Materialization should still be there
    let mat_after = store.latest_successful_materialization(id).await.unwrap().unwrap();
    assert_eq!(mat_after.materialization_id, mat_id_before, "materialization preserved after file move");
}

#[tokio::test]
async fn test_w4_duplicate_continuity_key_fails_reindex() {
    // Two assets with the same explicit name should fail reindex.
    let asset1 = make_named_asset_at("/tmp/barca-test/example/a.py", "get_a", "shared_name");
    let asset2 = make_named_asset_at("/tmp/barca-test/example/b.py", "get_b", "shared_name");
    let s = Scenario::new(DynamicMockPythonBridge::new(vec![asset1, asset2], default_materialize_fn())).await;

    let result = barca_server::reindex(&s.state).await;
    assert!(result.is_err(), "duplicate continuity_key should fail reindex");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("duplicate continuity key"), "error should mention duplicate: {err_msg}");
}

#[tokio::test]
async fn test_w4_old_materializations_remain_queryable() {
    // After an unnamed asset is renamed (new lineage), old materializations
    // should still be queryable under the old asset id.
    let original = make_unnamed_asset_at("/tmp/barca-test/example/assets.py", "my_asset");
    let s = Scenario::new(DynamicMockPythonBridge::new(vec![original], default_materialize_fn())).await;
    s.reindex().await;
    let old_id = s.asset_id_by_name("my_asset").await;

    // Materialize
    s.refresh(old_id).await;
    s.assert_fresh(old_id).await;

    let store = s.state.store.lock().await;
    let old_mat = store.latest_successful_materialization(old_id).await.unwrap().unwrap();
    let old_mat_id = old_mat.materialization_id;
    drop(store);

    // "Rename" file → creates new lineage
    let renamed = make_unnamed_asset_at("/tmp/barca-test/example/renamed.py", "my_asset");
    s.mock.set_assets(vec![renamed]).await;
    s.reindex().await;

    // Old asset's materialization should still be queryable
    let store = s.state.store.lock().await;
    let old_mat_after = store.latest_successful_materialization(old_id).await.unwrap();
    assert!(old_mat_after.is_some(), "old materialization should still be queryable");
    assert_eq!(old_mat_after.unwrap().materialization_id, old_mat_id, "same materialization id");

    // Old asset should still be in asset_detail
    let old_detail = store.asset_detail(old_id).await;
    assert!(old_detail.is_ok(), "old asset detail should still be queryable");
}

// ===========================================================================
// Workflow 5: Schedules & Reconciliation — not yet implemented
// ===========================================================================

#[tokio::test]
#[ignore = "workflow 5: schedules not yet implemented"]
async fn test_w5_manual_schedule_only_runs_on_explicit_request() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 5: schedules not yet implemented"]
async fn test_w5_always_schedule_runs_when_stale_and_upstream_ready() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 5: schedules not yet implemented"]
async fn test_w5_cron_schedule_waits_for_window() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 5: schedules not yet implemented"]
async fn test_w5_stale_waiting_for_upstream_vs_schedule_are_distinct() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 5: schedules not yet implemented"]
async fn test_w5_partitioned_asset_evaluates_per_partition() {
    todo!()
}

// ===========================================================================
// Workflow 6: Sensors — not yet implemented
// ===========================================================================

#[tokio::test]
#[ignore = "workflow 6: sensors not yet implemented"]
async fn test_w6_sensor_discovered_from_decorator() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 6: sensors not yet implemented"]
async fn test_w6_sensor_feeds_asset_as_input() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 6: sensors not yet implemented"]
async fn test_w6_sensor_update_detected_false_no_downstream_stale() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 6: sensors not yet implemented"]
async fn test_w6_sensor_update_detected_true_marks_downstream_stale() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 6: sensors not yet implemented"]
async fn test_w6_sensor_observations_stored_historically() {
    todo!()
}

// ===========================================================================
// Workflow 7: Notebook Helpers — not yet implemented
// ===========================================================================

#[tokio::test]
#[ignore = "workflow 7: notebook helpers not yet implemented"]
async fn test_w7_load_inputs_returns_kwargs_dict() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 7: notebook helpers not yet implemented"]
async fn test_w7_load_inputs_keys_match_parameter_names() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 7: notebook helpers not yet implemented"]
async fn test_w7_call_with_load_inputs_produces_correct_result() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 7: notebook helpers not yet implemented"]
async fn test_w7_load_inputs_with_partition_key() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 7: notebook helpers not yet implemented"]
async fn test_w7_load_inputs_with_sensor_input() {
    todo!()
}

// ===========================================================================
// Workflow 8: Backfill & Replay — not yet implemented
// ===========================================================================

#[tokio::test]
#[ignore = "workflow 8: backfill/replay not yet implemented"]
async fn test_w8_backfill_over_partition_range() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 8: backfill/replay not yet implemented"]
async fn test_w8_replay_against_specific_upstream_materialization() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 8: backfill/replay not yet implemented"]
async fn test_w8_replay_metadata_includes_mode() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 8: backfill/replay not yet implemented"]
async fn test_w8_same_provenance_replay_hits_cache() {
    todo!()
}

// ===========================================================================
// Workflow 9: Execution Controls — not yet implemented
// ===========================================================================

#[tokio::test]
#[ignore = "workflow 9: execution controls not yet implemented"]
async fn test_w9_timeout_terminates_worker() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 9: execution controls not yet implemented"]
async fn test_w9_retry_on_failure_up_to_3_attempts() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 9: execution controls not yet implemented"]
async fn test_w9_cancel_running_job() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 9: execution controls not yet implemented"]
async fn test_w9_cancelled_output_never_published() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 9: execution controls not yet implemented"]
async fn test_w9_ad_hoc_params_in_cache_key() {
    todo!()
}

#[tokio::test]
#[ignore = "workflow 9: execution controls not yet implemented"]
async fn test_w9_same_params_same_provenance_reuses_cache() {
    todo!()
}

// ===========================================================================
// Cross-cutting
// ===========================================================================

#[tokio::test]
#[ignore = "cross-cutting: diamond dependency not yet tested"]
async fn test_cross_diamond_dependency_a_b_c_d() {
    // D depends on B and C; B and C both depend on A
    // Materializing D should materialize A once, then B+C, then D
    todo!()
}

#[tokio::test]
#[ignore = "cross-cutting: three-level chain not yet tested"]
async fn test_cross_three_level_chain_a_b_c() {
    // C depends on B, B depends on A
    // Materializing C should materialize A, then B, then C
    todo!()
}

#[tokio::test]
async fn test_cross_concurrent_enqueue_deduplication() {
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;
    s.reindex().await;
    let id = s.asset_id_by_name("my_asset").await;

    // Enqueue twice in quick succession — second should join first
    barca_server::enqueue_refresh_request(&s.state, id).await.unwrap();
    barca_server::enqueue_refresh_request(&s.state, id).await.unwrap();

    // Should still only have 1 job (second joined the first)
    s.assert_job_count(id, 1).await;
}

// ===========================================================================
// Workflow 5: Codebase-level change detection (far-off dependencies)
// ===========================================================================

#[tokio::test]
async fn test_w5_helper_module_change_invalidates_definition_hash() {
    // Scenario: asset in assets.py imports from helpers.py.
    // Changing helpers.py (not the asset's own file) should produce
    // a new codebase_hash and therefore a new definition_hash.
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;

    // Write an initial helper file to the repo root
    let helpers_path = s.repo_root().join("helpers.py");
    std::fs::write(&helpers_path, "CONSTANT = 42\n").unwrap();

    s.reindex().await;
    let id = s.asset_id_by_name("my_asset").await;
    let hash_before = s.definition_hash(id).await;
    let codebase_hash_before = s.codebase_hash().await;

    // Change the helper file (simulates editing a utility module)
    std::fs::write(&helpers_path, "CONSTANT = 99\n").unwrap();

    // Reindex — codebase_hash should change, which changes definition_hash
    s.reindex().await;
    let hash_after = s.definition_hash(id).await;
    let codebase_hash_after = s.codebase_hash().await;

    assert_ne!(codebase_hash_before, codebase_hash_after, "codebase_hash should change when any .py file changes");
    assert_ne!(hash_before, hash_after, "definition_hash should change when a helper module changes");
}

#[tokio::test]
async fn test_w5_helper_change_forces_rematerialization() {
    // Full lifecycle: materialize → change helper → reindex → materialize again.
    // Second materialization should NOT be a cache hit.
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;

    let helpers_path = s.repo_root().join("helpers.py");
    std::fs::write(&helpers_path, "MULTIPLIER = 1\n").unwrap();

    s.reindex().await;
    let id = s.asset_id_by_name("my_asset").await;

    // First materialization
    s.refresh(id).await;
    s.assert_fresh(id).await;
    let run_hash_1 = s.latest_run_hash(id).await.unwrap();

    // Change the helper
    std::fs::write(&helpers_path, "MULTIPLIER = 10\n").unwrap();

    // Reindex picks up the new codebase hash → new definition hash
    s.reindex().await;

    // Second materialization should produce a different run_hash
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    s.refresh(id).await;
    let run_hash_2 = s.latest_run_hash(id).await.unwrap();

    assert_ne!(run_hash_1, run_hash_2, "run_hash should change after helper module edit");
}

#[tokio::test]
async fn test_w5_unrelated_file_change_invalidates_all_assets() {
    // Changing ANY .py file should invalidate ALL assets (whole-codebase hash).
    let s = Scenario::new(DynamicMockPythonBridge::with_dependency_pair()).await;

    // Write an unrelated file that no asset imports
    let unrelated = s.repo_root().join("unrelated_utils.py");
    std::fs::write(&unrelated, "# nothing useful\n").unwrap();

    s.reindex().await;
    let fruit_id = s.asset_id_by_name("fruit").await;
    let upper_id = s.asset_id_by_name("uppercased").await;
    let fruit_hash_1 = s.definition_hash(fruit_id).await;
    let upper_hash_1 = s.definition_hash(upper_id).await;

    // Change the unrelated file
    std::fs::write(&unrelated, "# something changed\nX = 1\n").unwrap();

    s.reindex().await;
    let fruit_hash_2 = s.definition_hash(fruit_id).await;
    let upper_hash_2 = s.definition_hash(upper_id).await;

    assert_ne!(fruit_hash_1, fruit_hash_2, "fruit definition_hash should change");
    assert_ne!(upper_hash_1, upper_hash_2, "uppercased definition_hash should change");
}

#[tokio::test]
async fn test_w5_helper_change_cascades_through_dependency_chain() {
    // Full pipeline: fruit → uppercased. Change a helper file.
    // Both should get new definition_hashes, and refreshing the leaf
    // should produce new materializations (not cache hits).
    let s = Scenario::new(DynamicMockPythonBridge::with_dependency_pair()).await;

    let helpers_path = s.repo_root().join("helpers.py");
    std::fs::write(&helpers_path, "VERSION = 1\n").unwrap();

    s.reindex().await;
    let fruit_id = s.asset_id_by_name("fruit").await;
    let upper_id = s.asset_id_by_name("uppercased").await;

    // Materialize the full chain
    barca_server::enqueue_refresh_request(&s.state, upper_id).await.unwrap();
    s.run_worker_until_complete(upper_id).await;
    s.assert_fresh(fruit_id).await;
    s.assert_fresh(upper_id).await;

    let fruit_run_hash_1 = s.latest_run_hash(fruit_id).await.unwrap();
    let upper_run_hash_1 = s.latest_run_hash(upper_id).await.unwrap();

    // Change the helper and reindex
    std::fs::write(&helpers_path, "VERSION = 2\n").unwrap();
    s.reindex().await;

    // Re-materialize the chain
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    s.refresh(fruit_id).await;
    barca_server::enqueue_refresh_request(&s.state, upper_id).await.unwrap();
    s.run_worker_until_complete(upper_id).await;

    let fruit_run_hash_2 = s.latest_run_hash(fruit_id).await.unwrap();
    let upper_run_hash_2 = s.latest_run_hash(upper_id).await.unwrap();

    assert_ne!(fruit_run_hash_1, fruit_run_hash_2, "fruit should get new run_hash after helper change");
    assert_ne!(upper_run_hash_1, upper_run_hash_2, "uppercased should get new run_hash after helper change");
}

#[tokio::test]
async fn test_w5_no_change_means_cache_hit() {
    // Sanity check: reindex without any file changes should NOT invalidate.
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;

    let helpers_path = s.repo_root().join("helpers.py");
    std::fs::write(&helpers_path, "STABLE = true\n").unwrap();

    s.reindex().await;
    let id = s.asset_id_by_name("my_asset").await;
    let hash_1 = s.definition_hash(id).await;

    // Reindex again without changing anything
    s.reindex().await;
    let hash_2 = s.definition_hash(id).await;

    assert_eq!(hash_1, hash_2, "definition_hash should be stable when no files change");
}

#[tokio::test]
async fn test_w5_adding_new_py_file_invalidates() {
    // Adding a brand new .py file (not imported by anyone) still changes codebase_hash.
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;
    s.reindex().await;
    let id = s.asset_id_by_name("my_asset").await;
    let hash_before = s.definition_hash(id).await;

    // Add a new file
    std::fs::write(s.repo_root().join("new_module.py"), "print('hello')\n").unwrap();

    s.reindex().await;
    let hash_after = s.definition_hash(id).await;

    assert_ne!(hash_before, hash_after, "adding a new .py file should invalidate definition_hash");
}

#[tokio::test]
async fn test_w5_deleting_py_file_invalidates() {
    // Removing a .py file changes the codebase_hash.
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;

    let extra = s.repo_root().join("extra.py");
    std::fs::write(&extra, "X = 1\n").unwrap();

    s.reindex().await;
    let id = s.asset_id_by_name("my_asset").await;
    let hash_before = s.definition_hash(id).await;

    // Delete the file
    std::fs::remove_file(&extra).unwrap();

    s.reindex().await;
    let hash_after = s.definition_hash(id).await;

    assert_ne!(hash_before, hash_after, "deleting a .py file should invalidate definition_hash");
}

#[tokio::test]
async fn test_w5_version_history_preserved_across_helper_changes() {
    // After a helper change + re-materialization, both the old and new
    // materializations should exist in the version history.
    let s = Scenario::new(DynamicMockPythonBridge::with_one_asset()).await;

    let helpers_path = s.repo_root().join("helpers.py");
    std::fs::write(&helpers_path, "V = 1\n").unwrap();

    s.reindex().await;
    let id = s.asset_id_by_name("my_asset").await;
    s.refresh(id).await;
    s.assert_job_count(id, 1).await;

    // Change helper, reindex, rematerialize
    std::fs::write(&helpers_path, "V = 2\n").unwrap();
    s.reindex().await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    s.refresh(id).await;

    // Should now have 2 materializations in history
    s.assert_job_count(id, 2).await;

    // Both should be successful
    let store = s.state.store.lock().await;
    let jobs = store.list_materializations_for_asset(id, 100).await.unwrap();
    let successes: Vec<_> = jobs.iter().filter(|j| j.status == "success").collect();
    assert_eq!(successes.len(), 2, "both materializations should be successful");

    // They should have different definition_ids (different definition hashes)
    assert_ne!(
        successes[0].definition_id, successes[1].definition_id,
        "materializations should belong to different definitions"
    );
}
