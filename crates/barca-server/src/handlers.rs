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
    let files = state.config.files.clone();
    let python = state.config.python.clone();
    let lookup = name.clone();
    // stats() returns AssetNotFound (-> 404) for unknown names.
    let (summaries, stats) = tokio::task::spawn_blocking(move || {
        let summaries = commands::list_assets(&files, &python)?;
        let stats = commands::stats(&lookup, &files, &python)?;
        Ok::<_, barca_core::BarcaError>((summaries, stats))
    })
    .await??;

    let summary = summaries.into_iter().find(|s| {
        s.id == name || s.id.ends_with(&format!(":{name}")) || s.id.ends_with(&name)
    });

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
        if let Some(mut r) = st.runs.get_mut(&h) {
            r.status = RunStatus::Running;
        }

        let files = st.config.files.clone();
        let python = st.config.python.clone();
        let tgt = target.clone();
        let res = tokio::task::spawn_blocking(move || {
            commands::get(tgt.as_deref(), &files, &python, false, true)
        })
        .await;

        if let Some(mut r) = st.runs.get_mut(&h) {
            r.finished_at = Some(now_ts());
            match res {
                Ok(Ok(result)) => {
                    r.status = RunStatus::Complete;
                    r.result = Some(result);
                }
                Ok(Err(e)) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(e.to_string());
                }
                Err(e) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(format!("background task failed: {e}"));
                }
            }
        }
    });

    handle
}
