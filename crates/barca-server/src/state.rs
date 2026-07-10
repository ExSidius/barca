//! Shared server state and in-memory run tracking.

use barca_core::commands::{AssetSummary, GetResult, PlanResult};
use dashmap::DashMap;
use serde::Serialize;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use tokio::sync::Semaphore;

/// How many runs may execute concurrently by default (one per available core).
fn default_run_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

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
    /// Whether the cron scheduler fires `Schedule(...)` assets. On by default;
    /// disabled with `barca serve --no-schedule`.
    pub schedule: bool,
    /// Timezone cron expressions are evaluated in: `local` (default), `utc`, or
    /// an IANA name like `America/New_York`. Set via `--timezone`.
    pub timezone: String,
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

/// Durable, mutable view of one scheduled job, published by the scheduler and
/// read by `GET /schedule`. The volatile bits (next fire time, live run status)
/// are computed at request time from `cron` and `last_handle`.
#[derive(Clone, Debug, Serialize)]
pub struct JobStatus {
    /// Full node id (the run target).
    pub id: String,
    /// The node's cron expression.
    pub cron: String,
    /// Node kind (asset / sensor / task).
    pub kind: barca_core::NodeKind,
    /// Last time the scheduler fired this job (unix epoch seconds), if ever.
    pub last_fired: Option<i64>,
    /// Handle of the most recent run the scheduler triggered for this job.
    pub last_handle: Option<String>,
}

/// Cloneable application state shared across all axum handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServeConfig>,
    pub runs: Arc<DashMap<String, RunState>>,
    pub cache: Arc<RwLock<DagCache>>,
    /// Bounds how many runs execute concurrently. Runs execute Python in
    /// parallel; the shared metadata.db is kept race-free by a process-wide DB
    /// lock in `barca-core`, not by serializing whole runs.
    pub run_slots: Arc<Semaphore>,
    /// Live scheduler view, published by the scheduler and read by `GET /schedule`.
    pub schedule: Arc<RwLock<Vec<JobStatus>>>,
    /// Bumped by the `--watch` file watcher on every DAG invalidation, so the
    /// scheduler can re-read its job set without a restart.
    pub dag_generation: Arc<AtomicU64>,
}

impl AppState {
    pub fn new(config: ServeConfig) -> Self {
        Self {
            config: Arc::new(config),
            runs: Arc::new(DashMap::new()),
            cache: Arc::new(RwLock::new(DagCache::default())),
            run_slots: Arc::new(Semaphore::new(default_run_concurrency())),
            schedule: Arc::new(RwLock::new(Vec::new())),
            dag_generation: Arc::new(AtomicU64::new(0)),
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
