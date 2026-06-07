//! Endpoint handlers. Each handler that needs core work delegates to the
//! synchronous `barca_core::commands::*` functions via `spawn_blocking`, because
//! those functions build their own current-thread Tokio runtime internally.

use crate::error::ApiError;
use crate::state::{AppState, RunState, RunStatus, now_ts};
use axum::Json;
use axum::extract::{Path, State};
use barca_core::commands;
use barca_core::db;
use serde_json::{Value, json};
use std::time::Duration;

/// Default timeout for a single run (10 minutes).
const RUN_TIMEOUT: Duration = Duration::from_secs(600);

/// `GET /health` — liveness + version. No core work.
pub async fn health() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// `GET /plan` — execution plan for the server's files (cache-aware).
pub async fn plan(State(state): State<AppState>) -> Result<Json<commands::PlanResult>, ApiError> {
    if let Some(cached) = state.cache.read().unwrap().plan.clone() {
        return Ok(Json(cached));
    }
    let files = state.config.files.clone();
    let python = state.config.python.clone();
    let result = tokio::task::spawn_blocking(move || commands::plan(&files, &python)).await??;
    state.cache.write().unwrap().plan = Some(result.clone());
    Ok(Json(result))
}

/// `GET /assets` — list every node with kind/freshness/inputs (cache-aware).
pub async fn assets(
    State(state): State<AppState>,
) -> Result<Json<Vec<commands::AssetSummary>>, ApiError> {
    if let Some(cached) = state.cache.read().unwrap().assets.clone() {
        return Ok(Json(cached));
    }
    let files = state.config.files.clone();
    let python = state.config.python.clone();
    let result =
        tokio::task::spawn_blocking(move || commands::list_assets(&files, &python)).await??;
    state.cache.write().unwrap().assets = Some(result.clone());
    Ok(Json(result))
}

/// `GET /assets/{name}` — summary joined with timing/cache stats for one asset.
pub async fn asset_detail(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Use cached assets if available, otherwise fetch and cache them.
    let summaries = if let Some(cached) = state.cache.read().unwrap().assets.clone() {
        cached
    } else {
        let files = state.config.files.clone();
        let python = state.config.python.clone();
        let result =
            tokio::task::spawn_blocking(move || commands::list_assets(&files, &python)).await??;
        state.cache.write().unwrap().assets = Some(result.clone());
        result
    };

    // Exact match first, then colon-prefixed match. No unbounded ends_with.
    let matches: Vec<_> = summaries
        .iter()
        .filter(|s| s.id == name || s.id.ends_with(&format!(":{name}")))
        .collect();

    let summary = match matches.len() {
        0 => return Err(ApiError::NotFound(format!("asset '{name}' not found"))),
        1 => matches[0].clone(),
        n => {
            let ids: Vec<_> = matches.iter().map(|s| s.id.as_str()).collect();
            return Err(ApiError::Conflict(format!(
                "'{name}' is ambiguous — matches {n} assets: {}",
                ids.join(", ")
            )));
        }
    };

    let files = state.config.files.clone();
    let python = state.config.python.clone();
    let resolved_id = summary.id.clone();
    let stats = tokio::task::spawn_blocking(move || commands::stats(&resolved_id, &files, &python))
        .await??;

    Ok(Json(json!({
        "asset": summary,
        "stats": stats,
    })))
}

/// `POST /run` — trigger a full run; returns a polling handle immediately.
pub async fn run(State(state): State<AppState>) -> Json<Value> {
    let handle = start_run(state, None);
    Json(json!({ "run_id": handle }))
}

/// `POST /run/{target}` — trigger a task run; returns a polling handle.
pub async fn run_target(State(state): State<AppState>, Path(target): Path<String>) -> Json<Value> {
    let handle = start_run_task(state, target);
    Json(json!({ "run_id": handle }))
}

/// `POST /get/{target}` — trigger a target-scoped get; returns a polling handle.
pub async fn get_target(State(state): State<AppState>, Path(target): Path<String>) -> Json<Value> {
    let handle = start_run(state, Some(target));
    Json(json!({ "run_id": handle }))
}

/// `GET /status/{run_id}` — poll an in-flight or finished run.
pub async fn status(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<RunState>, ApiError> {
    state
        .runs
        .get(&run_id)
        .map(|r| Json(r.clone()))
        .ok_or_else(|| ApiError::NotFound(format!("run '{run_id}' not found")))
}

/// Insert a `Pending` run, spawn the background execution task, and return the
/// server-side handle. The real DB run id is surfaced in the completed payload.
fn start_run(state: AppState, target: Option<String>) -> String {
    let handle = db::generate_run_id();
    state.runs.insert(
        handle.clone(),
        RunState {
            handle: handle.clone(),
            status: RunStatus::Pending,
            result: None,
            error: None,
            started_at: now_ts(),
            finished_at: None,
        },
    );

    let st = state.clone();
    let h = handle.clone();
    tokio::spawn(async move {
        // Serialize runs so only one pipeline executes at a time, preventing
        // concurrent writes to the shared metadata.db.
        let _guard = st.run_mutex.lock().await;

        if let Some(mut r) = st.runs.get_mut(&h) {
            r.status = RunStatus::Running;
        }

        let files = st.config.files.clone();
        let python = st.config.python.clone();
        let tgt = target.clone();
        let res = tokio::time::timeout(
            RUN_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                commands::get(tgt.as_deref(), &files, &python, false, true)
            }),
        )
        .await;

        if let Some(mut r) = st.runs.get_mut(&h) {
            r.finished_at = Some(now_ts());
            match res {
                Ok(Ok(Ok(result))) => {
                    r.status = RunStatus::Complete;
                    r.result = Some(result);
                }
                Ok(Ok(Err(e))) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(e.to_string());
                }
                Ok(Err(e)) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(format!("background task failed: {e}"));
                }
                Err(_) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(format!("run timed out after {}s", RUN_TIMEOUT.as_secs()));
                }
            }
        }
    });

    handle
}

/// Insert a `Pending` run for a task, spawn the background execution via
/// `commands::run`, and return the server-side handle.
fn start_run_task(state: AppState, target: String) -> String {
    let handle = db::generate_run_id();
    state.runs.insert(
        handle.clone(),
        RunState {
            handle: handle.clone(),
            status: RunStatus::Pending,
            result: None,
            error: None,
            started_at: now_ts(),
            finished_at: None,
        },
    );

    let st = state.clone();
    let h = handle.clone();
    tokio::spawn(async move {
        let _guard = st.run_mutex.lock().await;

        if let Some(mut r) = st.runs.get_mut(&h) {
            r.status = RunStatus::Running;
        }

        let files = st.config.files.clone();
        let python = st.config.python.clone();
        let tgt = target.clone();
        let res = tokio::time::timeout(
            RUN_TIMEOUT,
            tokio::task::spawn_blocking(move || commands::run(&tgt, &files, &python, None, true)),
        )
        .await;

        if let Some(mut r) = st.runs.get_mut(&h) {
            r.finished_at = Some(now_ts());
            match res {
                Ok(Ok(Ok(result))) => {
                    r.status = RunStatus::Complete;
                    r.result = Some(result);
                }
                Ok(Ok(Err(e))) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(e.to_string());
                }
                Ok(Err(e)) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(format!("background task failed: {e}"));
                }
                Err(_) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(format!("run timed out after {}s", RUN_TIMEOUT.as_secs()));
                }
            }
        }
    });

    handle
}

/// Evict completed/failed runs older than `max_age` from the in-memory runs map.
/// Intended to be spawned as a background task from `serve_async`.
pub async fn evict_finished_runs(state: AppState, interval: Duration, max_age: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        let cutoff = now_ts() - max_age.as_secs_f64();
        state.runs.retain(|_, run| {
            match run.status {
                RunStatus::Complete | RunStatus::Failed => {
                    // Keep if it finished recently (or hasn't finished yet somehow).
                    run.finished_at.map_or(true, |t| t > cutoff)
                }
                _ => true,
            }
        });
    }
}
