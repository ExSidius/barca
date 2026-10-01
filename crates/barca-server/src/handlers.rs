//! Endpoint handlers. Core commands are `async fn`s that run directly on the
//! server's runtime — handlers simply `.await` them. Each run carries a
//! `CancellationToken` (a child of the server-wide shutdown token) so
//! `DELETE /run/{id}`, the run timeout, and Ctrl-C can all stop it mid-flight:
//! workers are terminated and the run is marked cancelled/failed.

use crate::error::ApiError;
use crate::state::{AppState, RunChannel, RunState, RunStatus, now_ts};
use axum::Json;
use axum::extract::{Path, State};
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};
use barca_core::commands::{self, GetResult};
use barca_core::{BarcaError, RunEvent, db};
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
    let result = commands::plan(&state.config.files, &state.config.python).await?;
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
    let result = commands::list_assets(&state.config.files, &state.config.python).await?;
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
        let result = commands::list_assets(&state.config.files, &state.config.python).await?;
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

    let stats = commands::stats(
        &state.config.resolved,
        &summary.id,
        &state.config.files,
        &state.config.python,
    )
    .await?;

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

/// `DELETE /run/{run_id}` — cancel an in-flight run. The run's workers are
/// terminated and its status transitions to `cancelled`; poll `/status/{id}`
/// to observe the transition. Cancelling a finished run is a no-op conflict.
pub async fn cancel_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let run = state
        .runs
        .get(&run_id)
        .ok_or_else(|| ApiError::NotFound(format!("run '{run_id}' not found")))?;
    match run.status {
        RunStatus::Pending | RunStatus::Running => {
            run.cancel.cancel();
            Ok(Json(json!({ "run_id": run_id, "status": "cancelling" })))
        }
        status => Err(ApiError::Conflict(format!(
            "run '{run_id}' already finished ({})",
            json!(status).as_str().unwrap_or("finished")
        ))),
    }
}

/// `GET /schedule` — list scheduled jobs with next fire time and last run status.
/// Reads the scheduler's published registry; volatile fields (next fire, live
/// status) are computed per request.
pub async fn schedule(State(state): State<AppState>) -> Json<Value> {
    use barca_core::CronExpr;
    use chrono::Local;

    let now = Local::now();
    let jobs = state.schedule.read().map(|g| g.clone()).unwrap_or_default();
    let items: Vec<Value> = jobs
        .into_iter()
        .map(|j| {
            let next_fire = CronExpr::parse(&j.cron)
                .ok()
                .and_then(|c| c.find_next_occurrence(&now, false).ok())
                .map(|t| t.timestamp());
            let last_status = j
                .last_handle
                .as_ref()
                .and_then(|h| state.runs.get(h).map(|r| r.status));
            json!({
                "id": j.id,
                "cron": j.cron,
                "kind": j.kind,
                "next_fire": next_fire,
                "last_fired": j.last_fired,
                "last_run": j.last_handle,
                "last_status": last_status,
            })
        })
        .collect();
    Json(json!(items))
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

    let cfg = &state.config.resolved;
    db::ensure_env_dirs(&cfg.env)?;
    // Ensure the schema exists — /logs may be hit before any run, since the
    // server inits the DB lazily on first execution.
    db::init_db(&cfg.db_path).await?;
    let entries = db::get_logs(&cfg.db_path, &db_run_id).await?;

    Ok(Json(json!({ "logs": entries })))
}

/// Which core command a background run executes.
enum RunKind {
    /// `commands::get` with an optional target (assets).
    Get(Option<String>),
    /// `commands::run` for a task target.
    Task(String),
}

/// Insert a `Pending` run, spawn the background execution task, and return the
/// server-side handle. The real DB run id is surfaced in the completed payload.
pub(crate) fn start_run(state: AppState, target: Option<String>) -> String {
    spawn_run(state, RunKind::Get(target))
}

/// Insert a `Pending` run for a task, spawn the background execution via
/// `commands::run`, and return the server-side handle.
pub(crate) fn start_run_task(state: AppState, target: String) -> String {
    spawn_run(state, RunKind::Task(target))
}

fn spawn_run(state: AppState, kind: RunKind) -> String {
    let handle = db::generate_run_id();
    // Child of the server-wide shutdown token: DELETE /run/{id} cancels just
    // this run; graceful shutdown cancels all of them.
    let cancel = state.shutdown.child_token();
    state.runs.insert(
        handle.clone(),
        RunState {
            handle: handle.clone(),
            status: RunStatus::Pending,
            result: None,
            error: None,
            started_at: now_ts(),
            finished_at: None,
            cancel: cancel.clone(),
        },
    );
    let channel = RunChannel::new();
    state.events.insert(handle.clone(), channel.clone());

    let st = state.clone();
    let h = handle.clone();
    tokio::spawn(async move {
        // Bound concurrency: acquire a run slot. Runs execute in parallel; the
        // shared metadata.db is kept safe by barca-core's process-wide DB lock.
        let _permit = st.run_slots.acquire().await.ok();

        // Cancelled while queued — never started, nothing to clean up.
        if cancel.is_cancelled() {
            if let Some(mut r) = st.runs.get_mut(&h) {
                r.status = RunStatus::Cancelled;
                r.error = Some("run cancelled".to_string());
                r.finished_at = Some(now_ts());
            }
            channel.emit(RunEvent::RunFinished {
                run_id: h.clone(),
                ok: false,
            });
            return;
        }

        if let Some(mut r) = st.runs.get_mut(&h) {
            r.status = RunStatus::Running;
        }
        channel.emit(RunEvent::RunStarted { run_id: h.clone() });

        // Core streams RunEvents over an unbounded channel (sync send from
        // inside the worker-pool loop). A drain task forwards them onto the
        // run's broadcast/backlog channel.
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<RunEvent>();
        let drain_ch = channel.clone();
        let drain = tokio::spawn(async move {
            while let Some(ev) = event_rx.recv().await {
                drain_ch.emit(ev);
            }
        });

        let files = st.config.files.clone();
        let python = st.config.python.clone();
        let cfg = st.config.resolved.clone();

        let mut timed_out = false;
        let outcome: Result<GetResult, BarcaError> = {
            let fut = async {
                match &kind {
                    RunKind::Get(target) => {
                        commands::get_streaming(
                            &cfg,
                            target.as_deref(),
                            &files,
                            &python,
                            false,
                            true,
                            cancel.clone(),
                            Some(event_tx),
                        )
                        .await
                    }
                    RunKind::Task(target) => {
                        commands::run_streaming(
                            &cfg,
                            target,
                            &files,
                            &python,
                            commands::CachePolicy::RefreshAll,
                            true,
                            cancel.clone(),
                            Some(event_tx),
                        )
                        .await
                    }
                }
            };
            tokio::pin!(fut);

            // On timeout, cancel the token and keep awaiting: the run observes the
            // cancellation, terminates its workers, persists partial results, and
            // returns — nothing is left running in the background.
            tokio::select! {
                res = &mut fut => res,
                _ = tokio::time::sleep(RUN_TIMEOUT) => {
                    // An operator cancel that is still unwinding when the deadline
                    // hits stays classified as cancelled, not as a timeout.
                    timed_out = !cancel.is_cancelled();
                    cancel.cancel();
                    fut.await
                }
            }
            // `fut` (and with it the event sender) is dropped here.
        };

        // The run has returned and its event sender is gone — await the drain
        // so trailing logs land before RunFinished.
        drain.await.ok();
        let ok = outcome.is_ok();

        if let Some(mut r) = st.runs.get_mut(&h) {
            r.finished_at = Some(now_ts());
            match outcome {
                Ok(result) => {
                    r.status = RunStatus::Complete;
                    r.result = Some(result);
                }
                Err(BarcaError::Cancelled) if timed_out => {
                    r.status = RunStatus::Failed;
                    r.error = Some(format!("run timed out after {}s", RUN_TIMEOUT.as_secs()));
                }
                Err(BarcaError::Cancelled) => {
                    r.status = RunStatus::Cancelled;
                    r.error = Some("run cancelled".to_string());
                }
                Err(e) => {
                    r.status = RunStatus::Failed;
                    r.error = Some(e.to_string());
                }
            }
        }
        channel.emit(RunEvent::RunFinished {
            run_id: h.clone(),
            ok,
        });
        // The run slot is released here, freeing capacity for a queued run.
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
                RunStatus::Complete | RunStatus::Failed | RunStatus::Cancelled => {
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
