pub mod config;
pub mod python_bridge;
pub mod server;
pub mod store;
pub mod templates;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
    time::Instant,
};

use anyhow::{anyhow, Context};
use barca_core::hashing::{
    compute_definition_hash, optional_file_hash, relative_path, repo_child, sha256_hex, slugify,
    DefinitionHashPayload, PROTOCOL_VERSION,
};
use barca_core::models::{ArtifactMetadata, AssetDetail, IndexedAsset, InspectedAsset};
use tokio::sync::{broadcast, Mutex, Notify};
use tracing::{error, info};

use crate::{config::BarcaConfig, python_bridge::PythonBridge, store::MetadataStore};

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

fn emit_log(
    tx: &broadcast::Sender<JobLogEntry>,
    asset_id: i64,
    job_id: i64,
    level: JobLogLevel,
    message: impl Into<String>,
) {
    let _ = tx.send(JobLogEntry {
        asset_id,
        job_id,
        level,
        message: message.into(),
    });
}

#[derive(Clone)]
pub struct AppState {
    pub repo_root: PathBuf,
    pub config: BarcaConfig,
    pub store: Arc<Mutex<MetadataStore>>,
    pub job_queue_notify: Arc<Notify>,
    pub job_completion_tx: broadcast::Sender<i64>,
    pub job_log_tx: broadcast::Sender<JobLogEntry>,
    pub python: PythonBridge,
}

impl AppState {
    pub fn new(
        repo_root: PathBuf,
        config: BarcaConfig,
        store: MetadataStore,
        python: PythonBridge,
    ) -> Self {
        let (job_completion_tx, _) = broadcast::channel(16);
        let (job_log_tx, _) = broadcast::channel(256);
        Self {
            repo_root: repo_root.clone(),
            config,
            store: Arc::new(Mutex::new(store)),
            job_queue_notify: Arc::new(Notify::new()),
            job_completion_tx,
            job_log_tx,
            python,
        }
    }
}

pub async fn reindex(state: &AppState) -> anyhow::Result<()> {
    let inspected = state
        .python
        .inspect_modules(&state.config.python.modules)
        .await?;
    let mut seen = std::collections::HashSet::new();

    let uv_lock_hash = optional_file_hash(&state.repo_root.join("uv.lock"));
    for inspected_asset in inspected {
        let indexed = build_indexed_asset(&state.repo_root, inspected_asset, uv_lock_hash.clone())?;
        if !seen.insert(indexed.continuity_key.clone()) {
            return Err(anyhow!(
                "duplicate continuity key detected: {}",
                indexed.continuity_key
            ));
        }
        let store = state.store.lock().await;
        store.upsert_indexed_asset(&indexed).await?;
        drop(store);
    }
    Ok(())
}

pub fn build_indexed_asset(
    repo_root: &Path,
    inspected: InspectedAsset,
    uv_lock_hash: Option<String>,
) -> anyhow::Result<IndexedAsset> {
    if inspected.kind != "asset" {
        return Err(anyhow!("unsupported node kind: {}", inspected.kind));
    }

    let file_path = PathBuf::from(&inspected.file_path);
    let relative_file = relative_path(repo_root, &file_path);
    let explicit_name = inspected
        .decorator_metadata
        .get("name")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);
    let continuity_key = explicit_name
        .clone()
        .unwrap_or_else(|| format!("{relative_file}:{}", inspected.function_name));
    let logical_name = continuity_key.clone();
    let filename = file_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("asset.py");
    let asset_slug = slugify(&[
        relative_file.as_str(),
        filename,
        inspected.function_name.as_str(),
    ]);
    let serializer_kind = inspected
        .decorator_metadata
        .get("serializer")
        .and_then(|value| value.as_str())
        .unwrap_or("json")
        .to_string();
    let decorator_json = serde_json::to_string(&inspected.decorator_metadata)?;
    let definition_hash = compute_definition_hash(&DefinitionHashPayload {
        module_source: &inspected.module_source,
        function_source: &inspected.function_source,
        decorator_metadata: &inspected.decorator_metadata,
        serializer_kind: &serializer_kind,
        python_version: &inspected.python_version,
        uv_lock_hash: uv_lock_hash.as_deref(),
        protocol_version: PROTOCOL_VERSION,
    })?;

    Ok(IndexedAsset {
        asset_id: 0,
        logical_name,
        continuity_key,
        module_path: inspected.module_path,
        file_path: relative_file,
        function_name: inspected.function_name,
        asset_slug,
        definition_id: 0,
        definition_hash: definition_hash.clone(),
        run_hash: definition_hash,
        source_text: inspected.function_source,
        module_source_text: inspected.module_source,
        decorator_metadata_json: decorator_json,
        return_type: inspected.return_type,
        serializer_kind,
        python_version: inspected.python_version,
        uv_lock_hash,
    })
}

pub async fn run_refresh_queue_worker(state: AppState) {
    info!("refresh queue worker started");

    loop {
        let notified = state.job_queue_notify.notified();
        let next_job = {
            let store = state.store.lock().await;
            store.claim_next_queued_materialization().await
        };

        match next_job {
            Ok(Some(job)) => {
                execute_refresh_job(&state, job).await;
            }
            Ok(None) => {
                notified.await;
            }
            Err(error) => {
                error!(error = %error, "refresh queue worker failed to claim the next job");
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
}

pub async fn enqueue_refresh_request(
    state: &AppState,
    asset_id: i64,
) -> anyhow::Result<AssetDetail> {
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
        format!(
            "Refresh request received for {}",
            detail.asset.function_name
        ),
    );

    let current_inspected = state
        .python
        .inspect_modules(std::slice::from_ref(&detail.asset.module_path))
        .await?
        .into_iter()
        .find(|asset| asset.function_name == detail.asset.function_name)
        .ok_or_else(|| {
            anyhow!(
                "asset {} could not be re-inspected",
                detail.asset.logical_name
            )
        })?;
    let current_indexed = build_indexed_asset(
        &state.repo_root,
        current_inspected,
        optional_file_hash(&state.repo_root.join("uv.lock")),
    )?;

    if current_indexed.definition_hash != detail.asset.definition_hash {
        let msg = format!(
            "Definition changed since indexing for {}. Reindex first.",
            detail.asset.logical_name
        );
        emit_log(&state.job_log_tx, asset_id, 0, JobLogLevel::Error, &msg);
        return Err(anyhow!(msg));
    }

    let store = state.store.lock().await;
    if let Some(existing) = store
        .successful_materialization_for_run(asset_id, &detail.asset.run_hash)
        .await?
    {
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

    if let Some(existing) = store
        .active_materialization_for_run(asset_id, &detail.asset.run_hash)
        .await?
    {
        let msg = if existing.status == "running" {
            format!(
                "Job {} already running — request joined",
                existing.materialization_id
            )
        } else {
            format!(
                "Job {} already queued — request joined",
                existing.materialization_id
            )
        };
        info!(
            asset_id = detail.asset.asset_id,
            job_id = existing.materialization_id,
            "{msg}"
        );
        emit_log(
            &state.job_log_tx,
            asset_id,
            existing.materialization_id,
            JobLogLevel::Info,
            &msg,
        );
        return store.asset_detail(asset_id).await;
    }

    let materialization_id = store
        .insert_queued_materialization(asset_id, detail.asset.definition_id, &detail.asset.run_hash)
        .await?;
    let queued_detail = store.asset_detail(asset_id).await?;
    drop(store);
    info!(
        asset_id = detail.asset.asset_id,
        job_id = materialization_id,
        asset = %detail.asset.logical_name,
        "queued",
    );
    emit_log(
        &state.job_log_tx,
        asset_id,
        materialization_id,
        JobLogLevel::Info,
        format!("Job {} queued", materialization_id),
    );
    state.job_queue_notify.notify_one();
    Ok(queued_detail)
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
                if let Err(mark_error) =
                    fail_refresh_job(state, &job, None, started_at, &error_message).await
                {
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
    emit_log(
        log,
        aid,
        jid,
        JobLogLevel::Info,
        format!(
            "Job {} started — executing {}",
            jid, detail.asset.function_name
        ),
    );

    let result: anyhow::Result<()> = async {
        if detail.asset.definition_id != job.definition_id || detail.asset.run_hash != job.run_hash
        {
            return Err(anyhow!(
                "definition changed since this job was queued for {}. Trigger a new refresh.",
                detail.asset.logical_name
            ));
        }

        emit_log(log, aid, jid, JobLogLevel::Info, "Verifying definition...");

        let current_inspected = state
            .python
            .inspect_modules(std::slice::from_ref(&detail.asset.module_path))
            .await?
            .into_iter()
            .find(|asset| asset.function_name == detail.asset.function_name)
            .ok_or_else(|| {
                anyhow!(
                    "asset {} could not be re-inspected",
                    detail.asset.logical_name
                )
            })?;
        let current_indexed = build_indexed_asset(
            &state.repo_root,
            current_inspected,
            optional_file_hash(&state.repo_root.join("uv.lock")),
        )?;

        if current_indexed.definition_hash != detail.asset.definition_hash {
            return Err(anyhow!(
                "definition changed since indexing for {}. Reindex first.",
                detail.asset.logical_name
            ));
        }

        emit_log(
            log,
            aid,
            jid,
            JobLogLevel::Info,
            "Spawning Python worker...",
        );

        let staging_dir = repo_child(
            &state.repo_root,
            PathBuf::from("tmp").join(format!(
                "asset-{}-{}",
                detail.asset.asset_id, job.materialization_id
            )),
        );
        if staging_dir.exists() {
            fs::remove_dir_all(&staging_dir).ok();
        }
        fs::create_dir_all(&staging_dir)?;

        let worker = state
            .python
            .materialize_asset(
                &detail.asset.module_path,
                &detail.asset.function_name,
                &staging_dir,
                jid,
                log.clone(),
                aid,
            )
            .await?;
        let artifact_dir = repo_child(
            &state.repo_root,
            PathBuf::from(".barcafiles")
                .join(&detail.asset.asset_slug)
                .join(&detail.asset.definition_hash),
        );
        fs::create_dir_all(&artifact_dir)?;

        let value_path = PathBuf::from(
            worker
                .value_path
                .context("worker did not return value path")?,
        );
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
            run_hash: &detail.asset.run_hash,
            serializer_kind: &detail.asset.serializer_kind,
            python_version: &detail.asset.python_version,
            return_type: detail.asset.return_type.as_deref(),
            inputs: Vec::new(),
            barca_version: PROTOCOL_VERSION,
        };
        fs::write(
            artifact_dir.join("metadata.json"),
            serde_json::to_vec_pretty(&metadata)?,
        )?;

        let store = state.store.lock().await;
        store
            .mark_materialization_success(
                job.materialization_id,
                &artifact_path,
                artifact_format,
                &artifact_checksum,
            )
            .await?;
        let elapsed = started_at.elapsed().as_millis();
        info!(
            asset_id = aid,
            job_id = jid,
            duration_ms = elapsed,
            "completed",
        );
        emit_log(
            log,
            aid,
            jid,
            JobLogLevel::Info,
            format!("Job {} completed successfully in {}ms", jid, elapsed),
        );
        Ok(())
    }
    .await;

    if let Err(error) = result {
        let error_message = error.to_string();
        emit_log(log, aid, jid, JobLogLevel::Error, &error_message);
        if let Err(mark_error) = fail_refresh_job(
            state,
            &job,
            Some(&detail.asset.logical_name),
            started_at,
            &error_message,
        )
        .await
        {
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

async fn fail_refresh_job(
    state: &AppState,
    job: &barca_core::models::MaterializationRecord,
    asset_name: Option<&str>,
    started_at: Instant,
    error_message: &str,
) -> anyhow::Result<()> {
    let jid = job.materialization_id;
    let store = state.store.lock().await;
    store
        .mark_materialization_failed(job.materialization_id, error_message)
        .await?;
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
const RESET_TARGETS: &[(&str, &str)] = &[
    (".barca", "db"),
    (".barcafiles", "artifacts"),
    ("tmp", "tmp"),
];

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
