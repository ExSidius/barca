use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use barca_core::models::JobDetail;
use barca_server::config::load_config;
use barca_server::python_bridge::UvPythonBridge;
use barca_server::store::MetadataStore;
use barca_server::AppState;

use crate::display;

/// Load config from barca.toml or return a friendly error.
fn load_project_config(repo_root: &Path) -> anyhow::Result<barca_server::config::BarcaConfig> {
    load_config(&repo_root.join("barca.toml")).context("no barca.toml found — are you in a barca project directory?")
}

/// Create a full AppState and reindex. Called at the top of every command
/// (except reset) so the DAG is always up to date.
async fn init(repo_root: &Path) -> anyhow::Result<AppState> {
    let config = load_project_config(repo_root)?;
    let db_path = repo_root.join(".barca").join("metadata.db");
    let store = MetadataStore::open(&db_path).await?;
    let python = Arc::new(UvPythonBridge::new(repo_root.to_path_buf()));
    let state = AppState::new(repo_root.to_path_buf(), config, store, python);
    barca_server::reindex(&state).await?;
    Ok(state)
}

pub async fn serve() -> anyhow::Result<()> {
    let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
    let state = init(&repo_root).await?;

    {
        let store = state.store.lock().await;
        store.requeue_running_materializations().await?;
    }
    tracing::info!("refresh queue recovery complete");
    tokio::spawn(barca_server::run_refresh_queue_worker(state.clone()));
    tokio::spawn(barca_server::run_log_persister(state.clone()));

    let app = barca_server::server::router().with_state(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    tracing::info!("barca listening on http://127.0.0.1:3000");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.expect("failed to listen for ctrl-c");
            tracing::info!("shutting down");
        })
        .await?;
    Ok(())
}

pub async fn reindex_cmd() -> anyhow::Result<()> {
    let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
    let state = init(&repo_root).await?;
    let store = state.store.lock().await;
    let assets = store.list_assets().await?;
    println!("{}", display::assets_table(&assets));
    Ok(())
}

pub fn reset_cmd(db: bool, artifacts: bool, tmp: bool) -> anyhow::Result<()> {
    let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
    let output = barca_server::reset(&repo_root, db, artifacts, tmp)?;
    print!("{output}");
    Ok(())
}

pub async fn assets_list() -> anyhow::Result<()> {
    let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
    let state = init(&repo_root).await?;
    let store = state.store.lock().await;
    let assets = store.list_assets().await?;
    println!("{}", display::assets_table(&assets));
    Ok(())
}

pub async fn assets_show(id: i64) -> anyhow::Result<()> {
    let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
    let state = init(&repo_root).await?;
    let store = state.store.lock().await;
    let detail = store.asset_detail(id).await?;
    println!("{}", display::asset_detail(&detail));
    Ok(())
}

pub async fn assets_refresh(id: i64) -> anyhow::Result<()> {
    let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
    let state = init(&repo_root).await?;

    // Spawn the worker so it can process the queued job
    let worker_state = state.clone();
    let worker_handle = tokio::spawn(async move {
        barca_server::run_refresh_queue_worker(worker_state).await;
    });
    tokio::spawn(barca_server::run_log_persister(state.clone()));

    // Subscribe to completions before enqueueing
    let mut completion_rx = state.job_completion_tx.subscribe();

    let _detail = barca_server::enqueue_refresh_request(&state, id).await?;

    // Count how many queued/running jobs exist for this asset
    let pending_count = {
        let store = state.store.lock().await;
        let jobs = store.list_materializations_for_asset(id, 100).await?;
        jobs.iter().filter(|j| j.status == "queued" || j.status == "running").count()
    };

    if pending_count == 0 {
        println!("Asset #{} is already fresh.", id);
        worker_handle.abort();
        return Ok(());
    }

    // Wait for all pending jobs to complete
    println!("Waiting for materialization of asset #{} ({} job{})...", id, pending_count, if pending_count == 1 { "" } else { "s" });
    let mut completed = 0;
    loop {
        match completion_rx.recv().await {
            Ok(completed_id) if completed_id == id => {
                completed += 1;
                if completed >= pending_count {
                    break;
                }
            }
            Ok(_) => continue,
            Err(e) => anyhow::bail!("job completion channel error: {}", e),
        }
    }

    // Show final status
    let store = state.store.lock().await;
    let final_detail = store.asset_detail(id).await?;
    drop(store);
    println!("{}", display::asset_detail(&final_detail));

    worker_handle.abort();
    Ok(())
}

pub async fn jobs_list() -> anyhow::Result<()> {
    let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
    let state = init(&repo_root).await?;
    let store = state.store.lock().await;
    let pairs = store.list_recent_materializations(50).await?;
    let jobs: Vec<JobDetail> = pairs.into_iter().map(|(mat, asset)| JobDetail { job: mat, asset }).collect();
    println!("{}", display::jobs_table(&jobs));
    Ok(())
}

pub async fn jobs_show(id: i64) -> anyhow::Result<()> {
    let repo_root = std::env::current_dir().context("failed to resolve current dir")?;
    let state = init(&repo_root).await?;
    let store = state.store.lock().await;
    let (mat, asset) = store.get_materialization_with_asset(id).await?;
    let detail = JobDetail { job: mat, asset };
    println!("{}", display::job_detail(&detail));
    Ok(())
}
