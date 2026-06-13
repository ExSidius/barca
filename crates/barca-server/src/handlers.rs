//! Endpoint handlers. Each handler that needs core work delegates to the
//! synchronous `barca_core::commands::*` functions via `spawn_blocking`, because
//! those functions build their own current-thread Tokio runtime internally.

use crate::error::ApiError;
use crate::state::{AppState, RunChannel, RunState, RunStatus, now_ts};
use axum::Json;
use axum::extract::{Path, State};
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};
use barca_core::RunEvent;
use barca_core::commands;
use barca_core::db;
use futures::stream::{self, Stream, StreamExt};
use serde_json::{Value, json};
use std::convert::Infallible;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;

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
    let handle = start(state, None, false);
    Json(json!({ "run_id": handle }))
}

/// `POST /run/{target}` — trigger a task run; returns a polling handle.
pub async fn run_target(State(state): State<AppState>, Path(target): Path<String>) -> Json<Value> {
    let handle = start(state, Some(target), true);
    Json(json!({ "run_id": handle }))
}

/// `POST /get/{target}` — trigger a target-scoped get; returns a polling handle.
pub async fn get_target(State(state): State<AppState>, Path(target): Path<String>) -> Json<Value> {
    let handle = start(state, Some(target), false);
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

/// `GET /events/{run_id}` — Server-Sent Events stream of a run's live events
/// (run lifecycle, logs, step completion). Replays the backlog first so a client
/// that connects a beat after the run starts still sees everything, then streams
/// live. Each SSE message is a JSON-encoded [`RunEvent`].
pub async fn events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let channel: RunChannel = state
        .events
        .get(&run_id)
        .map(|c| c.clone())
        .ok_or_else(|| ApiError::NotFound(format!("run '{run_id}' not found")))?;

    let (backlog, rx) = channel.snapshot_and_subscribe();

    let backlog_stream = stream::iter(backlog);
    let live_stream = BroadcastStream::new(rx).filter_map(|r| async move { r.ok() });

    let stream = backlog_stream.chain(live_stream).map(|ev: RunEvent| {
        Ok(Event::default()
            .json_data(&ev)
            .unwrap_or_else(|_| Event::default()))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// `GET /logs/{run_id}` — persisted stdout lines for a run (durable history).
///
/// Accepts the server-side polling handle and resolves it to the DB run id
/// (which `commands::execute` generates and surfaces in the completed result);
/// also accepts a raw DB run id directly.
pub async fn logs(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Map a polling handle to its DB run id if we know it; otherwise treat the
    // path param as a DB run id.
    let db_run_id = state
        .runs
        .get(&run_id)
        .and_then(|r| r.result.as_ref().map(|res| res.run_id.clone()))
        .unwrap_or(run_id);

    let entries = tokio::task::spawn_blocking(move || -> Result<_, barca_core::BarcaError> {
        let db_path = db::ensure_db_dir()?;
        // Ensure the schema exists — /logs may be hit before any run, since the
        // server inits the DB lazily on first execution.
        db::init_db_sync(&db_path)?;
        db::get_logs_sync(&db_path, &db_run_id)
    })
    .await??;

    Ok(Json(json!({ "logs": entries })))
}

/// Insert a `Pending` run, spawn the background execution task, and return the
/// server-side handle. Live events (run lifecycle, logs, step completion) are
/// emitted on the run's [`RunChannel`] as execution progresses; the real DB run
/// id is surfaced in the completed payload.
///
/// `is_task` selects `run` (tasks, always re-execute) vs `get` (assets, cache-aware).
fn start(state: AppState, target: Option<String>, is_task: bool) -> String {
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
    let channel = RunChannel::new();
    state.events.insert(handle.clone(), channel.clone());

    let st = state.clone();
    let h = handle.clone();
    tokio::spawn(async move {
        // Serialize runs so only one pipeline executes at a time, preventing
        // concurrent writes to the shared metadata.db.
        let _guard = st.run_mutex.lock().await;

        if let Some(mut r) = st.runs.get_mut(&h) {
            r.status = RunStatus::Running;
        }
        channel.emit(RunEvent::RunStarted { run_id: h.clone() });

        let files = st.config.files.clone();
        let python = st.config.python.clone();
        let tgt = target.clone();

        // Core streams RunEvents over an unbounded channel (sync send, crosses
        // the spawn_blocking + nested-runtime boundary). A drain task on this
        // runtime forwards them onto the run's broadcast/backlog channel.
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<RunEvent>();
        let drain_ch = channel.clone();
        let drain = tokio::spawn(async move {
            while let Some(ev) = event_rx.recv().await {
                drain_ch.emit(ev);
            }
        });

        // NOTE: spawn_blocking tasks cannot be aborted — they run on OS threads
        // Tokio cannot interrupt. If the timeout fires, the blocking thread keeps
        // running until it finishes naturally; `_guard` is held until this whole
        // block completes, so no concurrent run can race on the DB.
        let join_handle = tokio::task::spawn_blocking(move || {
            if is_task {
                commands::run_streaming(
                    tgt.as_deref().unwrap_or(""),
                    &files,
                    &python,
                    None,
                    true,
                    Some(event_tx),
                )
            } else {
                commands::get_streaming(
                    tgt.as_deref(),
                    &files,
                    &python,
                    false,
                    true,
                    Some(event_tx),
                )
            }
        });
        let res = tokio::time::timeout(RUN_TIMEOUT, join_handle).await;

        // On normal completion the blocking task has returned, dropping its event
        // sender — await the drain so trailing logs land before RunFinished. On
        // timeout the task is orphaned (sender still alive), so don't await.
        match &res {
            Ok(_) => {
                drain.await.ok();
            }
            Err(_) => {
                drain.abort();
            }
        }
        let outcome = match res {
            Ok(inner) => Ok(inner),
            Err(_elapsed) => Err(()),
        };

        let ok = if let Some(mut r) = st.runs.get_mut(&h) {
            r.finished_at = Some(now_ts());
            match outcome {
                Ok(Ok(Ok(result))) => {
                    r.status = RunStatus::Complete;
                    r.result = Some(result);
                    true
                }
                Ok(Ok(Err(e))) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(e.to_string());
                    false
                }
                Ok(Err(e)) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(format!("background task failed: {e}"));
                    false
                }
                Err(_) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(format!("run timed out after {}s", RUN_TIMEOUT.as_secs()));
                    false
                }
            }
        } else {
            false
        };
        channel.emit(RunEvent::RunFinished {
            run_id: h.clone(),
            ok,
        });
        // _guard is dropped here — after the result has been recorded.
    });

    handle
}

/// Evict completed/failed runs older than `max_age` from the in-memory runs map.
/// Intended to be spawned as a background task from `serve_async`.
pub async fn evict_finished_runs(state: AppState, interval: Duration, max_age: Duration) {
    loop {
        tokio::time::sleep(interval).await;
        let cutoff = now_ts() - max_age.as_secs_f64();
        state.runs.retain(|handle, run| {
            let keep = match run.status {
                RunStatus::Complete | RunStatus::Failed => {
                    // Keep if it finished recently (or hasn't finished yet somehow).
                    run.finished_at.map_or(true, |t| t > cutoff)
                }
                _ => true,
            };
            if !keep {
                // Drop the live event channel too; subscribers' streams end.
                state.events.remove(handle);
            }
            keep
        });
    }
}
