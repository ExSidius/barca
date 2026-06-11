//! Shared server state and in-memory run tracking.

use barca_core::commands::{AssetSummary, GetResult, PlanResult};
use dashmap::DashMap;
use serde::Serialize;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

/// Configuration for a `barca serve` instance. Built by the CLI and handed to
/// [`crate::serve`].
#[derive(Clone, Debug)]
pub struct ServeConfig {
    /// Python source files that define the DAG this server operates on.
    pub files: Vec<String>,
    /// Bind address (defaults to 127.0.0.1 — local only, no auth).
    pub host: IpAddr,
    /// Bind port (default 8274).
    pub port: u16,
    /// Dev-mode hot reload: re-parse the DAG when source files change.
    /// Off by default; has no effect on the production serving path.
    pub watch: bool,
    /// Python interpreter used for execution (and dynamic-partition resolution).
    pub python: PathBuf,
}

/// Lifecycle of an async run tracked by the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Accepted, not yet started.
    Pending,
    /// Executing in a background task.
    Running,
    /// Finished successfully.
    Complete,
    /// Finished with an error.
    Failed,
}

/// In-memory record of a single run. The server-side `handle` is the polling id
/// returned by `POST /run`; the real DB run id lives inside `result` once complete.
#[derive(Clone, Debug, Serialize)]
pub struct RunState {
    /// Server-side polling handle (see `/status/{run_id}`).
    pub handle: String,
    pub status: RunStatus,
    /// Populated when `status == Complete`. Carries the DB run id, timing, output.
    pub result: Option<GetResult>,
    /// Populated when `status == Failed`.
    pub error: Option<String>,
    /// Unix epoch seconds when the run was accepted.
    pub started_at: f64,
    /// Unix epoch seconds when the run finished (success or failure).
    pub finished_at: Option<f64>,
}

/// Cached static-analysis results, invalidated by the file watcher in `--watch`
/// mode. Without `--watch` the cache simply persists for the process lifetime.
#[derive(Default)]
pub struct DagCache {
    pub assets: Option<Vec<AssetSummary>>,
    pub plan: Option<PlanResult>,
}

/// Cloneable application state shared across all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServeConfig>,
    pub runs: Arc<DashMap<String, RunState>>,
    pub cache: Arc<RwLock<DagCache>>,
    /// Serializes run execution so only one pipeline executes at a time,
    /// preventing concurrent DB writes from racing on the shared metadata.db.
    pub run_mutex: Arc<Mutex<()>>,
}

impl AppState {
    pub fn new(config: ServeConfig) -> Self {
        Self {
            config: Arc::new(config),
            runs: Arc::new(DashMap::new()),
            cache: Arc::new(RwLock::new(DagCache::default())),
            run_mutex: Arc::new(Mutex::new(())),
        }
    }
}

/// Current time as unix epoch seconds (no external date crate needed).
pub fn now_ts() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}
