//! `barca-server` — the long-running HTTP API for the barca orchestrator.
//!
//! Layering: this crate depends on `barca-core` (pure planning/execution logic)
//! and exposes it over an axum JSON API. The CLI's `barca serve` subcommand is a
//! thin caller of [`serve`]. A future UI is a separate package that consumes this
//! same HTTP API as its contract.
//!
//! Runs are async: `POST /run` returns a handle immediately and the work happens
//! in a background task; clients poll `GET /status/{run_id}`. Handlers reach core
//! through `spawn_blocking`, since the core commands build their own
//! current-thread runtime internally.

mod error;
mod handlers;
mod routes;
mod state;
mod watch;

pub use state::ServeConfig;

use state::AppState;

/// Errors raised while starting or running the server (not per-request errors).
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("server I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to build server runtime: {0}")]
    Runtime(String),
}

/// Build the API router over a fresh [`AppState`] for the given config.
///
/// Exposed for tests and for embedding the API into another server; does not
/// start a watcher or bind a socket.
pub fn app(config: ServeConfig) -> axum::Router {
    routes::router(AppState::new(config))
}

/// Start the server: build a multi-thread runtime, bind, and serve until Ctrl-C.
/// Blocking — intended to be called from the CLI.
pub fn serve(config: ServeConfig) -> Result<(), ServeError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| ServeError::Runtime(e.to_string()))?;
    rt.block_on(serve_async(config))
}

async fn serve_async(config: ServeConfig) -> Result<(), ServeError> {
    let addr = std::net::SocketAddr::new(config.host, config.port);
    let n_files = config.files.len();
    let watch = config.watch;

    let state = AppState::new(config);

    // Evict completed/failed runs older than 1 hour, checking every 5 minutes.
    tokio::spawn(handlers::evict_finished_runs(
        state.clone(),
        std::time::Duration::from_secs(300),
        std::time::Duration::from_secs(3600),
    ));

    // Dev-mode hot reload. The watcher must be held for the server's lifetime.
    let _watcher = if watch {
        match watch::spawn(state.clone()) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("[barca] watch disabled: {e}");
                None
            }
        }
    } else {
        None
    };

    let app = routes::router(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!(
        "[barca] serving on http://{addr}  ({n_files} file{}{})",
        if n_files == 1 { "" } else { "s" },
        if watch { " · watch" } else { "" },
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Resolve on Ctrl-C for graceful shutdown.
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
