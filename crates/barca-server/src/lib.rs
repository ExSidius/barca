pub mod display;
pub mod python_bridge;
pub mod server;
pub mod snapshot;
pub mod store;
pub mod templates;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{anyhow, Context};
use barca_core::hashing::{compute_codebase_hash, compute_definition_hash, relative_path, repo_child, sha256_hex, slugify, DefinitionHashPayload, PROTOCOL_VERSION};
use barca_core::models::{ArtifactMetadata, AssetDetail, IndexedAsset, InspectedAsset};
use tokio::sync::{broadcast, Mutex, Notify};
use tracing::{error, info};

use crate::{python_bridge::PythonBridge, snapshot::SnapshotManager, store::MetadataStore};

/// A log entry emitted during job lifecycle, streamed to the UI in real time.
#[derive(Clone, Debug)]
pub struct JobLogEntry {
    pub asset_id: i64,
    pub job_id: i64,
    pub level: JobLogLevel,
    pub message: String,
}

#[derive(Clone, Debug)]
pub enum JobLogLevel {
    Info,
    Warn,
    Error,
    Output,
}

impl JobLogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
            Self::Output => "output",
        }
    }
}

pub fn emit_log(tx: &broadcast::Sender<JobLogEntry>, asset_id: i64, job_id: i64, level: JobLogLevel, message: impl Into<String>) {
    let _ = tx.send(JobLogEntry {
        asset_id,
        job_id,
        level,
        message: message.into(),
    });
}

/// Cache of definition-hash verifications so we don't re-inspect for every
/// partition job of the same asset.  Key = (module_path, function_name),
/// value = verified definition_hash.
type DefinitionCache = Arc<Mutex<HashMap<(String, String), String>>>;

#[derive(Clone)]
pub struct AppState {
    pub repo_root: PathBuf,
    pub store: Arc<Mutex<MetadataStore>>,
    pub job_queue_notify: Arc<Notify>,
    pub job_completion_tx: broadcast::Sender<i64>,
    pub job_log_tx: broadcast::Sender<JobLogEntry>,
    /// Fired whenever the global asset list changes (reindex, reset) so all
    /// connected main-page streams can re-render `#main-content`.
    pub state_tx: broadcast::Sender<()>,
    pub python: Arc<dyn PythonBridge>,
    pub definition_cache: DefinitionCache,
    /// Current merkle hash of the entire Python codebase.
    pub current_codebase_hash: Arc<Mutex<String>>,
    /// Manages frozen copies of the Python source tree for execution.
    pub snapshot_manager: Arc<SnapshotManager>,
    /// Path to the current active snapshot (set after reindex).
    pub current_snapshot_path: Arc<Mutex<Option<PathBuf>>>,
    /// Maximum number of concurrent Python worker subprocesses.
    /// Set to 1 for single-process (sequential) execution.
    pub max_concurrent_jobs: usize,
}

impl AppState {
    pub fn new(repo_root: PathBuf, store: MetadataStore, python: Arc<dyn PythonBridge>) -> Self {
        let (job_completion_tx, _) = broadcast::channel(16384);
        let (job_log_tx, _) = broadcast::channel(4096);
        let (state_tx, _) = broadcast::channel(16);
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self {
            repo_root: repo_root.clone(),
            store: Arc::new(Mutex::new(store)),
            job_queue_notify: Arc::new(Notify::new()),
            job_completion_tx,
            job_log_tx,
            state_tx,
            snapshot_manager: Arc::new(SnapshotManager::new(&repo_root)),
            current_snapshot_path: Arc::new(Mutex::new(None)),
            python,
            definition_cache: Arc::new(Mutex::new(HashMap::new())),
            current_codebase_hash: Arc::new(Mutex::new(String::new())),
            max_concurrent_jobs: cpus,
        }
    }

    pub fn with_max_concurrent_jobs(mut self, n: usize) -> Self {
        self.max_concurrent_jobs = n;
        self
    }
}

pub async fn run_log_persister(state: AppState) {
    let mut rx = state.job_log_tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(entry) => {
                if entry.job_id == 0 {
                    continue;
                }
                let store = state.store.lock().await;
                if let Err(e) = store.insert_job_log(entry.job_id, entry.asset_id, entry.level.as_str(), &entry.message).await {
                    error!(job_id = entry.job_id, error = %e, "failed to persist job log");
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "log persister lagged, some log entries were dropped");
            }
            Err(broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }
}

/// Resolve asset inputs: try the `asset_inputs` table first, fall back to
/// parsing the decorator metadata JSON (the source of truth). Persists any
/// newly resolved inputs back to the table for next time.
async fn resolve_asset_inputs(store: &store::MetadataStore, detail: &AssetDetail, repo_root: &Path) -> anyhow::Result<Vec<barca_core::models::AssetInput>> {
    let mut inputs = store.get_asset_inputs(detail.asset.definition_id).await?;

    if !inputs.is_empty() {
        return Ok(inputs);
    }

    // Fall back to decorator metadata
    let meta: serde_json::Value = match serde_json::from_str(&detail.asset.decorator_metadata_json) {
        Ok(v) => v,
        Err(_) => return Ok(inputs),
    };
    let inputs_obj = match meta.get("inputs").and_then(|v| v.as_object()) {
        Some(obj) => obj,
        None => return Ok(inputs),
    };

    for (param_name, ref_value) in inputs_obj {
        if let Some(abs_ref) = ref_value.as_str() {
            let canonical_ref = if let Some(colon_pos) = abs_ref.rfind(':') {
                let abs_path = &abs_ref[..colon_pos];
                let func_name = &abs_ref[colon_pos + 1..];
                let rel = relative_path(repo_root, Path::new(abs_path));
                format!("{rel}:{func_name}")
            } else {
                abs_ref.to_string()
            };
            let upstream_id = store.asset_id_by_logical_name(&canonical_ref).await?;
            inputs.push(barca_core::models::AssetInput {
                parameter_name: param_name.clone(),
                upstream_asset_ref: canonical_ref,
                upstream_asset_id: upstream_id,
            });
        }
    }

    if !inputs.is_empty() {
        store.upsert_asset_inputs(detail.asset.definition_id, &inputs).await?;
    }

    Ok(inputs)
}

/// Extract partition values from decorator metadata. Returns a list of
/// partition key objects, e.g. [{"ticker": "AAPL"}, {"ticker": "MSFT"}, ...].
/// Returns empty vec for non-partitioned assets.
fn resolve_partition_values(detail: &AssetDetail) -> Vec<serde_json::Value> {
    let meta: serde_json::Value = match serde_json::from_str(&detail.asset.decorator_metadata_json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let partitions_obj = match meta.get("partitions").and_then(|v| v.as_object()) {
        Some(obj) => obj,
        None => return Vec::new(),
    };

    // For now, support single-dimension inline partitions.
    // Each dimension has {"kind": "inline", "values_json": "[...]"}.
    // With one dimension, each value becomes {"dim_name": value}.
    let mut result = Vec::new();
    for (dim_name, spec) in partitions_obj {
        let spec_obj = match spec.as_object() {
            Some(o) => o,
            None => continue,
        };
        if spec_obj.get("kind").and_then(|v| v.as_str()) != Some("inline") {
            continue;
        }
        let values_json = match spec_obj.get("values_json").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => continue,
        };
        let values: Vec<serde_json::Value> = match serde_json::from_str(values_json) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for val in values {
            let mut key = serde_json::Map::new();
            key.insert(dim_name.clone(), val);
            result.push(serde_json::Value::Object(key));
        }
    }
    result
}

pub async fn reindex(state: &AppState) -> anyhow::Result<()> {
    // Clear the definition cache — code may have changed
    {
        let mut cache = state.definition_cache.lock().await;
        cache.clear();
    }
    // Compute codebase-level merkle hash (all .py files + uv.lock)
    let codebase_hash = compute_codebase_hash(&state.repo_root)?;
    info!(codebase_hash = %codebase_hash, "computed codebase hash");
    {
        let mut current = state.current_codebase_hash.lock().await;
        *current = codebase_hash.clone();
    }

    // Create a frozen snapshot of the codebase for workers to execute against
    let snapshot_path = state.snapshot_manager.ensure_snapshot(&state.repo_root, &codebase_hash)?;
    {
        let mut current = state.current_snapshot_path.lock().await;
        *current = Some(snapshot_path);
    }

    // Clean up old snapshots
    state.snapshot_manager.cleanup(&[&codebase_hash])?;

    let inspected = state.python.inspect_modules(&[]).await?;
    let mut seen = std::collections::HashSet::new();

    // First pass: upsert all assets and collect input declarations
    let mut assets_with_inputs: Vec<(String, Vec<barca_core::models::AssetInput>)> = Vec::new();

    for inspected_asset in inspected {
        let (indexed, inputs) = build_indexed_asset(&state.repo_root, inspected_asset, &codebase_hash)?;
        if !seen.insert(indexed.continuity_key.clone()) {
            return Err(anyhow!("duplicate continuity key detected: {}", indexed.continuity_key));
        }
        if !inputs.is_empty() {
            assets_with_inputs.push((indexed.continuity_key.clone(), inputs));
        }
        let store = state.store.lock().await;
        store.upsert_indexed_asset(&indexed).await?;
        drop(store);
    }

    // Second pass: resolve input upstream_asset_ids and persist
    let store = state.store.lock().await;
    for (continuity_key, mut inputs) in assets_with_inputs {
        let asset_id = store
            .asset_id_by_logical_name(&continuity_key)
            .await?
            .ok_or_else(|| anyhow!("asset {} not found after upsert", continuity_key))?;
        let detail = store.asset_detail(asset_id).await?;

        for input in &mut inputs {
            let upstream_id = store
                .asset_id_by_logical_name(&input.upstream_asset_ref)
                .await?
                .ok_or_else(|| anyhow!("input '{}' on asset '{}' references unknown asset '{}'", input.parameter_name, continuity_key, input.upstream_asset_ref,))?;
            input.upstream_asset_id = Some(upstream_id);
        }

        store.upsert_asset_inputs(detail.asset.definition_id, &inputs).await?;
    }
    drop(store);

    Ok(())
}

pub fn build_indexed_asset(repo_root: &Path, inspected: InspectedAsset, codebase_hash: &str) -> anyhow::Result<(IndexedAsset, Vec<barca_core::models::AssetInput>)> {
    if inspected.kind != "asset" {
        return Err(anyhow!("unsupported node kind: {}", inspected.kind));
    }

    let file_path = PathBuf::from(&inspected.file_path);
    let relative_file = relative_path(repo_root, &file_path);
    let explicit_name = inspected.decorator_metadata.get("name").and_then(|value| value.as_str()).map(ToOwned::to_owned);
    let continuity_key = explicit_name.clone().unwrap_or_else(|| format!("{relative_file}:{}", inspected.function_name));
    let logical_name = continuity_key.clone();
    let filename = file_path.file_name().and_then(|name| name.to_str()).unwrap_or("asset.py");
    let asset_slug = slugify(&[relative_file.as_str(), filename, inspected.function_name.as_str()]);
    let serializer_kind = inspected.decorator_metadata.get("serializer").and_then(|value| value.as_str()).unwrap_or("json").to_string();
    let decorator_json = serde_json::to_string(&inspected.decorator_metadata)?;
    let definition_hash = compute_definition_hash(&DefinitionHashPayload {
        codebase_hash,
        function_source: &inspected.function_source,
        decorator_metadata: &inspected.decorator_metadata,
        serializer_kind: &serializer_kind,
        python_version: &inspected.python_version,
        protocol_version: PROTOCOL_VERSION,
    })?;

    // Extract inputs from decorator metadata and relativize absolute paths
    let mut inputs = Vec::new();
    if let Some(inputs_map) = inspected.decorator_metadata.get("inputs") {
        if let Some(obj) = inputs_map.as_object() {
            for (param_name, ref_value) in obj {
                if let Some(abs_ref) = ref_value.as_str() {
                    // Refs come as "{abs_file_path}:{function_name}" from the decorator.
                    // Relativize: split on last ":", relativize the path portion.
                    let canonical_ref = if let Some(colon_pos) = abs_ref.rfind(':') {
                        let abs_path = &abs_ref[..colon_pos];
                        let func_name = &abs_ref[colon_pos + 1..];
                        let rel = relative_path(repo_root, Path::new(abs_path));
                        format!("{rel}:{func_name}")
                    } else {
                        abs_ref.to_string()
                    };
                    inputs.push(barca_core::models::AssetInput {
                        parameter_name: param_name.clone(),
                        upstream_asset_ref: canonical_ref,
                        upstream_asset_id: None,
                    });
                }
            }
        }
    }

    let has_inputs = !inputs.is_empty();
    let run_hash = if has_inputs {
        // run_hash computed at materialization time for assets with inputs
        String::new()
    } else {
        definition_hash.clone()
    };

    Ok((
        IndexedAsset {
            asset_id: 0,
            logical_name,
            continuity_key,
            module_path: inspected.module_path,
            file_path: relative_file,
            function_name: inspected.function_name,
            asset_slug,
            definition_id: 0,
            definition_hash,
            run_hash,
            source_text: inspected.function_source,
            module_source_text: inspected.module_source,
            decorator_metadata_json: decorator_json,
            return_type: inspected.return_type,
            serializer_kind,
            python_version: inspected.python_version,
            codebase_hash: codebase_hash.to_string(),
        },
        inputs,
    ))
}

/// How many jobs to claim from the queue in one batch.
const CLAIM_BATCH_SIZE: usize = 256;

pub async fn run_refresh_queue_worker(state: AppState) {
    let max_concurrent = state.max_concurrent_jobs;
    info!("refresh queue worker started (max_concurrent={})", max_concurrent);

    let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrent));
    let mut in_flight = tokio::task::JoinSet::new();

    loop {
        // Batch-claim queued jobs (up to CLAIM_BATCH_SIZE at once)
        let claimed = {
            let store = state.store.lock().await;
            match store.claim_queued_materializations(CLAIM_BATCH_SIZE).await {
                Ok(jobs) => jobs,
                Err(error) => {
                    error!(error = %error, "refresh queue worker failed to claim jobs");
                    Vec::new()
                }
            }
        };

        // Group partition jobs by asset_id for batch execution
        let mut partition_groups: HashMap<i64, Vec<barca_core::models::MaterializationRecord>> = HashMap::new();
        let mut standalone_jobs: Vec<barca_core::models::MaterializationRecord> = Vec::new();

        for job in claimed {
            if job.partition_key_json.is_some() {
                partition_groups.entry(job.asset_id).or_default().push(job);
            } else {
                standalone_jobs.push(job);
            }
        }

        // Dispatch partition groups as batches (one pre-fetch, N parallel workers)
        for (_asset_id, group) in partition_groups {
            let job_state = state.clone();
            let sem = semaphore.clone();
            in_flight.spawn(async move {
                execute_partition_batch(&job_state, group, sem).await;
            });
        }

        // Dispatch standalone jobs individually (existing path)
        for job in standalone_jobs {
            let job_state = state.clone();
            let sem = semaphore.clone();
            in_flight.spawn(async move {
                let _permit = sem.acquire().await.expect("semaphore closed");
                execute_refresh_job(&job_state, job).await;
            });
        }

        if in_flight.is_empty() {
            // Nothing in-flight, nothing queued — wait for a notification
            state.job_queue_notify.notified().await;
        } else {
            // Wait for any in-flight job to finish, OR a new notification
            tokio::select! {
                Some(result) = in_flight.join_next() => {
                    if let Err(e) = result {
                        error!(error = %e, "job task panicked");
                    }
                    // A job finished — wake up to check for re-queued downstream jobs
                    state.job_queue_notify.notify_one();
                }
                _ = state.job_queue_notify.notified() => {
                    // New job was enqueued — loop back to claim it
                }
            }
        }
    }
}

pub fn enqueue_refresh_request(state: &AppState, asset_id: i64) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<AssetDetail>> + Send + '_>> {
    Box::pin(enqueue_refresh_request_inner(state, asset_id))
}

async fn enqueue_refresh_request_inner(state: &AppState, asset_id: i64) -> anyhow::Result<AssetDetail> {
    let detail = {
        let store = state.store.lock().await;
        store.asset_detail(asset_id).await?
    };
    info!(
        asset_id = detail.asset.asset_id,
        asset = %detail.asset.logical_name,
        "refresh request received"
    );
    emit_log(
        &state.job_log_tx,
        asset_id,
        0,
        JobLogLevel::Info,
        format!("Refresh request received for {}", detail.asset.function_name),
    );

    let current_inspected = state
        .python
        .inspect_modules(std::slice::from_ref(&detail.asset.module_path))
        .await?
        .into_iter()
        .find(|asset| asset.function_name == detail.asset.function_name)
        .ok_or_else(|| anyhow!("asset {} could not be re-inspected", detail.asset.logical_name))?;
    let codebase_hash = state.current_codebase_hash.lock().await.clone();
    let (current_indexed, _) = build_indexed_asset(&state.repo_root, current_inspected, &codebase_hash)?;

    if current_indexed.definition_hash != detail.asset.definition_hash {
        let msg = format!("Definition changed since indexing for {}. Reindex first.", detail.asset.logical_name);
        emit_log(&state.job_log_tx, asset_id, 0, JobLogLevel::Error, &msg);
        return Err(anyhow!(msg));
    }

    // For assets without inputs, check for existing fresh materialization
    let has_inputs = !detail.asset.run_hash.is_empty() && detail.asset.run_hash != detail.asset.definition_hash;
    let skip_run_hash_check = has_inputs || detail.asset.run_hash.is_empty();

    let store = state.store.lock().await;
    if !skip_run_hash_check {
        if let Some(existing) = store.successful_materialization_for_run(asset_id, &detail.asset.run_hash).await? {
            info!(
                asset_id = detail.asset.asset_id,
                job_id = existing.materialization_id,
                "resolved from existing successful materialization",
            );
            emit_log(
                &state.job_log_tx,
                asset_id,
                existing.materialization_id,
                JobLogLevel::Info,
                "Resolved from existing successful materialization (already fresh)",
            );
            return store.asset_detail(asset_id).await;
        }

        if let Some(existing) = store.active_materialization_for_run(asset_id, &detail.asset.run_hash).await? {
            let msg = if existing.status == "running" {
                format!("Job {} already running — request joined", existing.materialization_id)
            } else {
                format!("Job {} already queued — request joined", existing.materialization_id)
            };
            info!(asset_id = detail.asset.asset_id, job_id = existing.materialization_id, "{msg}");
            emit_log(&state.job_log_tx, asset_id, existing.materialization_id, JobLogLevel::Info, &msg);
            return store.asset_detail(asset_id).await;
        }
    }

    // For assets with inputs, check if there's already an active job for this asset
    if skip_run_hash_check {
        if let Some(existing) = store.active_materialization_for_asset(asset_id).await? {
            let msg = format!("Job {} already {} — request joined", existing.materialization_id, existing.status);
            info!(asset_id, job_id = existing.materialization_id, "{msg}");
            emit_log(&state.job_log_tx, asset_id, existing.materialization_id, JobLogLevel::Info, &msg);
            return store.asset_detail(asset_id).await;
        }
    }

    // Recursively enqueue upstream deps first (before the current asset)
    let asset_inputs = resolve_asset_inputs(&store, &detail, &state.repo_root).await?;
    drop(store);

    for input in &asset_inputs {
        let upstream_asset_id = input.upstream_asset_id.unwrap_or(-1);
        let store = state.store.lock().await;
        let has_successful = store.latest_successful_materialization(upstream_asset_id).await?.is_some();
        drop(store);

        if !has_successful {
            info!(
                asset_id,
                upstream_asset_id,
                param = %input.parameter_name,
                "enqueuing upstream dependency"
            );
            // Recursive call — enqueues the upstream (and its upstreams) first
            enqueue_refresh_request(state, upstream_asset_id).await?;
        }
    }

    // Resolve partition values from decorator metadata (if any)
    let partition_values = resolve_partition_values(&detail);

    // Now enqueue the current asset — one job per partition (or one job if no partitions)
    let store = state.store.lock().await;
    let run_hash = if has_inputs { detail.asset.definition_hash.clone() } else { detail.asset.run_hash.clone() };

    if partition_values.is_empty() {
        // Non-partitioned asset: single job
        let materialization_id = store.insert_queued_materialization(asset_id, detail.asset.definition_id, &run_hash, None).await?;
        info!(
            asset_id = detail.asset.asset_id,
            job_id = materialization_id,
            asset = %detail.asset.logical_name,
            "queued",
        );
        emit_log(&state.job_log_tx, asset_id, materialization_id, JobLogLevel::Info, format!("Job {} queued", materialization_id));
    } else {
        // Partitioned asset: batch-insert all partition jobs at once
        let partition_keys: Vec<String> = partition_values.iter().map(|pv| serde_json::to_string(pv).unwrap_or_default()).collect();
        let count = store.insert_queued_materializations_batch(asset_id, detail.asset.definition_id, &run_hash, &partition_keys).await?;
        info!(
            asset_id = detail.asset.asset_id,
            count,
            asset = %detail.asset.logical_name,
            "queued partitions (batch)",
        );
        emit_log(
            &state.job_log_tx,
            asset_id,
            0,
            JobLogLevel::Info,
            format!("Queued {} partition jobs for {}", count, detail.asset.logical_name),
        );
    }

    let queued_detail = store.asset_detail(asset_id).await?;
    drop(store);
    state.job_queue_notify.notify_one();
    Ok(queued_detail)
}

/// Execute a batch of partition jobs for the same asset in parallel.
/// Pre-fetches all shared data once (asset detail, definition verification,
/// upstream inputs), then dispatches N Python workers concurrently with
/// minimal lock contention.
async fn execute_partition_batch(state: &AppState, jobs: Vec<barca_core::models::MaterializationRecord>, semaphore: Arc<tokio::sync::Semaphore>) {
    let started_at = Instant::now();
    let aid = jobs[0].asset_id;
    let log = &state.job_log_tx;

    info!(asset_id = aid, count = jobs.len(), "partition batch started");

    // Helper: fail all jobs with a shared error message and return early.
    macro_rules! fail_all {
        ($jobs:expr, $name:expr, $msg:expr) => {{
            let msg: String = $msg;
            for job in $jobs {
                emit_log(log, aid, job.materialization_id, JobLogLevel::Error, &msg);
                let _ = fail_refresh_job(state, job, $name, started_at, &msg).await;
            }
            return;
        }};
    }

    // --- Phase 1: Shared setup (one lock acquisition per step) ---

    // 1. Fetch asset detail (ONE lock)
    let detail = {
        let store = state.store.lock().await;
        match store.asset_detail(aid).await {
            Ok(d) => d,
            Err(e) => fail_all!(&jobs, None, e.to_string()),
        }
    };
    let asset_name = detail.asset.logical_name.clone();

    // Verify all jobs match current definition
    for job in &jobs {
        if job.definition_id != detail.asset.definition_id {
            fail_all!(
                &jobs,
                Some(asset_name.as_str()),
                format!("definition changed since batch was queued for {}. Trigger a new refresh.", asset_name)
            );
        }
    }

    // 2. Verify definition (ONE cache check or ONE inspect call)
    let cache_key = (detail.asset.module_path.clone(), detail.asset.function_name.clone());
    let cached = {
        let cache = state.definition_cache.lock().await;
        cache.get(&cache_key).cloned()
    };
    match cached {
        Some(ref hash) if hash == &detail.asset.definition_hash => { /* already verified */ }
        Some(_) => {
            fail_all!(&jobs, Some(asset_name.as_str()), format!("definition changed since indexing for {}. Reindex first.", asset_name));
        }
        None => {
            let inspected = match state.python.inspect_modules(std::slice::from_ref(&detail.asset.module_path)).await {
                Ok(i) => i,
                Err(e) => fail_all!(&jobs, Some(asset_name.as_str()), e.to_string()),
            };
            let current_inspected = match inspected.into_iter().find(|a| a.function_name == detail.asset.function_name) {
                Some(i) => i,
                None => fail_all!(&jobs, Some(asset_name.as_str()), format!("asset {} could not be re-inspected", asset_name)),
            };
            let cb_hash = state.current_codebase_hash.lock().await.clone();
            let (current_indexed, _) = match build_indexed_asset(&state.repo_root, current_inspected, &cb_hash) {
                Ok(r) => r,
                Err(e) => fail_all!(&jobs, Some(asset_name.as_str()), e.to_string()),
            };
            {
                let mut cache = state.definition_cache.lock().await;
                cache.insert(cache_key, current_indexed.definition_hash.clone());
            }
            if current_indexed.definition_hash != detail.asset.definition_hash {
                fail_all!(&jobs, Some(asset_name.as_str()), format!("definition changed since indexing for {}. Reindex first.", asset_name));
            }
        }
    }

    // 3. Resolve inputs + load upstream artifacts (ONE lock for entire block)
    let mut base_input_kwargs: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    let mut upstream_mat_ids: Vec<i64> = Vec::new();
    let mut mat_inputs: Vec<barca_core::models::MaterializationInput> = Vec::new();
    let mut provenance_entries: Vec<serde_json::Value> = Vec::new();

    {
        let store = state.store.lock().await;
        let asset_inputs = match resolve_asset_inputs(&store, &detail, &state.repo_root).await {
            Ok(inputs) => inputs,
            Err(e) => {
                drop(store);
                fail_all!(&jobs, Some(asset_name.as_str()), e.to_string());
            }
        };

        for input in &asset_inputs {
            let upstream_asset_id = input.upstream_asset_id.unwrap_or(-1);
            let upstream_mat = match store.latest_successful_materialization(upstream_asset_id).await {
                Ok(Some(m)) => m,
                Ok(None) => {
                    info!(asset_id = aid, upstream_asset_id, "upstream not ready — re-queuing partition batch");
                    for job in &jobs {
                        emit_log(
                            log,
                            aid,
                            job.materialization_id,
                            JobLogLevel::Info,
                            format!("Upstream asset #{} not ready — re-queuing", upstream_asset_id),
                        );
                        let _ = store.requeue_materialization(job.materialization_id).await;
                    }
                    drop(store);
                    state.job_queue_notify.notify_one();
                    return;
                }
                Err(e) => {
                    drop(store);
                    fail_all!(&jobs, Some(asset_name.as_str()), e.to_string());
                }
            };

            let artifact_path = match upstream_mat.artifact_path.as_ref() {
                Some(p) => p.clone(),
                None => {
                    drop(store);
                    fail_all!(&jobs, Some(asset_name.as_str()), format!("upstream asset #{} has no artifact path", upstream_asset_id));
                }
            };
            let full_path = state.repo_root.join(&artifact_path);
            let value_bytes = match fs::read(&full_path) {
                Ok(b) => b,
                Err(e) => {
                    drop(store);
                    fail_all!(&jobs, Some(asset_name.as_str()), format!("failed to read upstream artifact at {}: {}", full_path.display(), e));
                }
            };
            let value: serde_json::Value = match serde_json::from_slice(&value_bytes) {
                Ok(v) => v,
                Err(e) => {
                    drop(store);
                    fail_all!(&jobs, Some(asset_name.as_str()), e.to_string());
                }
            };

            base_input_kwargs.insert(input.parameter_name.clone(), value);
            upstream_mat_ids.push(upstream_mat.materialization_id);
            mat_inputs.push(barca_core::models::MaterializationInput {
                parameter_name: input.parameter_name.clone(),
                upstream_materialization_id: upstream_mat.materialization_id,
                upstream_asset_id,
            });
            provenance_entries.push(serde_json::json!({
                "parameter_name": input.parameter_name,
                "asset_ref": input.upstream_asset_ref,
                "materialization_id": upstream_mat.materialization_id,
            }));
        }
    }

    upstream_mat_ids.sort();

    // 4. Compute run_hash per partition (pure computation, no lock)
    struct PartitionWork {
        job: barca_core::models::MaterializationRecord,
        run_hash: String,
    }
    let partition_work: Vec<PartitionWork> = jobs
        .into_iter()
        .map(|job| {
            let run_hash = barca_core::hashing::compute_run_hash(&detail.asset.definition_hash, &upstream_mat_ids, job.partition_key_json.as_deref());
            PartitionWork { job, run_hash }
        })
        .collect();

    // 5. Batch check cache + update run_hashes (ONE lock)
    let cached_mats;
    {
        let store = state.store.lock().await;
        let all_run_hashes: Vec<String> = partition_work.iter().map(|pw| pw.run_hash.clone()).collect();
        cached_mats = match store.batch_check_cached_materializations(aid, &all_run_hashes).await {
            Ok(c) => c,
            Err(e) => {
                drop(store);
                let msg = e.to_string();
                for pw in &partition_work {
                    emit_log(log, aid, pw.job.materialization_id, JobLogLevel::Error, &msg);
                    let _ = fail_refresh_job(state, &pw.job, Some(&asset_name), started_at, &msg).await;
                }
                return;
            }
        };
        // Update run_hashes for all jobs in the same lock
        for pw in &partition_work {
            if let Err(e) = store.update_materialization_run_hash(pw.job.materialization_id, &pw.run_hash).await {
                error!(job_id = pw.job.materialization_id, error = %e, "failed to update run_hash");
            }
        }
    }

    // 6. Separate cached vs needs-work
    let mut cached_successes: Vec<(i64, String, String, String, String)> = Vec::new();
    let mut needs_work: Vec<PartitionWork> = Vec::new();

    for pw in partition_work {
        if let Some(existing) = cached_mats.get(&pw.run_hash) {
            info!(
                asset_id = aid,
                job_id = pw.job.materialization_id,
                existing_job = existing.materialization_id,
                "reusing cached materialization (run_hash match)",
            );
            emit_log(log, aid, pw.job.materialization_id, JobLogLevel::Info, "Reusing cached materialization");
            cached_successes.push((
                pw.job.materialization_id,
                pw.run_hash,
                existing.artifact_path.clone().unwrap_or_default(),
                existing.artifact_format.clone().unwrap_or_else(|| "json".to_string()),
                existing.artifact_checksum.clone().unwrap_or_default(),
            ));
        } else {
            needs_work.push(pw);
        }
    }

    // Batch mark cached successes (ONE lock)
    if !cached_successes.is_empty() {
        let store = state.store.lock().await;
        if let Err(e) = store.batch_mark_materialization_success(&cached_successes).await {
            error!(asset_id = aid, error = %e, "failed to batch-mark cached successes");
        }
        if !mat_inputs.is_empty() {
            for &(jid, _, _, _, _) in &cached_successes {
                if let Err(e) = store.insert_materialization_inputs(jid, &mat_inputs).await {
                    error!(job_id = jid, error = %e, "failed to insert materialization inputs");
                }
            }
        }
        drop(store);
        let elapsed = started_at.elapsed().as_millis();
        for &(jid, _, _, _, _) in &cached_successes {
            emit_log(log, aid, jid, JobLogLevel::Info, format!("Job {} completed (cached) in {}ms", jid, elapsed));
        }
    }

    if needs_work.is_empty() {
        let _ = state.job_completion_tx.send(aid);
        return;
    }

    // --- Phase 2: Dispatch non-cached partitions in parallel ---

    type PartitionResult = Result<
        (i64, String, String, String, String),               // (job_id, run_hash, artifact_path, format, checksum)
        (barca_core::models::MaterializationRecord, String), // (job, error_msg)
    >;

    let mut handles = tokio::task::JoinSet::<PartitionResult>::new();

    for pw in needs_work {
        let sem = semaphore.clone();
        let state_clone = state.clone();
        let detail_clone = detail.clone();
        let base_kwargs = base_input_kwargs.clone();
        let provenance_clone = provenance_entries.clone();
        let run_hash = pw.run_hash;
        let job = pw.job;

        handles.spawn(async move {
            let _permit = sem.acquire().await.expect("semaphore closed");
            let jid = job.materialization_id;
            let log = &state_clone.job_log_tx;

            emit_log(log, aid, jid, JobLogLevel::Info, format!("Job {} started — executing {}", jid, detail_clone.asset.function_name));

            // Merge partition key into kwargs
            let mut input_kwargs = base_kwargs;
            if let Some(ref pk_json) = job.partition_key_json {
                if let Ok(pk) = serde_json::from_str::<serde_json::Value>(pk_json) {
                    if let Some(obj) = pk.as_object() {
                        for (k, v) in obj {
                            input_kwargs.insert(k.clone(), v.clone());
                        }
                    }
                }
            }

            let input_kwargs_json = if input_kwargs.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&serde_json::Value::Object(input_kwargs)).map_err(|e| (job.clone(), e.to_string()))?)
            };

            emit_log(log, aid, jid, JobLogLevel::Info, "Spawning Python worker...");

            let staging_dir = barca_core::hashing::repo_child(&state_clone.repo_root, PathBuf::from("tmp").join(format!("asset-{}-{}", aid, jid)));
            if staging_dir.exists() {
                fs::remove_dir_all(&staging_dir).ok();
            }
            fs::create_dir_all(&staging_dir).map_err(|e| (job.clone(), e.to_string()))?;

            let snap = state_clone.current_snapshot_path.lock().await.clone();
            let worker = state_clone
                .python
                .materialize_asset(
                    &detail_clone.asset.module_path,
                    &detail_clone.asset.function_name,
                    &staging_dir,
                    jid,
                    log.clone(),
                    aid,
                    input_kwargs_json.as_deref(),
                    snap.as_deref(),
                )
                .await
                .map_err(|e| (job.clone(), e.to_string()))?;

            // Build artifact path with partition subdirectory
            let mut artifact_base = PathBuf::from(".barcafiles").join(&detail_clone.asset.asset_slug).join(&detail_clone.asset.definition_hash);
            if let Some(ref pk_json) = job.partition_key_json {
                if let Ok(pk) = serde_json::from_str::<serde_json::Value>(pk_json) {
                    if let Some(obj) = pk.as_object() {
                        let mut parts: Vec<String> = obj
                            .iter()
                            .map(|(k, v)| {
                                let val = match v {
                                    serde_json::Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                format!("{k}={val}")
                            })
                            .collect();
                        parts.sort();
                        artifact_base = artifact_base.join("partitions").join(parts.join(","));
                    }
                }
            }
            let artifact_dir = barca_core::hashing::repo_child(&state_clone.repo_root, &artifact_base);
            fs::create_dir_all(&artifact_dir).map_err(|e| (job.clone(), e.to_string()))?;

            let value_path_str = worker.value_path.ok_or_else(|| (job.clone(), "worker did not return value path".to_string()))?;
            let value_path = PathBuf::from(value_path_str);
            let value_bytes = fs::read(&value_path).map_err(|e| (job.clone(), e.to_string()))?;
            let artifact_checksum = barca_core::hashing::sha256_hex(&value_bytes);
            let final_value_path = artifact_dir.join("value.json");
            fs::copy(&value_path, &final_value_path).map_err(|e| (job.clone(), e.to_string()))?;
            let artifact_path = barca_core::hashing::relative_path(&state_clone.repo_root, &final_value_path);
            let artifact_format = worker.artifact_format.unwrap_or_else(|| "json".to_string());

            // Write metadata files (filesystem only, no lock)
            let _ = fs::write(artifact_dir.join("code.txt"), &detail_clone.asset.source_text);
            let metadata = ArtifactMetadata {
                asset_name: &detail_clone.asset.logical_name,
                module_path: &detail_clone.asset.module_path,
                file_path: &detail_clone.asset.file_path,
                function_name: &detail_clone.asset.function_name,
                definition_hash: &detail_clone.asset.definition_hash,
                run_hash: &run_hash,
                serializer_kind: &detail_clone.asset.serializer_kind,
                python_version: &detail_clone.asset.python_version,
                return_type: detail_clone.asset.return_type.as_deref(),
                inputs: provenance_clone,
                barca_version: PROTOCOL_VERSION,
            };
            let _ = fs::write(artifact_dir.join("metadata.json"), serde_json::to_vec_pretty(&metadata).unwrap_or_default());

            let elapsed = started_at.elapsed().as_millis();
            emit_log(log, aid, jid, JobLogLevel::Info, format!("Job {} completed successfully in {}ms", jid, elapsed));

            Ok((jid, run_hash, artifact_path, artifact_format, artifact_checksum))
        });
    }

    // 8. Collect results
    let mut successes: Vec<(i64, String, String, String, String)> = Vec::new();

    while let Some(result) = handles.join_next().await {
        match result {
            Ok(Ok(success_data)) => {
                successes.push(success_data);
            }
            Ok(Err((job, error_msg))) => {
                emit_log(log, aid, job.materialization_id, JobLogLevel::Error, &error_msg);
                let _ = fail_refresh_job(state, &job, Some(&asset_name), started_at, &error_msg).await;
            }
            Err(e) => {
                error!(asset_id = aid, error = %e, "partition task panicked");
            }
        }
    }

    // 9. Batch mark successes (ONE lock)
    if !successes.is_empty() {
        let store = state.store.lock().await;
        if let Err(e) = store.batch_mark_materialization_success(&successes).await {
            error!(asset_id = aid, error = %e, "failed to batch-mark successes");
        }
        if !mat_inputs.is_empty() {
            for &(jid, _, _, _, _) in &successes {
                if let Err(e) = store.insert_materialization_inputs(jid, &mat_inputs).await {
                    error!(job_id = jid, error = %e, "failed to insert materialization inputs");
                }
            }
        }
    }

    // 10. Signal completion
    let _ = state.job_completion_tx.send(aid);
}

async fn execute_refresh_job(state: &AppState, job: barca_core::models::MaterializationRecord) {
    let started_at = Instant::now();
    let jid = job.materialization_id;
    let aid = job.asset_id;
    let log = &state.job_log_tx;

    let detail = {
        let store = state.store.lock().await;
        match store.asset_detail(aid).await {
            Ok(detail) => detail,
            Err(error) => {
                let error_message = error.to_string();
                emit_log(log, aid, jid, JobLogLevel::Error, &error_message);
                if let Err(mark_error) = fail_refresh_job(state, &job, None, started_at, &error_message).await {
                    error!(job_id = jid, error = %mark_error, "failed to persist failure");
                }
                return;
            }
        }
    };

    info!(
        asset_id = aid, job_id = jid,
        asset = %detail.asset.logical_name,
        "started",
    );
    emit_log(log, aid, jid, JobLogLevel::Info, format!("Job {} started — executing {}", jid, detail.asset.function_name));

    let result: anyhow::Result<()> = async {
        if detail.asset.definition_id != job.definition_id || detail.asset.run_hash != job.run_hash {
            return Err(anyhow!("definition changed since this job was queued for {}. Trigger a new refresh.", detail.asset.logical_name));
        }

        emit_log(log, aid, jid, JobLogLevel::Info, "Verifying definition...");

        // Check the definition cache first — all partitions of the same asset
        // share the same definition, so we only need to inspect once.
        let cache_key = (detail.asset.module_path.clone(), detail.asset.function_name.clone());
        let cached = {
            let cache = state.definition_cache.lock().await;
            cache.get(&cache_key).cloned()
        };

        match cached {
            Some(ref hash) if hash == &detail.asset.definition_hash => {
                // Already verified — skip the subprocess call
            }
            Some(_) => {
                return Err(anyhow!("definition changed since indexing for {}. Reindex first.", detail.asset.logical_name));
            }
            None => {
                // First time seeing this asset — inspect and cache
                let current_inspected = state
                    .python
                    .inspect_modules(std::slice::from_ref(&detail.asset.module_path))
                    .await?
                    .into_iter()
                    .find(|asset| asset.function_name == detail.asset.function_name)
                    .ok_or_else(|| anyhow!("asset {} could not be re-inspected", detail.asset.logical_name))?;
                let cb_hash = state.current_codebase_hash.lock().await.clone();
                let (current_indexed, _) = build_indexed_asset(&state.repo_root, current_inspected, &cb_hash)?;

                let mut cache = state.definition_cache.lock().await;
                cache.insert(cache_key, current_indexed.definition_hash.clone());

                if current_indexed.definition_hash != detail.asset.definition_hash {
                    return Err(anyhow!("definition changed since indexing for {}. Reindex first.", detail.asset.logical_name));
                }
            }
        }

        // Resolve upstream inputs (table first, falls back to decorator metadata)
        let asset_inputs = {
            let store = state.store.lock().await;
            resolve_asset_inputs(&store, &detail, &state.repo_root).await?
        };

        let mut input_kwargs: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        let mut upstream_mat_ids: Vec<i64> = Vec::new();
        let mut mat_inputs: Vec<barca_core::models::MaterializationInput> = Vec::new();
        let mut provenance_entries: Vec<serde_json::Value> = Vec::new();

        for input in &asset_inputs {
            let upstream_asset_id = input.upstream_asset_id.unwrap_or(-1);
            emit_log(
                log,
                aid,
                jid,
                JobLogLevel::Info,
                format!("Resolving upstream input '{}' (asset #{})", input.parameter_name, upstream_asset_id),
            );

            // Check upstream has a successful materialization — if not, re-queue this job
            let upstream_mat = {
                let store = state.store.lock().await;
                store.latest_successful_materialization(upstream_asset_id).await?
            };
            let Some(upstream_mat) = upstream_mat else {
                info!(asset_id = aid, job_id = jid, upstream_asset_id, "upstream not ready — re-queuing job");
                emit_log(log, aid, jid, JobLogLevel::Info, format!("Upstream asset #{} not ready — re-queuing", upstream_asset_id));
                let store = state.store.lock().await;
                store.requeue_materialization(jid).await?;
                // Notify worker to pick up next job (which should be the upstream)
                drop(store);
                state.job_queue_notify.notify_one();
                return Ok(());
            };

            // Load the upstream artifact value
            let artifact_path = upstream_mat
                .artifact_path
                .as_ref()
                .ok_or_else(|| anyhow!("upstream asset #{} has no artifact path", upstream_asset_id))?;
            let full_path = state.repo_root.join(artifact_path);
            let value_bytes = fs::read(&full_path).with_context(|| format!("failed to read upstream artifact at {}", full_path.display()))?;
            let value: serde_json::Value = serde_json::from_slice(&value_bytes)?;

            input_kwargs.insert(input.parameter_name.clone(), value);
            upstream_mat_ids.push(upstream_mat.materialization_id);
            mat_inputs.push(barca_core::models::MaterializationInput {
                parameter_name: input.parameter_name.clone(),
                upstream_materialization_id: upstream_mat.materialization_id,
                upstream_asset_id,
            });
            provenance_entries.push(serde_json::json!({
                "parameter_name": input.parameter_name,
                "asset_ref": input.upstream_asset_ref,
                "materialization_id": upstream_mat.materialization_id,
            }));
        }

        // Compute run_hash (includes upstream mat IDs + partition key)
        let has_inputs_or_partition = !asset_inputs.is_empty() || job.partition_key_json.is_some();
        let run_hash = if !has_inputs_or_partition {
            detail.asset.definition_hash.clone()
        } else {
            upstream_mat_ids.sort();
            barca_core::hashing::compute_run_hash(&detail.asset.definition_hash, &upstream_mat_ids, job.partition_key_json.as_deref())
        };

        // Check for existing successful materialization with this run_hash
        {
            let store = state.store.lock().await;
            if let Some(existing) = store.successful_materialization_for_run(aid, &run_hash).await? {
                info!(
                    asset_id = aid,
                    job_id = jid,
                    existing_job = existing.materialization_id,
                    "reusing cached materialization (run_hash match)",
                );
                emit_log(log, aid, jid, JobLogLevel::Info, "Reusing cached materialization");
                // Update this job's run_hash and mark success with same artifact
                store.update_materialization_run_hash(jid, &run_hash).await?;
                store
                    .mark_materialization_success(
                        jid,
                        existing.artifact_path.as_deref().unwrap_or(""),
                        existing.artifact_format.as_deref().unwrap_or("json"),
                        existing.artifact_checksum.as_deref().unwrap_or(""),
                    )
                    .await?;
                if !mat_inputs.is_empty() {
                    store.insert_materialization_inputs(jid, &mat_inputs).await?;
                }
                let elapsed = started_at.elapsed().as_millis();
                emit_log(log, aid, jid, JobLogLevel::Info, format!("Job {} completed (cached) in {}ms", jid, elapsed));
                return Ok(());
            }
            // Update the run_hash on the queued job
            store.update_materialization_run_hash(jid, &run_hash).await?;
        }

        // Merge partition key into kwargs if present
        if let Some(ref pk_json) = job.partition_key_json {
            if let Ok(pk) = serde_json::from_str::<serde_json::Value>(pk_json) {
                if let Some(obj) = pk.as_object() {
                    for (k, v) in obj {
                        input_kwargs.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        let input_kwargs_json = if input_kwargs.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&serde_json::Value::Object(input_kwargs))?)
        };

        emit_log(log, aid, jid, JobLogLevel::Info, "Spawning Python worker...");

        let staging_dir = repo_child(&state.repo_root, PathBuf::from("tmp").join(format!("asset-{}-{}", detail.asset.asset_id, job.materialization_id)));
        if staging_dir.exists() {
            fs::remove_dir_all(&staging_dir).ok();
        }
        fs::create_dir_all(&staging_dir)?;

        let snap = state.current_snapshot_path.lock().await.clone();
        let worker = state
            .python
            .materialize_asset(
                &detail.asset.module_path,
                &detail.asset.function_name,
                &staging_dir,
                jid,
                log.clone(),
                aid,
                input_kwargs_json.as_deref(),
                snap.as_deref(),
            )
            .await?;
        let mut artifact_base = PathBuf::from(".barcafiles").join(&detail.asset.asset_slug).join(&detail.asset.definition_hash);
        // For partitioned assets, nest under partitions/<key=value>/
        if let Some(ref pk_json) = job.partition_key_json {
            if let Ok(pk) = serde_json::from_str::<serde_json::Value>(pk_json) {
                if let Some(obj) = pk.as_object() {
                    let mut parts: Vec<String> = obj
                        .iter()
                        .map(|(k, v)| {
                            let val = match v {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            format!("{k}={val}")
                        })
                        .collect();
                    parts.sort();
                    artifact_base = artifact_base.join("partitions").join(parts.join(","));
                }
            }
        }
        let artifact_dir = repo_child(&state.repo_root, &artifact_base);
        fs::create_dir_all(&artifact_dir)?;

        let value_path = PathBuf::from(worker.value_path.context("worker did not return value path")?);
        let value_bytes = fs::read(&value_path)?;
        let artifact_checksum = sha256_hex(&value_bytes);
        let final_value_path = artifact_dir.join("value.json");
        fs::copy(&value_path, &final_value_path)?;
        let artifact_path = relative_path(&state.repo_root, &final_value_path);
        let artifact_format = worker.artifact_format.as_deref().unwrap_or("json");

        fs::write(artifact_dir.join("code.txt"), &detail.asset.source_text)?;
        let metadata = ArtifactMetadata {
            asset_name: &detail.asset.logical_name,
            module_path: &detail.asset.module_path,
            file_path: &detail.asset.file_path,
            function_name: &detail.asset.function_name,
            definition_hash: &detail.asset.definition_hash,
            run_hash: &run_hash,
            serializer_kind: &detail.asset.serializer_kind,
            python_version: &detail.asset.python_version,
            return_type: detail.asset.return_type.as_deref(),
            inputs: provenance_entries,
            barca_version: PROTOCOL_VERSION,
        };
        fs::write(artifact_dir.join("metadata.json"), serde_json::to_vec_pretty(&metadata)?)?;

        let store = state.store.lock().await;
        store.mark_materialization_success(job.materialization_id, &artifact_path, artifact_format, &artifact_checksum).await?;
        if !mat_inputs.is_empty() {
            store.insert_materialization_inputs(jid, &mat_inputs).await?;
        }
        let elapsed = started_at.elapsed().as_millis();
        info!(asset_id = aid, job_id = jid, duration_ms = elapsed, "completed",);
        emit_log(log, aid, jid, JobLogLevel::Info, format!("Job {} completed successfully in {}ms", jid, elapsed));
        Ok(())
    }
    .await;

    if let Err(error) = result {
        let error_message = error.to_string();
        emit_log(log, aid, jid, JobLogLevel::Error, &error_message);
        if let Err(mark_error) = fail_refresh_job(state, &job, Some(&detail.asset.logical_name), started_at, &error_message).await {
            error!(
                asset_id = aid, job_id = jid,
                error = %mark_error,
                "failed to persist failure",
            );
        }
    } else {
        let _ = state.job_completion_tx.send(aid);
    }
}

async fn fail_refresh_job(state: &AppState, job: &barca_core::models::MaterializationRecord, asset_name: Option<&str>, started_at: Instant, error_message: &str) -> anyhow::Result<()> {
    let jid = job.materialization_id;
    let store = state.store.lock().await;
    store.mark_materialization_failed(job.materialization_id, error_message).await?;
    error!(
        asset_id = job.asset_id,
        asset = %asset_name.unwrap_or("unknown"),
        job_id = jid,
        duration_ms = started_at.elapsed().as_millis(),
        error = %error_message,
        "failed",
    );
    drop(store);
    let _ = state.job_completion_tx.send(job.asset_id);
    Ok(())
}

/// Directories that `barca reset` can remove.
const RESET_TARGETS: &[(&str, &str)] = &[(".barca", "db"), (".barcafiles", "artifacts"), ("tmp", "tmp")];

/// Remove generated files and caches from the repo root.
///
/// When `db`, `artifacts`, and `tmp` are all false, everything is removed.
/// Returns a human-readable summary of what was done.
pub fn reset(repo_root: &Path, db: bool, artifacts: bool, tmp: bool) -> anyhow::Result<String> {
    let all = !db && !artifacts && !tmp;
    let flags: &[bool] = &[db, artifacts, tmp];

    let mut lines: Vec<String> = Vec::new();

    for (i, &(dir, _label)) in RESET_TARGETS.iter().enumerate() {
        if !all && !flags[i] {
            continue;
        }
        let path = repo_root.join(dir);
        if path.exists() {
            fs::remove_dir_all(&path).with_context(|| format!("failed to remove {dir}/"))?;
            lines.push(format!("removed {dir}/"));
        }
    }

    if lines.is_empty() {
        lines.push("nothing to reset".into());
    }

    Ok(lines.join("\n") + "\n")
}
