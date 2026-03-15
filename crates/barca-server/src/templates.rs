// HTML templates and components for the Barca UI

/// Base HTML page template with dark mode support
pub fn page(title: &str, body: &str) -> String {
    format!(
        r##"<!doctype html>
        <html lang="en">
          <head>
            <meta charset="utf-8" />
            <meta name="viewport" content="width=device-width, initial-scale=1" />
            <title>{}</title>
            <link rel="preconnect" href="https://fonts.googleapis.com" />
            <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
            <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&display=swap" rel="stylesheet" />
            <style>{}</style>
            <script type="module" src="https://cdn.jsdelivr.net/gh/starfederation/datastar@1.0.0-RC.8/bundles/datastar.js"></script>
            {}
            {}
          </head>
          <body class="min-h-screen flex flex-col bg-white dark:bg-[#0a0a0a] text-gray-900 dark:text-gray-100 antialiased" data-signals='{{confirmModalOpen: false, confirmAssetId: 0, _panelOpen: false}}'>{}
            <div id="panel-backdrop" data-class:open="$_panelOpen" data-on:click="$_panelOpen = false"></div>
            <div id="panel-wrapper" data-class:open="$_panelOpen">
              <div id="panel-content"></div>
            </div>
            <div data-on:keydown__window="evt.key === 'Escape' && ($_panelOpen = false)"></div>
            <div data-show="$confirmModalOpen" class="fixed inset-0 z-50 flex items-center justify-center bg-black/25" data-on:click="$confirmModalOpen = false">
              <div data-on:click__stop="void 0" class="w-[400px] max-w-[90vw] rounded-2xl border border-gray-200 dark:border-gray-700 bg-white dark:bg-[#0f1117] p-6 shadow-[0_20px_60px_rgba(0,0,0,0.15)] dark:shadow-[0_20px_60px_rgba(0,0,0,0.5)]">
                <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Re-materialize?</h3>
                <p class="mt-2 text-sm text-gray-600 dark:text-gray-400">This asset is already fresh. Are you sure you want to run it again?</p>
                <div class="mt-5 flex justify-end gap-3">
                  <button type="button" data-on:click="$confirmModalOpen = false" class="rounded-lg px-3.5 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-800 transition-colors">Cancel</button>
                  <button type="button" data-on:click="$_panelOpen = true; $confirmModalOpen = false; @get(`/assets/${{$confirmAssetId}}/panel/stream?refresh=true`)" class="rounded-lg px-3.5 py-2 text-sm font-medium bg-gray-900 dark:bg-white text-white dark:text-gray-900 hover:bg-gray-800 dark:hover:bg-gray-100 transition-colors">Refresh anyway</button>
                </div>
              </div>
            </div>
          </body>
        </html>"##,
        escape_html(title),
        include_str!("../../../static/css/output.css"),
        web_components(),
        styles(),
        body
    )
}

/// Web component definitions (AssetStatusBadge)
fn web_components() -> &'static str {
    r#"<script>
              class AssetStatusBadge extends HTMLElement {
                static get observedAttributes() {
                  return ["label", "tone"];
                }

                connectedCallback() {
                  this.render();
                }

                attributeChangedCallback() {
                  this.render();
                }

                render() {
                  const label = this.getAttribute("label") || "Stale";
                  const tone = this.getAttribute("tone") || "stale";
                  const tones = {
                    fresh: "bg-emerald-50 dark:bg-emerald-950/30 text-emerald-700 dark:text-emerald-400 ring-emerald-600/20 dark:ring-emerald-500/30",
                    stale: "bg-gray-100 dark:bg-gray-800 text-gray-600 dark:text-gray-400 ring-gray-300/30 dark:ring-gray-600/30",
                    running: "bg-amber-50 dark:bg-amber-950/30 text-amber-700 dark:text-amber-400 ring-amber-600/20 dark:ring-amber-500/30",
                    failed: "bg-rose-50 dark:bg-rose-950/30 text-rose-700 dark:text-rose-400 ring-rose-600/20 dark:ring-rose-500/30"
                  };
                  const classes = tones[tone] || tones.stale;
                  this.innerHTML =
                    `<span class="inline-flex items-center rounded-full px-2.5 py-1 text-xs font-medium ring-1 ring-inset ${classes}">${escapeHtml(label)}</span>`;
                }
              }

              function escapeHtml(value) {
                return value
                  .replaceAll("&", "&amp;")
                  .replaceAll("<", "&lt;")
                  .replaceAll(">", "&gt;")
                  .replaceAll('"', "&quot;");
              }

              customElements.define("asset-status-badge", AssetStatusBadge);

              // Dark mode initialization
              const savedTheme = localStorage.getItem('theme');
              const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
              if (savedTheme === 'dark' || (!savedTheme && prefersDark)) {
                document.documentElement.classList.add('dark');
              }

              function toggleTheme() {
                const html = document.documentElement;
                html.classList.toggle('dark');
                localStorage.setItem('theme', html.classList.contains('dark') ? 'dark' : 'light');
              }

            </script>"#
}

/// CSS styles for the application
fn styles() -> &'static str {
    r#"<style>
              html {
                font-family: 'Inter', ui-sans-serif, system-ui, sans-serif;
              }

              body {
                margin: 0;
                display: flex;
                flex-direction: column;
                transition: background-color 0.2s, color 0.2s;
              }

              main {
                width: 100%;
                box-sizing: border-box;
              }

              .card {
                transition: box-shadow 0.2s, transform 0.2s;
              }

              .card:hover {
                transform: translateY(-1px);
              }

              body:not(.dark) .card {
                box-shadow: 0 1px 3px rgba(0,0,0,0.08);
              }

              body:not(.dark) .card:hover {
                box-shadow: 0 4px 12px rgba(0,0,0,0.12);
              }

              .dark .card {
                box-shadow: 0 1px 3px rgba(0,0,0,0.5);
              }

              .dark .card:hover {
                box-shadow: 0 4px 12px rgba(0,0,0,0.7);
              }

              .btn-primary {
                transition: all 0.2s;
              }

              .btn-primary:hover {
                transform: translateY(-1px);
              }

              .btn-primary:active {
                transform: translateY(0);
              }

              code {
                font-family: 'SF Mono', 'Menlo', 'Monaco', monospace;
                font-size: 0.875em;
              }

              .scrollbar-custom::-webkit-scrollbar {
                width: 8px;
              }

              body:not(.dark) .scrollbar-custom::-webkit-scrollbar-thumb {
                background: rgba(0, 0, 0, 0.15);
                border-radius: 4px;
              }

              .dark .scrollbar-custom::-webkit-scrollbar-thumb {
                background: rgba(255, 255, 255, 0.15);
                border-radius: 4px;
              }

              .scrollbar-custom::-webkit-scrollbar-thumb:hover {
                background: rgba(0, 0, 0, 0.25);
              }

              .dark .scrollbar-custom::-webkit-scrollbar-thumb:hover {
                background: rgba(255, 255, 255, 0.25);
              }

              /* Panel slide-in animation */
              #panel-wrapper {
                position: fixed;
                right: 0;
                top: 0;
                bottom: 0;
                z-index: 40;
                width: 440px;
                max-width: 90vw;
                transform: translateX(100%);
                transition: transform 0.32s cubic-bezier(0.4, 0, 0.2, 1);
                overflow-y: auto;
                overflow-x: hidden;
                background: #ffffff;
                border-left: 1px solid #e5e7eb;
                box-shadow: -8px 0 32px rgba(0,0,0,0.06);
              }

              .dark #panel-wrapper {
                background: #0f1117;
                border-left-color: rgba(255,255,255,0.08);
                box-shadow: -8px 0 32px rgba(0,0,0,0.5);
              }

              #panel-wrapper.open {
                transform: translateX(0);
              }

              #panel-backdrop {
                position: fixed;
                inset: 0;
                z-index: 39;
                background: rgba(0,0,0,0);
                pointer-events: none;
                transition: background 0.32s cubic-bezier(0.4, 0, 0.2, 1);
              }

              #panel-backdrop.open {
                background: rgba(0,0,0,0.12);
                pointer-events: auto;
              }
            </style>"#
}

/// Theme toggle button component
pub fn theme_toggle() -> &'static str {
    r#"<div class="fixed bottom-4 left-4 z-50">
              <button
                onclick="toggleTheme()"
                class="p-2.5 rounded-lg bg-gray-100 dark:bg-gray-800 hover:bg-gray-200 dark:hover:bg-gray-700 transition-colors"
                aria-label="Toggle theme"
              >
                <svg class="w-5 h-5 dark:hidden" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M20.354 15.354A9 9 0 018.646 3.646 9.003 9.003 0 0012 21a9.003 9.003 0 008.354-5.646z" />
                </svg>
                <svg class="w-5 h-5 hidden dark:block" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 3v1m0 16v1m9-9h-1M4 12H3m15.364 6.364l-.707-.707M6.343 6.343l-.707-.707m12.728 0l-.707.707M6.343 17.657l-.707.707M16 12a4 4 0 11-8 0 4 4 0 018 0z" />
                </svg>
              </button>
            </div>"#
}

/// Page header with title
pub fn page_header(title: &str, subtitle: &str) -> String {
    format!(
        r#"<header class="border-b border-gray-200 dark:border-gray-800 pb-6">
                <div class="max-w-3xl">
                  <p class="text-[11px] font-medium uppercase tracking-[0.2em] text-gray-500 dark:text-gray-400">Barca</p>
                  <h1 class="mt-3 text-4xl font-bold tracking-tight text-gray-900 dark:text-white sm:text-5xl">{}</h1>
                  <p class="mt-3 max-w-2xl text-sm leading-6 text-gray-600 dark:text-gray-400">
                    {}
                  </p>
                </div>
              </header>"#,
        escape_html(title),
        escape_html(subtitle)
    )
}

/// Sidebar with Assets and Jobs tabs
pub fn sidebar(active: &str) -> String {
    let assets_active = if active == "assets" {
        r#" aria-current="page" class="flex items-center gap-3 rounded-lg bg-gray-100 dark:bg-gray-800 px-3 py-2 text-sm font-medium text-gray-900 dark:text-white""#
    } else {
        r#" class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800 hover:text-gray-900 dark:hover:text-white""#
    };
    let jobs_active = if active == "jobs" {
        r#" aria-current="page" class="flex items-center gap-3 rounded-lg bg-gray-100 dark:bg-gray-800 px-3 py-2 text-sm font-medium text-gray-900 dark:text-white""#
    } else {
        r#" class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800 hover:text-gray-900 dark:hover:text-white""#
    };
    format!(
        r#"<nav class="flex w-48 shrink-0 flex-col gap-1 border-r border-gray-200 dark:border-gray-800 pr-4">
          <a href="/?view=assets"{}>Assets</a>
          <a href="/?view=jobs"{}>Jobs</a>
          <div class="mt-4 border-t border-gray-200 dark:border-gray-800 pt-4 flex flex-col gap-1">
            <button type="button" data-on:click="@post('/reindex')" class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800 hover:text-gray-900 dark:hover:text-white text-left">Reindex</button>
            <button type="button" data-on:click="@post('/reset')" class="flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-800 hover:text-rose-600 dark:hover:text-rose-400 text-left">Reset</button>
          </div>
        </nav>"#,
        assets_active, jobs_active
    )
}

/// Job status icon: queued (...), running (spinner), success (green check), failed (red x)
pub fn job_status_icon(status: &str) -> String {
    match status {
        "queued" => r#"<span class="inline-flex h-6 w-6 items-center justify-center rounded-full bg-gray-100 dark:bg-gray-800 text-gray-500 dark:text-gray-400 text-xs font-medium">...</span>"#.to_string(),
        "running" => r#"<span class="inline-flex h-6 w-6 items-center justify-center"><svg class="animate-spin h-5 w-5 text-amber-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24"><circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle><path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"></path></svg></span>"#.to_string(),
        "success" => r#"<span class="inline-flex h-6 w-6 items-center justify-center rounded-full bg-emerald-100 dark:bg-emerald-900/30 text-emerald-600 dark:text-emerald-400"><svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M5 13l4 4L19 7" /></svg></span>"#.to_string(),
        "failed" => r#"<span class="inline-flex h-6 w-6 items-center justify-center rounded-full bg-rose-100 dark:bg-rose-900/30 text-rose-600 dark:text-rose-400"><svg class="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M6 18L18 6M6 6l12 12" /></svg></span>"#.to_string(),
        _ => r#"<span class="inline-flex h-6 w-6 items-center justify-center rounded-full bg-gray-100 dark:bg-gray-800 text-gray-500 dark:text-gray-400 text-xs">?</span>"#.to_string(),
    }
}

/// Jobs list for the Jobs view
pub fn jobs_list(jobs: &[(barca_core::models::MaterializationRecord, barca_core::models::AssetSummary)]) -> String {
    if jobs.is_empty() {
        return r#"<section class="mt-8"><p class="text-gray-500 dark:text-gray-400">No jobs yet.</p></section>"#.to_string();
    }
    let rows: String = jobs
        .iter()
        .map(|(mat, summary)| {
            format!(
                r#"<div class="card flex items-center gap-4 rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-4 cursor-pointer hover:border-gray-400 dark:hover:border-gray-600 transition-colors" data-on:click="$_panelOpen = true; @get('/jobs/{}/panel/stream')">
                  <span class="flex shrink-0">{}</span>
                  <div class="min-w-0 flex-1">
                    <p class="font-medium text-gray-900 dark:text-white">{}</p>
                    <p class="text-xs text-gray-500 dark:text-gray-400">Job #{} · {}</p>
                  </div>
                  <p class="text-xs text-gray-500 dark:text-gray-400">{}</p>
                </div>"#,
                mat.materialization_id,
                job_status_icon(&mat.status),
                escape_html(&summary.function_name),
                mat.materialization_id,
                escape_html(&mat.run_hash.chars().take(12).collect::<String>()),
                format_job_timestamp(mat.created_at)
            )
        })
        .collect();
    format!(
        r#"<section id="jobs-list" class="mt-8 space-y-4 max-h-[calc(100vh-15rem)] overflow-y-auto scrollbar-custom pr-1">{}</section>"#,
        rows
    )
}

fn format_job_timestamp(timestamp: i64) -> String {
    let datetime = time::OffsetDateTime::from_unix_timestamp(timestamp).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let month = match datetime.month() {
        time::Month::January => "Jan",
        time::Month::February => "Feb",
        time::Month::March => "Mar",
        time::Month::April => "Apr",
        time::Month::May => "May",
        time::Month::June => "Jun",
        time::Month::July => "Jul",
        time::Month::August => "Aug",
        time::Month::September => "Sep",
        time::Month::October => "Oct",
        time::Month::November => "Nov",
        time::Month::December => "Dec",
    };
    let hour = datetime.hour();
    let meridiem = if hour >= 12 { "PM" } else { "AM" };
    let display_hour = match hour % 12 {
        0 => 12,
        value => value,
    };
    format!("{} {}, {}:{:02} {}", month, datetime.day(), display_hour, datetime.minute(), meridiem)
}

/// Navigation bar for detail pages
pub fn detail_nav() -> &'static str {
    r#"<div class="border-b border-gray-200 dark:border-gray-800 pb-5">
                <a href="/" class="text-sm text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-gray-100 transition-colors">← Back to assets</a>
              </div>"#
}

/// Asset card component
pub fn asset_card(asset_id: i64, function_name: &str, file_path: &str, last_updated: &str, status_label: &str, status_tone: &str, is_fresh: bool) -> String {
    let button_classes = "btn-primary bg-gray-900 dark:bg-white text-white dark:text-gray-900 hover:bg-gray-800 dark:hover:bg-gray-100 focus:outline-none focus:ring-2 focus:ring-gray-900 dark:focus:ring-white focus:ring-offset-2";

    let button_action = if is_fresh {
        format!(r#"data-on:click__stop="$confirmAssetId = {}; $confirmModalOpen = true""#, asset_id)
    } else {
        format!(r#"data-on:click__stop="$_panelOpen = true; @get('/assets/{}/panel/stream?refresh=true')""#, asset_id)
    };

    format!(
        r#"<article id="asset-card-{}" class="card rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5 cursor-pointer hover:border-gray-400 dark:hover:border-gray-600 transition-colors" data-on:click="$_panelOpen = true; @get('/assets/{}/panel/stream')">
          <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
            <div class="min-w-0 flex-1">
              <p class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">{}</p>
              <h2 class="mt-2 text-2xl font-semibold tracking-tight text-gray-900 dark:text-white">{}</h2>
              <p class="mt-2 text-sm text-gray-500 dark:text-gray-400">{}</p>
            </div>
            <div class="flex shrink-0 items-center gap-3">
              <asset-status-badge label="{}" tone="{}"></asset-status-badge>
              <button type="button"
                class="inline-flex items-center justify-center rounded-lg px-3.5 py-2 text-sm font-medium {}"
                {}>Refresh</button>
            </div>
          </div>
          <div class="mt-4 border-t border-gray-100 dark:border-gray-800 pt-3">
            <p class="truncate text-xs text-gray-500 dark:text-gray-400">{}</p>
          </div>
        </article>"#,
        asset_id,
        asset_id,
        escape_html(function_name),
        escape_html(function_name),
        escape_html(last_updated),
        escape_html(status_label),
        status_tone,
        button_classes,
        button_action,
        escape_html(file_path)
    )
}

/// Definition section for asset detail page
pub fn definition_section(module_path: &str, file_path: &str, definition_hash: &str) -> String {
    format!(
        r#"<section class="mt-4 rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
                <h1 class="text-2xl font-semibold tracking-tight text-gray-900 dark:text-white">Definition</h1>
                <dl class="mt-5 grid gap-5 sm:grid-cols-2">
                  <div>
                    <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Module</dt>
                    <dd class="mt-2 text-sm text-gray-700 dark:text-gray-300">{}</dd>
                  </div>
                  <div>
                    <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">File</dt>
                    <dd class="mt-2 text-sm text-gray-700 dark:text-gray-300">{}</dd>
                  </div>
                  <div class="sm:col-span-2">
                    <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Definition hash</dt>
                    <dd class="mt-2 text-sm text-gray-700 dark:text-gray-300 break-all"><code class="bg-gray-50 dark:bg-gray-800 px-2 py-1 rounded">{}</code></dd>
                  </div>
                </dl>
              </section>"#,
        escape_html(module_path),
        escape_html(file_path),
        escape_html(definition_hash)
    )
}

/// Empty state for asset list
pub fn empty_asset_list() -> &'static str {
    r#"<article class="rounded-xl border border-dashed border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-6 py-8 text-center">
                <p class="text-2xl tracking-tight text-gray-900 dark:text-white">No assets indexed yet.</p>
                <p class="mt-2 text-sm text-gray-500 dark:text-gray-400">Run a reindex once your Python modules are available.</p>
              </article>"#
}

/// A single log line for the live job log viewer in the panel
pub fn job_log_line(message: &str, level: &str) -> String {
    let color = match level {
        "error" => "text-rose-500 dark:text-rose-400",
        "warn" => "text-amber-600 dark:text-amber-400",
        "output" => "text-sky-600 dark:text-sky-400",
        _ => "text-gray-500 dark:text-gray-400",
    };
    let prefix = match level {
        "error" => "ERR",
        "warn" => "WRN",
        "output" => "OUT",
        _ => "INF",
    };
    format!(
        r#"<div class="flex gap-2 py-0.5"><span class="shrink-0 font-mono text-[10px] font-bold {color}">{prefix}</span><span class="font-mono text-xs text-gray-700 dark:text-gray-300 break-all">{}</span></div>"#,
        escape_html(message)
    )
}

/// Log viewer section for the asset panel
pub fn job_log_viewer() -> &'static str {
    r#"<section class="mt-6 rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Live logs</h3>
            <div id="job-log-entries" class="mt-3 max-h-64 overflow-y-auto scrollbar-custom rounded-lg bg-gray-50 dark:bg-gray-950 px-3 py-2 text-xs">
            </div>
          </section>"#
}

/// Log viewer section, optionally pre-populated with persisted logs
pub fn job_log_viewer_with_logs(logs: &[barca_core::models::JobLogRecord]) -> String {
    let entries: String = logs.iter().map(|log| job_log_line(&log.message, &log.level)).collect();
    format!(
        r#"<section class="mt-6 rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Logs</h3>
            <div id="job-log-entries" class="mt-3 max-h-64 overflow-y-auto scrollbar-custom rounded-lg bg-gray-50 dark:bg-gray-950 px-3 py-2 text-xs">
              {}
            </div>
          </section>"#,
        entries
    )
}

/// Job panel content rendered inside the sliding panel
pub fn job_panel(mat: &barca_core::models::MaterializationRecord, asset: &barca_core::models::AssetSummary, persisted_logs: &[barca_core::models::JobLogRecord]) -> String {
    let status_tone = match mat.status.as_str() {
        "success" => "fresh",
        "failed" => "failed",
        "running" | "queued" => "running",
        _ => "stale",
    };

    let error_section = mat
        .last_error
        .as_deref()
        .map(|err| {
            format!(
                r#"<section class="mt-6 rounded-xl border border-rose-200 dark:border-rose-900 bg-rose-50 dark:bg-rose-950/30 px-5 py-5">
                  <h3 class="text-lg font-semibold text-rose-700 dark:text-rose-400">Error</h3>
                  <pre class="mt-3 whitespace-pre-wrap break-all text-xs text-rose-700 dark:text-rose-400">{}</pre>
                </section>"#,
                escape_html(err)
            )
        })
        .unwrap_or_default();

    let artifact_section = mat
        .artifact_path
        .as_deref()
        .map(|path| {
            format!(
                r#"<div>
                    <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Artifact</dt>
                    <dd class="mt-1 text-sm text-gray-700 dark:text-gray-300 break-all"><code class="bg-gray-50 dark:bg-gray-800 px-2 py-1 rounded">{}</code></dd>
                  </div>"#,
                escape_html(path)
            )
        })
        .unwrap_or_default();

    let is_terminal = mat.status == "success" || mat.status == "failed";
    let log_section = if is_terminal && !persisted_logs.is_empty() {
        job_log_viewer_with_logs(persisted_logs)
    } else {
        job_log_viewer().to_string()
    };

    format!(
        r#"<div id="panel-content" class="p-6">
          <div class="flex items-center justify-between gap-3 border-b border-gray-200 dark:border-gray-800 pb-4">
            <div class="flex items-center gap-3 min-w-0">
              <h2 class="text-xl font-semibold text-gray-900 dark:text-white truncate">Job #{}</h2>
              <asset-status-badge label="{}" tone="{}"></asset-status-badge>
            </div>
          </div>
          <section class="mt-6 rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Asset</h3>
            <div class="mt-3 flex items-center gap-3 cursor-pointer group" data-on:click="@get('/assets/{}/panel/stream')">
              <div class="min-w-0 flex-1">
                <p class="font-medium text-gray-900 dark:text-white group-hover:text-blue-600 dark:group-hover:text-blue-400 transition-colors">{}</p>
                <p class="text-xs text-gray-500 dark:text-gray-400">{}</p>
              </div>
              <svg class="h-4 w-4 shrink-0 text-gray-400 group-hover:text-blue-500 transition-colors" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2"><path stroke-linecap="round" stroke-linejoin="round" d="M9 5l7 7-7 7"/></svg>
            </div>
          </section>
          {}
          {}
          <section class="mt-6 rounded-xl border border-gray-200 dark:border-gray-800 bg-white dark:bg-gray-900 px-5 py-5">
            <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Details</h3>
            <dl class="mt-4 grid gap-4 sm:grid-cols-2">
              <div>
                <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Created</dt>
                <dd class="mt-1 text-sm text-gray-700 dark:text-gray-300">{}</dd>
              </div>
              <div>
                <dt class="text-[10px] font-medium uppercase tracking-[0.15em] text-gray-500 dark:text-gray-400">Run hash</dt>
                <dd class="mt-1 text-sm text-gray-700 dark:text-gray-300 break-all"><code class="bg-gray-50 dark:bg-gray-800 px-2 py-1 rounded">{}</code></dd>
              </div>
              {}
            </dl>
          </section>
        </div>"#,
        mat.materialization_id,
        escape_html(&capitalize(&mat.status)),
        status_tone,
        asset.asset_id,
        escape_html(&asset.function_name),
        escape_html(&asset.file_path),
        log_section,
        error_section,
        format_job_timestamp(mat.created_at),
        escape_html(&mat.run_hash),
        artifact_section,
    )
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().to_string() + chars.as_str(),
    }
}

/// Helper function to escape HTML
pub fn escape_html(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
