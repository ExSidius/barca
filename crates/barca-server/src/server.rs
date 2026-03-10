use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::{
    extract::{Path as AxumPath, Query, State},
    middleware,
    response::{Html, IntoResponse, Sse},
    routing::{get, post},
    Json, Router,
};
use barca_core::models::{AssetDetail, AssetSummary, MaterializationRecord};
use datastar::consts::ElementPatchMode;
use datastar::prelude::PatchElements;
use time::{Month, OffsetDateTime};
use tokio::select;

use crate::templates;
use crate::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(index_page))
        .route("/reindex", post(reindex_action))
        .route("/assets/{asset_id}/materialize", post(materialize_action))
        .route("/assets/{asset_id}/panel", get(asset_panel))
        .route("/assets/{asset_id}/panel/stream", get(asset_panel_stream))
        // JSON API
        .route("/api/assets", get(api_list_assets))
        .route("/api/assets/{asset_id}", get(api_get_asset))
        .route(
            "/api/assets/{asset_id}/materialize",
            post(api_materialize_asset),
        )
        .route("/api/reindex", post(api_reindex))
        .layer(middleware::from_fn(request_logger))
}

async fn request_logger(req: axum::extract::Request, next: middleware::Next) -> impl IntoResponse {
    let method = req.method().clone();
    let uri = req.uri().clone();
    tracing::info!("{method} {uri}");
    next.run(req).await
}

#[derive(Debug, serde::Deserialize)]
pub struct IndexQuery {
    pub view: Option<String>,
}

async fn index_page(
    Query(query): Query<IndexQuery>,
    State(state): State<AppState>,
) -> axum::response::Result<Html<String>> {
    let view = query.view.as_deref().unwrap_or("assets");
    let store = state.store.lock().await;

    let body = match view {
        "jobs" => {
            let jobs = store
                .list_recent_materializations(50)
                .await
                .map_err(internal_error)?;
            drop(store);
            format!(
                r#"
                <div class="flex flex-1 gap-6">
                  {}
                  <div class="flex min-w-0 flex-1 flex-col gap-6">
                    {}
                    <div id="main-content" class="flex-1">
                      {}
                    </div>
                  </div>
                </div>
                "#,
                templates::sidebar("jobs"),
                templates::page_header("Jobs", "Recent materialization jobs across all assets."),
                templates::jobs_list(&jobs)
            )
        }
        _ => {
            let assets = store.list_assets().await.map_err(internal_error)?;
            drop(store);
            format!(
                r#"
                <div class="flex flex-1 gap-6">
                  {}
                  <div class="flex min-w-0 flex-1 flex-col gap-6">
                    {}
                    <div id="main-content" class="flex-1">
                      {}
                    </div>
                  </div>
                </div>
                "#,
                templates::sidebar("assets"),
                templates::page_header("Assets", "A minimal registry of assets, their latest runs, and the jobs you can trigger from here."),
                render_asset_list(&assets)
            )
        }
    };

    Ok(Html(templates::page(
        "Barca",
        &format!(
            r#"
            {}
            <main class="mx-auto max-w-6xl flex flex-1 px-6 py-8 sm:px-10">
              {}
            </main>
            "#,
            templates::theme_toggle(),
            body
        ),
    )))
}

async fn reindex_action(State(state): State<AppState>) -> impl IntoResponse {
    let stream = stream! {
        match crate::reindex(&state).await {
            Ok(()) => {
                let store = state.store.lock().await;
                let assets = store.list_assets().await.unwrap_or_default();
                let html = render_asset_list(&assets).replace('\n', "");
                let patch = PatchElements::new(html).selector("#main-content");
                yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
            }
            Err(error) => {
                let html = format!(
                    r#"<section id="asset-list" class="mt-8"><article class="rounded-[28px] border border-rose-200 bg-rose-50/70 px-6 py-6 shadow-card"><p class="text-2xl tracking-[-0.03em] text-zinc-950">Reindex failed</p><p class="mt-3 font-sans text-sm leading-6 text-rose-700">{}</p></article></section>"#,
                    templates::escape_html(&error.to_string())
                );
                let patch = PatchElements::new(html).selector("#main-content");
                yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
            }
        }
    };

    Sse::new(stream).into_response()
}

async fn materialize_action(
    AxumPath(asset_id): AxumPath<i64>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let stream = stream! {
        match crate::enqueue_refresh_request(&state, asset_id).await {
            Ok(detail) => {
                let html = render_asset_card_from_detail(&detail).replace('\n', "");
                let patch = PatchElements::new(html);
                yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
            }
            Err(_error) => {
                let store = state.store.lock().await;
                let html = match store.asset_detail(asset_id).await {
                    Ok(detail) => render_asset_card_from_detail(&detail),
                    Err(_) => {
                        let summary = store
                            .list_assets()
                            .await
                            .ok()
                            .and_then(|assets| assets.into_iter().find(|a| a.asset_id == asset_id));
                        match summary {
                            Some(s) => templates::asset_card(
                                asset_id,
                                &s.function_name,
                                &s.file_path,
                                "Refresh failed",
                                "Failed",
                                "failed",
                                false,
                            ),
                            None => templates::asset_card(
                                asset_id,
                                "Asset",
                                "",
                                "Refresh failed",
                                "Failed",
                                "failed",
                                false,
                            ),
                        }
                    }
                }
                .replace('\n', "");
                drop(store);
                let patch = PatchElements::new(html);
                yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
            }
        }
    };

    Sse::new(stream).into_response()
}

async fn asset_panel(
    AxumPath(asset_id): AxumPath<i64>,
    State(state): State<AppState>,
) -> axum::response::Result<Html<String>> {
    let store = state.store.lock().await;
    let detail = store.asset_detail(asset_id).await.map_err(internal_error)?;
    let materializations = store
        .list_materializations_for_asset(asset_id, 20)
        .await
        .map_err(internal_error)?;
    drop(store);

    Ok(Html(render_asset_panel(&detail, &materializations)))
}

async fn asset_panel_stream(
    AxumPath(asset_id): AxumPath<i64>,
    State(state): State<AppState>,
) -> axum::response::Result<impl IntoResponse> {
    let mut job_completion_rx = state.job_completion_tx.subscribe();
    let mut job_log_rx = state.job_log_tx.subscribe();

    let stream = stream! {
        let panel_html = {
            let store = state.store.lock().await;
            match store.asset_detail(asset_id).await {
                Ok(detail) => {
                    let materializations = store.list_materializations_for_asset(asset_id, 20).await.unwrap_or_default();
                    drop(store);
                    render_asset_panel(&detail, &materializations)
                }
                Err(_) => {
                    drop(store);
                    String::new()
                }
            }
        };

        let patch = PatchElements::new(panel_html.clone())
            .selector("#asset-panel");
        yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());

        loop {
            let recv_completion = job_completion_rx.recv();
            let recv_log = job_log_rx.recv();
            let timeout = tokio::time::sleep(Duration::from_secs(30));
            select! {
                result = recv_log => {
                    match result {
                        Ok(entry) if entry.asset_id == asset_id => {
                            let log_line = templates::job_log_line(&entry.message, entry.level.as_str());
                            let patch = PatchElements::new(log_line)
                                .selector("#job-log-entries")
                                .mode(ElementPatchMode::Append);
                            yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                        }
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
                result = recv_completion => {
                    match result {
                        Ok(completed_asset_id) if completed_asset_id == asset_id => {
                            let panel_html = {
                                let store = state.store.lock().await;
                                match store.asset_detail(asset_id).await {
                                    Ok(detail) => {
                                        let materializations = store.list_materializations_for_asset(asset_id, 20).await.unwrap_or_default();
                                        drop(store);
                                        render_asset_panel(&detail, &materializations)
                                    }
                                    Err(_) => continue,
                                }
                            };
                            let patch = PatchElements::new(panel_html)
                                .selector("#asset-panel");
                            yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                        }
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
                _ = timeout => {
                    let patch = PatchElements::new("").selector("#asset-panel");
                    yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                }
            }
        }
    };

    Ok(Sse::new(stream).into_response())
}

fn render_asset_panel(detail: &AssetDetail, materializations: &[MaterializationRecord]) -> String {
    let (status_label, status_tone) = classify_asset_state(
        detail
            .latest_materialization
            .as_ref()
            .map(|m| m.status.as_str()),
        detail
            .latest_materialization
            .as_ref()
            .map(|m| m.run_hash.as_str()),
        &detail.asset.definition_hash,
    );
    let is_fresh = status_tone == "fresh";

    let refresh_button = if is_fresh {
        format!(
            r#"<button type="button" data-on-click="$confirmAssetId = {}; $confirmModalOpen = true" class="inline-flex items-center justify-center rounded-lg px-3 py-1.5 text-sm font-medium btn-primary bg-gray-900 dark:bg-white text-white dark:text-gray-900 hover:bg-gray-800 dark:hover:bg-gray-100">Refresh</button>"#,
            detail.asset.asset_id
        )
    } else {
        format!(
            r#"<button type="button" data-on-click="@post('/assets/{}/materialize')" class="inline-flex items-center justify-center rounded-lg px-3 py-1.5 text-sm font-medium btn-primary bg-gray-900 dark:bg-white text-white dark:text-gray-900 hover:bg-gray-800 dark:hover:bg-gray-100">Refresh</button>"#,
            detail.asset.asset_id
        )
    };

    let job_history: String = materializations
        .iter()
        .map(|m| {
            format!(
                r#"<div class="flex items-center gap-3 py-2 border-b border-gray-100 dark:border-gray-800 last:border-0">
                  <span class="flex h-6 w-6 shrink-0 items-center justify-center">{}</span>
                  <div class="min-w-0 flex-1">
                    <p class="truncate text-sm font-mono text-gray-600 dark:text-gray-400">{}</p>
                    <p class="text-xs text-gray-500 dark:text-gray-500">{}</p>
                  </div>
                </div>"#,
                templates::job_status_icon(&m.status),
                templates::escape_html(&m.run_hash.chars().take(12).collect::<String>()),
                format_timestamp(m.created_at)
            )
        })
        .collect();

    let content = format!(
        r#"<div id="asset-panel-content" class="p-6" data-asset-id="{}">
          <div class="flex items-center justify-between border-b border-gray-200 dark:border-gray-800 pb-4">
            <div class="flex items-center gap-3">
              <h2 class="text-xl font-semibold text-gray-900 dark:text-white">{}</h2>
              <asset-status-badge label="{}" tone="{}"></asset-status-badge>
            </div>
            {}
          </div>
          {}
          <section class="mt-6 rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Definition</h3>
            <dl class="mt-4 grid gap-4 sm:grid-cols-2">
              <div>
                <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Module</dt>
                <dd class="mt-1 text-sm text-gray-700 dark:text-gray-300">{}</dd>
              </div>
              <div>
                <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">File</dt>
                <dd class="mt-1 text-sm text-gray-700 dark:text-gray-300">{}</dd>
              </div>
              <div class="sm:col-span-2">
                <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Definition hash</dt>
                <dd class="mt-1 overflow-x-auto text-sm text-gray-700 dark:text-gray-300"><code class="bg-gray-50 dark:bg-gray-800 px-2 py-1 rounded">{}</code></dd>
              </div>
            </dl>
          </section>
          <section class="mt-6 rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Job history</h3>
            <div class="mt-4">{}</div>
          </section>
        </div>"#,
        detail.asset.asset_id,
        templates::escape_html(&detail.asset.logical_name),
        templates::escape_html(status_label),
        status_tone,
        refresh_button,
        templates::job_log_viewer(),
        templates::escape_html(&detail.asset.module_path),
        templates::escape_html(&detail.asset.file_path),
        templates::escape_html(&detail.asset.definition_hash),
        if job_history.is_empty() {
            r#"<p class="text-sm text-gray-500 dark:text-gray-400">No jobs yet.</p>"#.to_string()
        } else {
            job_history
        }
    );

    format!(r#"<div id="asset-panel">{}</div>"#, content)
}

fn render_asset_list(assets: &[AssetSummary]) -> String {
    let mut items = String::new();
    items.push_str(
        r#"<section id="asset-list" class="mt-8 max-h-[calc(100vh-15rem)] space-y-4 overflow-y-auto scrollbar-custom pr-1">"#,
    );
    for asset in assets {
        items.push_str(&render_asset_card_from_summary(asset));
    }
    if assets.is_empty() {
        items.push_str(templates::empty_asset_list());
    }
    items.push_str("</section>");
    items
}

#[allow(dead_code)]
fn render_status_fragment(detail: &AssetDetail) -> String {
    let body = match &detail.latest_materialization {
        Some(materialization) => {
            let (status_label, status_tone) = classify_asset_state(
                Some(materialization.status.as_str()),
                Some(materialization.run_hash.as_str()),
                &detail.asset.definition_hash,
            );
            let error_html = materialization
                .last_error
                .as_deref()
                .map(|error| {
                    format!(
                        r#"<div class="sm:col-span-2">
                              <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Error</dt>
                              <dd class="mt-2 text-sm leading-6 text-rose-700 dark:text-rose-400">{}</dd>
                            </div>"#,
                        templates::escape_html(error)
                    )
                })
                .unwrap_or_default();
            format!(
                r#"
                <div class="flex items-center justify-between gap-4">
                  <h2 class="text-2xl font-semibold tracking-tight text-gray-900 dark:text-white">Latest run</h2>
                  <asset-status-badge label="{}" tone="{}"></asset-status-badge>
                </div>
                <dl class="mt-5 grid gap-5 sm:grid-cols-2">
                  <div>
                    <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Last updated</dt>
                    <dd class="mt-2 text-sm text-gray-700 dark:text-gray-300">{}</dd>
                  </div>
                  <div>
                    <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Raw status</dt>
                    <dd class="mt-2 text-sm text-gray-700 dark:text-gray-300">{}</dd>
                  </div>
                  <div>
                    <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Run hash</dt>
                    <dd class="mt-2 overflow-x-auto text-sm text-gray-700 dark:text-gray-300"><code class="bg-gray-50 dark:bg-gray-800 px-2 py-1 rounded">{}</code></dd>
                  </div>
                  <div>
                    <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Artifact</dt>
                    <dd class="mt-2 text-sm text-gray-700 dark:text-gray-300">{}</dd>
                  </div>
                  {}
                </dl>
                "#,
                templates::escape_html(status_label),
                status_tone,
                templates::escape_html(&format_last_updated(
                    Some(materialization.created_at),
                    Some(materialization.status.as_str()),
                )),
                templates::escape_html(&materialization.status),
                templates::escape_html(&materialization.run_hash),
                templates::escape_html(
                    materialization
                        .artifact_path
                        .as_deref()
                        .unwrap_or("Not published"),
                ),
                error_html
            )
        }
        None => r#"
            <div class="flex items-center justify-between gap-4">
              <h2 class="text-2xl font-semibold tracking-tight text-gray-900 dark:text-white">Latest run</h2>
              <asset-status-badge label="Stale" tone="stale"></asset-status-badge>
            </div>
            <p class="mt-6 text-sm leading-6 text-gray-600 dark:text-gray-400">This asset has not been materialized yet.</p>
          "#
        .to_string(),
    };
    format!(
        r#"<section id="asset-status" class="mt-4 rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">{}</section>"#,
        body
    )
}

#[allow(dead_code)]
fn render_run_error_status(logical_name: &str, message: &str) -> String {
    format!(
        r#"
        <section id="asset-status" class="mt-4 rounded-xl border border-rose-200 dark:border-rose-900 bg-rose-50 dark:bg-rose-950/30 px-5 py-5">
          <div class="flex items-center justify-between gap-4">
            <h2 class="text-2xl font-semibold tracking-tight text-gray-900 dark:text-white">Latest run</h2>
            <asset-status-badge label="Failed" tone="failed"></asset-status-badge>
          </div>
          <p class="mt-6 text-sm leading-6 text-gray-700 dark:text-gray-300">
            Refreshing <span class="font-medium text-gray-900 dark:text-white">{}</span> failed.
          </p>
          <p class="mt-3 text-sm leading-6 text-rose-700 dark:text-rose-400">{}</p>
        </section>
        "#,
        templates::escape_html(logical_name),
        templates::escape_html(message)
    )
}

fn render_asset_card_from_summary(asset: &AssetSummary) -> String {
    let (status_label, status_tone) = classify_asset_state(
        asset.materialization_status.as_deref(),
        asset.materialization_run_hash.as_deref(),
        &asset.definition_hash,
    );
    templates::asset_card(
        asset.asset_id,
        &asset.function_name,
        &asset.file_path,
        &format_last_updated(
            asset.materialization_created_at,
            asset.materialization_status.as_deref(),
        ),
        status_label,
        status_tone,
        status_tone == "fresh",
    )
}

fn render_asset_card_from_detail(detail: &AssetDetail) -> String {
    let (status_label, status_tone) = classify_asset_state(
        detail
            .latest_materialization
            .as_ref()
            .map(|item| item.status.as_str()),
        detail
            .latest_materialization
            .as_ref()
            .map(|item| item.run_hash.as_str()),
        &detail.asset.definition_hash,
    );
    templates::asset_card(
        detail.asset.asset_id,
        &detail.asset.function_name,
        &detail.asset.file_path,
        &format_last_updated(
            detail
                .latest_materialization
                .as_ref()
                .map(|item| item.created_at),
            detail
                .latest_materialization
                .as_ref()
                .map(|item| item.status.as_str()),
        ),
        status_label,
        status_tone,
        status_tone == "fresh",
    )
}

fn classify_asset_state(
    latest_status: Option<&str>,
    latest_run_hash: Option<&str>,
    definition_hash: &str,
) -> (&'static str, &'static str) {
    match latest_status {
        Some("queued") => ("Queued", "running"),
        Some("running") => ("Running", "running"),
        Some("failed") => ("Failed", "failed"),
        Some("success") if latest_run_hash == Some(definition_hash) => ("Fresh", "fresh"),
        Some("success") => ("Stale", "stale"),
        Some(_) => ("Stale", "stale"),
        None => ("Stale", "stale"),
    }
}

fn format_last_updated(timestamp: Option<i64>, status: Option<&str>) -> String {
    match timestamp {
        Some(value) => {
            let prefix = match status {
                Some("queued") => "Queued",
                Some("running") => "Run started",
                _ => "Last updated",
            };
            format!("{prefix} {}", format_timestamp(value))
        }
        None => "Never materialized".to_string(),
    }
}

fn format_timestamp(timestamp: i64) -> String {
    let datetime =
        OffsetDateTime::from_unix_timestamp(timestamp).unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let month = match datetime.month() {
        Month::January => "Jan",
        Month::February => "Feb",
        Month::March => "Mar",
        Month::April => "Apr",
        Month::May => "May",
        Month::June => "Jun",
        Month::July => "Jul",
        Month::August => "Aug",
        Month::September => "Sep",
        Month::October => "Oct",
        Month::November => "Nov",
        Month::December => "Dec",
    };
    let hour = datetime.hour();
    let meridiem = if hour >= 12 { "PM" } else { "AM" };
    let display_hour = match hour % 12 {
        0 => 12,
        value => value,
    };

    format!(
        "{} {}, {} at {}:{:02} {} UTC",
        month,
        datetime.day(),
        datetime.year(),
        display_hour,
        datetime.minute(),
        meridiem
    )
}

// ---------------------------------------------------------------------------
// JSON API handlers
// ---------------------------------------------------------------------------

async fn api_list_assets(
    State(state): State<AppState>,
) -> axum::response::Result<Json<Vec<AssetSummary>>> {
    let store = state.store.lock().await;
    let assets = store.list_assets().await.map_err(internal_error)?;
    Ok(Json(assets))
}

async fn api_get_asset(
    AxumPath(asset_id): AxumPath<i64>,
    State(state): State<AppState>,
) -> axum::response::Result<Json<AssetDetail>> {
    let store = state.store.lock().await;
    let detail = store.asset_detail(asset_id).await.map_err(internal_error)?;
    Ok(Json(detail))
}

async fn api_materialize_asset(
    AxumPath(asset_id): AxumPath<i64>,
    State(state): State<AppState>,
) -> axum::response::Result<Json<AssetDetail>> {
    let detail = crate::enqueue_refresh_request(&state, asset_id)
        .await
        .map_err(internal_error)?;
    Ok(Json(detail))
}

async fn api_reindex(
    State(state): State<AppState>,
) -> axum::response::Result<Json<Vec<AssetSummary>>> {
    crate::reindex(&state).await.map_err(internal_error)?;
    let store = state.store.lock().await;
    let assets = store.list_assets().await.map_err(internal_error)?;
    Ok(Json(assets))
}

fn internal_error(error: anyhow::Error) -> axum::response::Response {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        format!("internal error: {error:#}"),
    )
        .into_response()
}
