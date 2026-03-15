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
use barca_core::models::{AssetDetail, AssetSummary, JobDetail, MaterializationRecord};
use datastar::consts::ElementPatchMode;
use datastar::prelude::PatchElements;
use time::{Month, OffsetDateTime};
use tokio::select;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

use crate::store::MetadataStore;
use crate::templates;
use crate::AppState;

#[derive(OpenApi)]
#[openapi(
    info(title = "Barca API", version = "0.1.0"),
    components(schemas(AssetSummary, AssetDetail, MaterializationRecord, JobDetail, barca_core::models::IndexedAsset,))
)]
struct ApiDoc;

pub fn router() -> Router<AppState> {
    // Build the OpenAPI router for /api/* routes
    let (api_router, api) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(utoipa_axum::routes!(api_list_assets))
        .routes(utoipa_axum::routes!(api_get_asset))
        .routes(utoipa_axum::routes!(api_materialize_asset))
        .routes(utoipa_axum::routes!(api_reindex))
        .routes(utoipa_axum::routes!(api_reset))
        .routes(utoipa_axum::routes!(api_list_jobs))
        .routes(utoipa_axum::routes!(api_get_job))
        .split_for_parts();

    Router::new()
        .route("/", get(index_page))
        .route("/stream", get(main_stream))
        .route("/reindex", post(reindex_action))
        .route("/reset", post(reset_action))
        .route("/assets/{asset_id}/materialize", post(materialize_action))
        .route("/assets/{asset_id}/panel", get(asset_panel))
        .route("/assets/{asset_id}/panel/stream", get(asset_panel_stream))
        .route("/jobs/{job_id}/panel/stream", get(job_panel_stream))
        .merge(api_router)
        .merge(SwaggerUi::new("/api/docs").url("/api/openapi.json", api))
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

async fn index_page(Query(query): Query<IndexQuery>, State(state): State<AppState>) -> axum::response::Result<Html<String>> {
    let view = query.view.as_deref().unwrap_or("assets");
    let store = state.store.lock().await;

    let body = match view {
        "jobs" => {
            let jobs = store.list_recent_materializations(50).await.map_err(internal_error)?;
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
            <div data-on:load__window="@get('/stream')" style="display:none" id="stream-init"></div>
            <div id="stream-ping" style="display:none"></div>
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
                let assets = {
                    let store = state.store.lock().await;
                    store.list_assets().await.unwrap_or_default()
                };
                let html = render_asset_list(&assets).replace('\n', "");
                let patch = PatchElements::new(html).selector("#main-content");
                yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                // Notify all connected main-page streams to also refresh
                let _ = state.state_tx.send(());
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

async fn materialize_action(AxumPath(asset_id): AxumPath<i64>, State(state): State<AppState>) -> impl IntoResponse {
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

/// Persistent SSE stream for the main asset list page.
///
/// Listens on two channels:
/// - `job_completion_tx`: patches the individual asset card when a job finishes
/// - `state_tx`: re-renders the full `#main-content` (fired after reindex/reset)
async fn main_stream(State(state): State<AppState>) -> impl IntoResponse {
    let mut job_completion_rx = state.job_completion_tx.subscribe();
    let mut state_rx = state.state_tx.subscribe();

    let stream = stream! {
        loop {
            let recv_completion = job_completion_rx.recv();
            let recv_state = state_rx.recv();
            let timeout = tokio::time::sleep(Duration::from_secs(30));
            select! {
                result = recv_completion => {
                    match result {
                        Ok(asset_id) => {
                            let store = state.store.lock().await;
                            if let Ok(detail) = store.asset_detail(asset_id).await {
                                drop(store);
                                let html = render_asset_card_from_detail(&detail).replace('\n', "");
                                let patch = PatchElements::new(html);
                                yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                            }
                        }
                        Err(_) => break,
                    }
                }
                result = recv_state => {
                    match result {
                        Ok(()) => {
                            let assets = {
                                let store = state.store.lock().await;
                                store.list_assets().await.unwrap_or_default()
                            };
                            let html = render_asset_list(&assets).replace('\n', "");
                            let patch = PatchElements::new(html).selector("#main-content");
                            yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                        }
                        Err(_) => break,
                    }
                }
                _ = timeout => {
                    // keepalive — patch the hidden ping element so the connection stays alive
                    let patch = PatchElements::new(r#"<div id="stream-ping"></div>"#);
                    yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                }
            }
        }
    };

    Sse::new(stream).into_response()
}

/// Reset all barca state (db, artifacts, tmp), reinitialise the store, and
/// broadcast a full UI refresh.  Intended for the sidebar Reset button.
async fn reset_action(State(state): State<AppState>) -> impl IntoResponse {
    let stream = stream! {
        let db_path = state.repo_root.join(".barca").join("metadata.db");

        if let Err(e) = crate::reset(&state.repo_root, false, false, false) {
            let html = format!(
                r#"<section id="asset-list" class="mt-8"><article class="rounded-[28px] border border-rose-200 bg-rose-50/70 px-6 py-6"><p class="text-2xl text-zinc-950">Reset failed</p><p class="mt-3 text-sm text-rose-700">{}</p></article></section>"#,
                templates::escape_html(&e.to_string())
            );
            let patch = PatchElements::new(html).selector("#main-content");
            yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
            return;
        }

        // Swap in a fresh store so the server doesn't keep the old (deleted) DB.
        match MetadataStore::open(&db_path).await {
            Ok(new_store) => {
                let mut guard = state.store.lock().await;
                *guard = new_store;
                drop(guard);
                state.definition_cache.lock().await.clear();
            }
            Err(e) => {
                let html = format!(
                    r#"<section id="asset-list" class="mt-8"><article class="rounded-[28px] border border-rose-200 bg-rose-50/70 px-6 py-6"><p class="text-2xl text-zinc-950">Reset failed</p><p class="mt-3 text-sm text-rose-700">{}</p></article></section>"#,
                    templates::escape_html(&e.to_string())
                );
                let patch = PatchElements::new(html).selector("#main-content");
                yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                return;
            }
        }

        // Re-inspect Python modules so the empty store is repopulated.
        let _ = crate::reindex(&state).await;

        let assets = {
            let store = state.store.lock().await;
            store.list_assets().await.unwrap_or_default()
        };
        let html = render_asset_list(&assets).replace('\n', "");
        let patch = PatchElements::new(html).selector("#main-content");
        yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());

        // Tell all other connected main-page streams to refresh too.
        let _ = state.state_tx.send(());
    };

    Sse::new(stream).into_response()
}

async fn asset_panel(AxumPath(asset_id): AxumPath<i64>, State(state): State<AppState>) -> axum::response::Result<Html<String>> {
    let store = state.store.lock().await;
    let detail = store.asset_detail(asset_id).await.map_err(internal_error)?;
    let materializations = store.list_materializations_for_asset(asset_id, 20).await.map_err(internal_error)?;
    let persisted_logs = load_latest_terminal_logs(&store, &detail).await;
    drop(store);

    Ok(Html(render_asset_panel(&detail, &materializations, &persisted_logs)))
}

#[derive(Debug, serde::Deserialize)]
struct PanelStreamQuery {
    #[serde(default)]
    refresh: bool,
}

async fn asset_panel_stream(AxumPath(asset_id): AxumPath<i64>, Query(params): Query<PanelStreamQuery>, State(state): State<AppState>) -> axum::response::Result<impl IntoResponse> {
    let mut job_completion_rx = state.job_completion_tx.subscribe();
    let mut job_log_rx = state.job_log_tx.subscribe();

    let stream = stream! {
        // Send initial panel HTML
        let panel_html = {
            let store = state.store.lock().await;
            match store.asset_detail(asset_id).await {
                Ok(detail) => {
                    let materializations = store.list_materializations_for_asset(asset_id, 20).await.unwrap_or_default();
                    let logs = load_latest_terminal_logs(&store, &detail).await;
                    drop(store);
                    render_asset_panel(&detail, &materializations, &logs)
                }
                Err(_) => {
                    drop(store);
                    String::new()
                }
            }
        };

        let patch = PatchElements::new(panel_html).selector("#panel-content");
        yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());

        // If refresh requested, enqueue materialization now (after SSE is established)
        if params.refresh {
            match crate::enqueue_refresh_request(&state, asset_id).await {
                Ok(detail) => {
                    // Update the asset card on the main page too
                    let card_html = render_asset_card_from_detail(&detail).replace('\n', "");
                    let patch = PatchElements::new(card_html);
                    yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                }
                Err(e) => {
                    tracing::error!(asset_id, error = %e, "failed to enqueue refresh from panel");
                }
            }
        }

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
                            // Re-render panel with updated state
                            let panel_html = {
                                let store = state.store.lock().await;
                                match store.asset_detail(asset_id).await {
                                    Ok(detail) => {
                                        let materializations = store.list_materializations_for_asset(asset_id, 20).await.unwrap_or_default();
                                        let logs = load_latest_terminal_logs(&store, &detail).await;
                                        drop(store);
                                        render_asset_panel(&detail, &materializations, &logs)
                                    }
                                    Err(_) => continue,
                                }
                            };
                            let patch = PatchElements::new(panel_html).selector("#panel-content");
                            yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());

                            // Also update the asset card on the main page
                            let card_html = {
                                let store = state.store.lock().await;
                                match store.asset_detail(asset_id).await {
                                    Ok(detail) => render_asset_card_from_detail(&detail).replace('\n', ""),
                                    Err(_) => continue,
                                }
                            };
                            let patch = PatchElements::new(card_html);
                            yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                        }
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
                _ = timeout => {
                    let patch = PatchElements::new("").selector("#panel-content");
                    yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                }
            }
        }
    };

    Ok(Sse::new(stream).into_response())
}

async fn job_panel_stream(AxumPath(job_id): AxumPath<i64>, State(state): State<AppState>) -> axum::response::Result<impl IntoResponse> {
    let mut job_completion_rx = state.job_completion_tx.subscribe();
    let mut job_log_rx = state.job_log_tx.subscribe();

    // Look up the asset_id for this job so we can filter events
    let asset_id = {
        let store = state.store.lock().await;
        let (mat, _) = store.get_materialization_with_asset(job_id).await.map_err(internal_error)?;
        mat.asset_id
    };

    let stream = stream! {
        let panel_html = {
            let store = state.store.lock().await;
            match store.get_materialization_with_asset(job_id).await {
                Ok((mat, summary)) => {
                    let logs = store.get_job_logs(job_id).await.unwrap_or_default();
                    templates::job_panel(&mat, &summary, &logs)
                }
                Err(_) => String::new(),
            }
        };

        let patch = PatchElements::new(panel_html).selector("#panel-content");
        yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());

        loop {
            let recv_completion = job_completion_rx.recv();
            let recv_log = job_log_rx.recv();
            let timeout = tokio::time::sleep(Duration::from_secs(30));
            select! {
                result = recv_log => {
                    match result {
                        Ok(entry) if entry.asset_id == asset_id && entry.job_id == job_id => {
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
                                match store.get_materialization_with_asset(job_id).await {
                                    Ok((mat, summary)) => {
                                        let logs = store.get_job_logs(job_id).await.unwrap_or_default();
                                        templates::job_panel(&mat, &summary, &logs)
                                    }
                                    Err(_) => continue,
                                }
                            };
                            let patch = PatchElements::new(panel_html).selector("#panel-content");
                            yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                        }
                        Ok(_) => continue,
                        Err(_) => break,
                    }
                }
                _ = timeout => {
                    let patch = PatchElements::new("").selector("#panel-content");
                    yield Ok::<_, Infallible>(patch.write_as_axum_sse_event());
                }
            }
        }
    };

    Ok(Sse::new(stream).into_response())
}

async fn load_latest_terminal_logs(store: &crate::store::MetadataStore, detail: &AssetDetail) -> Vec<barca_core::models::JobLogRecord> {
    if let Some(mat) = &detail.latest_materialization {
        if mat.status == "success" || mat.status == "failed" {
            return store.get_job_logs(mat.materialization_id).await.unwrap_or_default();
        }
    }
    Vec::new()
}

fn render_asset_panel(detail: &AssetDetail, materializations: &[MaterializationRecord], persisted_logs: &[barca_core::models::JobLogRecord]) -> String {
    let (status_label, status_tone) = classify_asset_state(
        detail.latest_materialization.as_ref().map(|m| m.status.as_str()),
        detail.latest_materialization.as_ref().map(|m| m.run_hash.as_str()),
        &detail.asset.definition_hash,
    );
    let is_fresh = status_tone == "fresh";

    let refresh_button = if is_fresh {
        format!(
            r#"<button type="button" data-on:click="$confirmAssetId = {}; $confirmModalOpen = true" class="inline-flex items-center justify-center rounded-lg px-3 py-1.5 text-sm font-medium btn-primary bg-gray-900 dark:bg-white text-white dark:text-gray-900 hover:bg-gray-800 dark:hover:bg-gray-100">Refresh</button>"#,
            detail.asset.asset_id
        )
    } else {
        format!(
            r#"<button type="button" data-on:click="@get('/assets/{}/panel/stream?refresh=true')" class="inline-flex items-center justify-center rounded-lg px-3 py-1.5 text-sm font-medium btn-primary bg-gray-900 dark:bg-white text-white dark:text-gray-900 hover:bg-gray-800 dark:hover:bg-gray-100">Refresh</button>"#,
            detail.asset.asset_id
        )
    };

    let job_history: String = materializations
        .iter()
        .map(|m| {
            format!(
                r#"<div class="flex items-center gap-3 py-2 border-b border-gray-100 dark:border-gray-800 last:border-0 cursor-pointer hover:bg-gray-50 dark:hover:bg-gray-800 rounded-lg px-2 -mx-2 transition-colors" data-on:click="@get('/jobs/{}/panel/stream')">
                  <span class="flex h-6 w-6 shrink-0 items-center justify-center">{}</span>
                  <div class="min-w-0 flex-1">
                    <p class="truncate text-sm font-mono text-gray-600 dark:text-gray-400">Job #{} · {}</p>
                    <p class="text-xs text-gray-500 dark:text-gray-500">{}</p>
                  </div>
                </div>"#,
                m.materialization_id,
                templates::job_status_icon(&m.status),
                m.materialization_id,
                templates::escape_html(&m.run_hash.chars().take(12).collect::<String>()),
                format_timestamp(m.created_at)
            )
        })
        .collect();

    format!(
        r#"<div id="panel-content" class="p-6" data-asset-id="{}">
          <div class="flex items-center justify-between gap-3 border-b border-gray-200 dark:border-gray-800 pb-4">
            <div class="flex items-center gap-3 min-w-0">
              <h2 class="text-xl font-semibold text-gray-900 dark:text-white truncate">{}</h2>
              <asset-status-badge label="{}" tone="{}"></asset-status-badge>
            </div>
            <div class="shrink-0">{}</div>
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
                <dd class="mt-1 text-sm text-gray-700 dark:text-gray-300 break-all"><code class="bg-gray-50 dark:bg-gray-800 px-2 py-1 rounded">{}</code></dd>
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
        if !persisted_logs.is_empty() {
            templates::job_log_viewer_with_logs(persisted_logs)
        } else {
            templates::job_log_viewer().to_string()
        },
        templates::escape_html(&detail.asset.module_path),
        templates::escape_html(&detail.asset.file_path),
        templates::escape_html(&detail.asset.definition_hash),
        if job_history.is_empty() {
            r#"<p class="text-sm text-gray-500 dark:text-gray-400">No jobs yet.</p>"#.to_string()
        } else {
            job_history
        }
    )
}

fn render_asset_list(assets: &[AssetSummary]) -> String {
    let mut items = String::new();
    items.push_str(r#"<section id="asset-list" class="mt-8 max-h-[calc(100vh-15rem)] space-y-4 overflow-y-auto scrollbar-custom pr-1">"#);
    for asset in assets {
        items.push_str(&render_asset_card_from_summary(asset));
    }
    if assets.is_empty() {
        items.push_str(templates::empty_asset_list());
    }
    items.push_str("</section>");
    items
}

fn render_asset_card_from_summary(asset: &AssetSummary) -> String {
    let (status_label, status_tone) = classify_asset_state(asset.materialization_status.as_deref(), asset.materialization_run_hash.as_deref(), &asset.definition_hash);
    templates::asset_card(
        asset.asset_id,
        &asset.function_name,
        &asset.file_path,
        &format_last_updated(asset.materialization_created_at, asset.materialization_status.as_deref()),
        status_label,
        status_tone,
        status_tone == "fresh",
    )
}

fn render_asset_card_from_detail(detail: &AssetDetail) -> String {
    let (status_label, status_tone) = classify_asset_state(
        detail.latest_materialization.as_ref().map(|item| item.status.as_str()),
        detail.latest_materialization.as_ref().map(|item| item.run_hash.as_str()),
        &detail.asset.definition_hash,
    );
    templates::asset_card(
        detail.asset.asset_id,
        &detail.asset.function_name,
        &detail.asset.file_path,
        &format_last_updated(
            detail.latest_materialization.as_ref().map(|item| item.created_at),
            detail.latest_materialization.as_ref().map(|item| item.status.as_str()),
        ),
        status_label,
        status_tone,
        status_tone == "fresh",
    )
}

fn classify_asset_state(latest_status: Option<&str>, latest_run_hash: Option<&str>, definition_hash: &str) -> (&'static str, &'static str) {
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
    let datetime = OffsetDateTime::from_unix_timestamp(timestamp).unwrap_or(OffsetDateTime::UNIX_EPOCH);
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

    format!("{} {}, {} at {}:{:02} {} UTC", month, datetime.day(), datetime.year(), display_hour, datetime.minute(), meridiem)
}

// ---------------------------------------------------------------------------
// JSON API handlers
// ---------------------------------------------------------------------------

/// List all assets
#[utoipa::path(
    get,
    path = "/api/assets",
    responses(
        (status = 200, description = "List of all indexed assets", body = Vec<AssetSummary>)
    ),
    tag = "assets"
)]
async fn api_list_assets(State(state): State<AppState>) -> axum::response::Result<Json<Vec<AssetSummary>>> {
    let store = state.store.lock().await;
    let assets = store.list_assets().await.map_err(internal_error)?;
    Ok(Json(assets))
}

/// Get asset details by ID
#[utoipa::path(
    get,
    path = "/api/assets/{asset_id}",
    params(("asset_id" = i64, Path, description = "Asset ID")),
    responses(
        (status = 200, description = "Asset details", body = AssetDetail),
        (status = 500, description = "Asset not found")
    ),
    tag = "assets"
)]
async fn api_get_asset(AxumPath(asset_id): AxumPath<i64>, State(state): State<AppState>) -> axum::response::Result<Json<AssetDetail>> {
    let store = state.store.lock().await;
    let detail = store.asset_detail(asset_id).await.map_err(internal_error)?;
    Ok(Json(detail))
}

/// Trigger materialization for an asset
#[utoipa::path(
    post,
    path = "/api/assets/{asset_id}/materialize",
    params(("asset_id" = i64, Path, description = "Asset ID")),
    responses(
        (status = 200, description = "Asset detail after enqueueing", body = AssetDetail),
        (status = 500, description = "Failed to enqueue")
    ),
    tag = "assets"
)]
async fn api_materialize_asset(AxumPath(asset_id): AxumPath<i64>, State(state): State<AppState>) -> axum::response::Result<Json<AssetDetail>> {
    let detail = crate::enqueue_refresh_request(&state, asset_id).await.map_err(internal_error)?;
    Ok(Json(detail))
}

/// Re-inspect Python modules and update asset index
#[utoipa::path(
    post,
    path = "/api/reindex",
    responses(
        (status = 200, description = "Assets after reindex", body = Vec<AssetSummary>)
    ),
    tag = "system"
)]
async fn api_reindex(State(state): State<AppState>) -> axum::response::Result<Json<Vec<AssetSummary>>> {
    crate::reindex(&state).await.map_err(internal_error)?;
    let store = state.store.lock().await;
    let assets = store.list_assets().await.map_err(internal_error)?;
    Ok(Json(assets))
}

/// Reset all barca state and re-inspect Python modules
#[utoipa::path(
    post,
    path = "/api/reset",
    responses(
        (status = 200, description = "Assets after reset and reindex", body = Vec<AssetSummary>)
    ),
    tag = "system"
)]
async fn api_reset(State(state): State<AppState>) -> axum::response::Result<Json<Vec<AssetSummary>>> {
    let db_path = state.repo_root.join(".barca").join("metadata.db");
    crate::reset(&state.repo_root, false, false, false).map_err(internal_error)?;
    let new_store = MetadataStore::open(&db_path).await.map_err(internal_error)?;
    {
        let mut guard = state.store.lock().await;
        *guard = new_store;
        state.definition_cache.lock().await.clear();
    }
    crate::reindex(&state).await.map_err(internal_error)?;
    let store = state.store.lock().await;
    let assets = store.list_assets().await.map_err(internal_error)?;
    drop(store);
    let _ = state.state_tx.send(());
    Ok(Json(assets))
}

/// List recent materialization jobs
#[utoipa::path(
    get,
    path = "/api/jobs",
    responses(
        (status = 200, description = "Recent jobs", body = Vec<JobDetail>)
    ),
    tag = "jobs"
)]
async fn api_list_jobs(State(state): State<AppState>) -> axum::response::Result<Json<Vec<JobDetail>>> {
    let store = state.store.lock().await;
    let pairs = store.list_recent_materializations(50).await.map_err(internal_error)?;
    let jobs: Vec<JobDetail> = pairs.into_iter().map(|(mat, asset)| JobDetail { job: mat, asset }).collect();
    Ok(Json(jobs))
}

/// Get job details by ID
#[utoipa::path(
    get,
    path = "/api/jobs/{job_id}",
    params(("job_id" = i64, Path, description = "Materialization job ID")),
    responses(
        (status = 200, description = "Job details", body = JobDetail),
        (status = 500, description = "Job not found")
    ),
    tag = "jobs"
)]
async fn api_get_job(AxumPath(job_id): AxumPath<i64>, State(state): State<AppState>) -> axum::response::Result<Json<JobDetail>> {
    let store = state.store.lock().await;
    let (mat, asset) = store.get_materialization_with_asset(job_id).await.map_err(internal_error)?;
    Ok(Json(JobDetail { job: mat, asset }))
}

fn internal_error(error: anyhow::Error) -> axum::response::Response {
    (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("internal error: {error:#}")).into_response()
}
