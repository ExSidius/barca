//! Database operations — schema init, output persistence, connection helpers.

use crate::BarcaError;
use crate::dispatch::OutputRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use turso::Builder;

/// Process-wide serialization of `metadata.db` operations. The server can run
/// multiple pipelines in parallel (they execute Python concurrently), but their
/// brief reads/writes to the shared SQLite file must not overlap. Every DB
/// helper — and the inline cache-check/persist ops in `commands::execute` — holds
/// this guard for the duration of its (short) database work, so runs never race
/// on the file without depending on WAL support. A one-shot CLI run leaves it
/// uncontended.
static DB_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the process-wide DB lock (recovering from a poisoned mutex — a prior
/// panic mid-op leaves the data intact for our purposes). Hold the returned
/// guard only across a single database operation; never across Python execution.
pub fn db_guard() -> MutexGuard<'static, ()> {
    DB_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Record of a single run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub command: String,
    pub files: String,
    pub target: Option<String>,
    pub status: String,
    pub steps_total: Option<i64>,
    pub steps_executed: i64,
    pub steps_cached: i64,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub elapsed_seconds: Option<f64>,
}

/// Aggregated statistics for a single asset node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetStats {
    pub node_id: String,
    pub total_runs: i64,
    pub avg_elapsed_seconds: Option<f64>,
    pub median_elapsed_seconds: Option<f64>,
    pub max_elapsed_seconds: Option<f64>,
    pub p95_elapsed_seconds: Option<f64>,
    pub cache_hit_rate: f64,
    pub recent_runs: Vec<AssetRunEntry>,
}

/// One materialization entry for asset stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRunEntry {
    pub elapsed_seconds: Option<f64>,
    pub status: String,
    pub created_at: String,
    /// Error message for `status='failed'` rows (None for successes).
    pub error_message: Option<String>,
    /// Number of attempts made.
    pub attempts: i64,
}

/// Local filesystem locations for one environment.
#[derive(Debug, Clone)]
pub struct LocalPaths {
    pub db_path: String,
    pub artifact_dir: String,
}

/// Path derivation for an environment, with no filesystem side effects.
/// The default env keeps the legacy layout so existing projects need no
/// migration; named envs live under `.barca/envs/<name>/`.
pub fn env_local_paths(env: &str) -> LocalPaths {
    let base = if env == crate::config::DEFAULT_ENV {
        PathBuf::from(".barca")
    } else {
        PathBuf::from(".barca").join("envs").join(env)
    };
    LocalPaths {
        db_path: base.join("metadata.db").to_string_lossy().to_string(),
        artifact_dir: base.join("artifacts").to_string_lossy().to_string(),
    }
}

/// Create the `.barca` tree for an environment and return its paths.
pub fn ensure_env_dirs(env: &str) -> Result<LocalPaths, BarcaError> {
    let paths = env_local_paths(env);
    let db_dir = Path::new(&paths.db_path)
        .parent()
        .expect("db path has a parent")
        .to_path_buf();
    fs::create_dir_all(&db_dir)
        .map_err(|e| BarcaError::Db(format!("failed to create {}: {e}", db_dir.display())))?;
    fs::create_dir_all(&paths.artifact_dir)
        .map_err(|e| BarcaError::Db(format!("failed to create artifacts dir: {e}")))?;
    let gitignore = PathBuf::from(".barca").join(".gitignore");
    if !gitignore.exists() {
        let _ = fs::write(&gitignore, "*\n");
    }
    Ok(paths)
}

pub fn ensure_db_dir() -> Result<String, BarcaError> {
    Ok(ensure_env_dirs(crate::config::DEFAULT_ENV)?.db_path)
}

pub fn init_db_sync(db_path: &str) -> Result<(), BarcaError> {
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS materializations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                node_id TEXT NOT NULL,
                run_hash TEXT,
                output_json TEXT,
                artifact_path TEXT,
                artifact_format TEXT,
                artifact_size_bytes INTEGER,
                elapsed_seconds REAL,
                status TEXT NOT NULL DEFAULT 'success',
                error_message TEXT,
                error_traceback TEXT,
                attempts INTEGER DEFAULT 1,
                sinks_json TEXT,
                created_at TEXT DEFAULT (datetime('now'))
            )",
            (),
        )
        .await
        .map_err(|e| BarcaError::Db(format!("failed to create materializations table: {e}")))?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mat_node_run ON materializations(node_id, run_hash)",
            (),
        )
        .await
        .map_err(|e| BarcaError::Db(format!("failed to create index: {e}")))?;

        // Migrate existing databases: add artifact columns if missing.
        // These are safe no-ops if the columns already exist.
        for col in [
            "ALTER TABLE materializations ADD COLUMN artifact_path TEXT",
            "ALTER TABLE materializations ADD COLUMN artifact_format TEXT",
            "ALTER TABLE materializations ADD COLUMN artifact_size_bytes INTEGER",
            "ALTER TABLE materializations ADD COLUMN elapsed_seconds REAL",
            "ALTER TABLE materializations ADD COLUMN error_message TEXT",
            "ALTER TABLE materializations ADD COLUMN error_traceback TEXT",
            "ALTER TABLE materializations ADD COLUMN attempts INTEGER DEFAULT 1",
            "ALTER TABLE materializations ADD COLUMN sinks_json TEXT",
            "ALTER TABLE materializations ADD COLUMN cpu_seconds REAL",
            "ALTER TABLE materializations ADD COLUMN max_rss_bytes INTEGER",
        ] {
            conn.execute(col, ()).await.ok();
        }

        // Per-node cost estimates: the persisted EWMA that seeds the next
        // run's batch sizing, so the 30s cold-start probe is paid once ever
        // per stable node, not once per run. One row per exact node id
        // (including partition suffix) — current estimate only, no history
        // vector.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS cost_estimates (
                node_id TEXT PRIMARY KEY,
                base_id TEXT NOT NULL,
                estimate_seconds REAL NOT NULL,
                cpu_seconds REAL,
                max_rss_bytes INTEGER,
                samples INTEGER DEFAULT 0,
                updated_at TEXT DEFAULT (datetime('now'))
            )",
            (),
        )
        .await
        .map_err(|e| BarcaError::Db(format!("failed to create cost_estimates table: {e}")))?;

        // Run history table.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                run_id TEXT UNIQUE NOT NULL,
                command TEXT NOT NULL,
                files TEXT NOT NULL,
                target TEXT,
                status TEXT NOT NULL DEFAULT 'running',
                steps_total INTEGER,
                steps_executed INTEGER DEFAULT 0,
                steps_cached INTEGER DEFAULT 0,
                started_at TEXT DEFAULT (datetime('now')),
                finished_at TEXT,
                elapsed_seconds REAL
            )",
            (),
        )
        .await
        .map_err(|e| BarcaError::Db(format!("failed to create runs table: {e}")))?;

        // Scheduler durability: the last time each scheduled node was fired, as
        // unix epoch seconds. Lets `barca serve` catch up a single missed tick
        // after downtime instead of silently skipping it.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS schedule_state (
                node_id TEXT PRIMARY KEY,
                last_fired_at INTEGER NOT NULL
            )",
            (),
        )
        .await
        .map_err(|e| BarcaError::Db(format!("failed to create schedule_state table: {e}")))?;
        Ok(())
    })
}

/// Load the last-fired time (unix epoch seconds) for every scheduled node.
pub fn get_schedule_state_sync(db_path: &str) -> Result<HashMap<String, i64>, BarcaError> {
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        let mut rows = conn
            .query("SELECT node_id, last_fired_at FROM schedule_state", ())
            .await
            .map_err(|e| BarcaError::Db(format!("failed to query schedule_state: {e}")))?;
        let mut out = HashMap::new();
        while let Ok(Some(row)) = rows.next().await {
            let node_id = row.get::<String>(0).unwrap_or_default();
            let last = row.get::<i64>(1).unwrap_or_default();
            if !node_id.is_empty() {
                out.insert(node_id, last);
            }
        }
        Ok(out)
    })
}

/// Record that a scheduled node fired at `epoch_secs` (unix epoch seconds).
pub fn upsert_schedule_state_sync(
    db_path: &str,
    node_id: &str,
    epoch_secs: i64,
) -> Result<(), BarcaError> {
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        conn.execute(
            "INSERT INTO schedule_state (node_id, last_fired_at) VALUES (?1, ?2)
             ON CONFLICT(node_id) DO UPDATE SET last_fired_at = ?2",
            [node_id.to_string(), epoch_secs.to_string()],
        )
        .await
        .map_err(|e| BarcaError::Db(format!("failed to upsert schedule_state: {e}")))?;
        Ok(())
    })
}

pub fn persist_outputs_sync(
    db_path: &str,
    outputs: &HashMap<String, OutputRef>,
    run_hashes: &HashMap<String, String>,
) -> Result<(), BarcaError> {
    if outputs.is_empty() {
        return Ok(());
    }
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        for (node_id, oref) in outputs {
            let run_hash = run_hashes.get(node_id).cloned().unwrap_or_default();
            let elapsed_str = oref
                .elapsed_seconds
                .map(|e| e.to_string())
                .unwrap_or_default();
            conn.execute(
                "INSERT INTO materializations (node_id, run_hash, artifact_path, artifact_format, artifact_size_bytes, elapsed_seconds) VALUES (?1, ?2, ?3, ?4, ?5, NULLIF(?6, ''))",
                [
                    node_id.clone(),
                    run_hash,
                    oref.path.clone(),
                    oref.format.clone(),
                    oref.size_bytes.to_string(),
                    elapsed_str,
                ],
            )
            .await
            .ok();
        }
        Ok(())
    })
}

/// Alias for backward compat in tests — delegates to persist_outputs_sync.
pub fn persist_output_refs_sync(db_path: &str, outputs: &HashMap<String, OutputRef>) {
    persist_outputs_sync(db_path, outputs, &HashMap::new()).ok();
}

/// Load every persisted per-node cost estimate (run start — seeds the
/// in-memory `CostModel` so batch sizing starts pre-warmed).
pub fn load_cost_estimates_sync(
    db_path: &str,
) -> Result<Vec<(String, crate::cost::NodeEstimate)>, BarcaError> {
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        let mut rows = conn
            .query(
                "SELECT node_id, estimate_seconds, cpu_seconds, max_rss_bytes, samples FROM cost_estimates",
                (),
            )
            .await
            .map_err(|e| BarcaError::Db(format!("failed to query cost_estimates: {e}")))?;
        let mut out = Vec::new();
        while let Ok(Some(row)) = rows.next().await {
            let node_id = row.get::<String>(0).unwrap_or_default();
            if node_id.is_empty() {
                continue;
            }
            out.push((
                node_id,
                crate::cost::NodeEstimate {
                    estimate_seconds: row.get::<f64>(1).unwrap_or(0.0),
                    cpu_seconds: row.get::<f64>(2).unwrap_or(0.0),
                    max_rss_bytes: row.get::<i64>(3).unwrap_or(0).max(0) as u64,
                    samples: row.get::<i64>(4).unwrap_or(0).max(0) as u64,
                },
            ));
        }
        Ok(out)
    })
}

/// Upsert per-node cost estimates (run end — persists the EWMA so the next
/// run skips the cold-start probe entirely).
pub fn upsert_cost_estimates_sync(
    db_path: &str,
    estimates: &[(String, crate::cost::NodeEstimate)],
) -> Result<(), BarcaError> {
    if estimates.is_empty() {
        return Ok(());
    }
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        for (node_id, est) in estimates {
            let base = crate::StepId::parse(node_id).base_id().to_string();
            conn.execute(
                "INSERT INTO cost_estimates (node_id, base_id, estimate_seconds, cpu_seconds, max_rss_bytes, samples, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
                 ON CONFLICT(node_id) DO UPDATE SET
                     estimate_seconds = ?3, cpu_seconds = ?4, max_rss_bytes = ?5,
                     samples = ?6, updated_at = datetime('now')",
                [
                    node_id.clone(),
                    base,
                    est.estimate_seconds.to_string(),
                    est.cpu_seconds.to_string(),
                    est.max_rss_bytes.to_string(),
                    est.samples.to_string(),
                ],
            )
            .await
            .ok();
        }
        Ok(())
    })
}

/// Generate a short run ID from timestamp + random bits (no uuid crate).
pub fn generate_run_id() -> String {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Mix in the address of a stack variable for pseudo-randomness.
    let stack_addr = &nanos as *const _ as u64;
    let mixed = nanos as u64 ^ stack_addr;
    format!("{:012x}", mixed & 0xFFFF_FFFF_FFFF)
}

/// Create a new run record at the start of execution.
pub fn create_run_sync(
    db_path: &str,
    run_id: &str,
    command: &str,
    files: &str,
    target: Option<&str>,
    steps_total: Option<usize>,
) -> Result<(), BarcaError> {
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        conn.execute(
            "INSERT INTO runs (run_id, command, files, target, status, steps_total) VALUES (?1, ?2, ?3, ?4, 'running', ?5)",
            [
                run_id.to_string(),
                command.to_string(),
                files.to_string(),
                target.unwrap_or("").to_string(),
                steps_total.map(|n| n.to_string()).unwrap_or_default(),
            ],
        )
        .await
        .ok();
        Ok(())
    })
}

/// Finalize a run record with status and stats.
pub fn finish_run_sync(
    db_path: &str,
    run_id: &str,
    status: &str,
    steps_executed: usize,
    steps_cached: usize,
    elapsed_seconds: f64,
) -> Result<(), BarcaError> {
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        conn.execute(
            "UPDATE runs SET status = ?1, steps_executed = ?2, steps_cached = ?3, elapsed_seconds = ?4, finished_at = datetime('now') WHERE run_id = ?5",
            [
                status.to_string(),
                steps_executed.to_string(),
                steps_cached.to_string(),
                elapsed_seconds.to_string(),
                run_id.to_string(),
            ],
        )
        .await
        .ok();
        Ok(())
    })
}

/// Retrieve recent run records, newest first.
pub fn get_recent_runs_sync(db_path: &str, limit: usize) -> Result<Vec<RunRecord>, BarcaError> {
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        let mut rows = conn
            .query(
                "SELECT run_id, command, files, target, status, steps_total, steps_executed, steps_cached, started_at, finished_at, elapsed_seconds FROM runs ORDER BY id DESC LIMIT ?1",
                [limit.to_string()],
            )
            .await
            .map_err(|e| BarcaError::Db(format!("failed to query runs: {e}")))?;
        let mut records = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to read row: {e}")))?
        {
            records.push(RunRecord {
                run_id: row.get::<String>(0).unwrap_or_default(),
                command: row.get::<String>(1).unwrap_or_default(),
                files: row.get::<String>(2).unwrap_or_default(),
                target: {
                    let t = row.get::<String>(3).unwrap_or_default();
                    if t.is_empty() { None } else { Some(t) }
                },
                status: row.get::<String>(4).unwrap_or_default(),
                steps_total: row.get::<i64>(5).ok(),
                steps_executed: row.get::<i64>(6).unwrap_or(0),
                steps_cached: row.get::<i64>(7).unwrap_or(0),
                started_at: row.get::<String>(8).unwrap_or_default(),
                finished_at: {
                    let t = row.get::<String>(9).unwrap_or_default();
                    if t.is_empty() { None } else { Some(t) }
                },
                elapsed_seconds: row.get::<f64>(10).ok(),
            });
        }
        Ok(records)
    })
}

/// Get aggregated stats for a specific asset/node.
pub fn get_asset_stats_sync(db_path: &str, node_id: &str) -> Result<AssetStats, BarcaError> {
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;

        // Fetch all elapsed times for percentile computation.
        let mut all_elapsed: Vec<f64> = Vec::new();
        {
            let mut rows = conn
                .query(
                    "SELECT elapsed_seconds FROM materializations WHERE node_id = ?1 AND elapsed_seconds IS NOT NULL AND elapsed_seconds > 0 ORDER BY elapsed_seconds",
                    [node_id.to_string()],
                )
                .await
                .map_err(|e| BarcaError::Db(format!("failed to query elapsed: {e}")))?;
            while let Some(row) = rows
                .next()
                .await
                .map_err(|e| BarcaError::Db(format!("failed to read row: {e}")))?
            {
                if let Ok(e) = row.get::<f64>(0) {
                    all_elapsed.push(e);
                }
            }
        }

        let total_runs: i64;
        {
            let mut rows = conn
                .query(
                    "SELECT COUNT(*) FROM materializations WHERE node_id = ?1",
                    [node_id.to_string()],
                )
                .await
                .map_err(|e| BarcaError::Db(format!("failed to query count: {e}")))?;
            total_runs = rows
                .next()
                .await
                .map_err(|e| BarcaError::Db(format!("failed to read row: {e}")))?
                .map(|r| r.get::<i64>(0).unwrap_or(0))
                .unwrap_or(0);
        }

        let avg_elapsed = if all_elapsed.is_empty() {
            None
        } else {
            Some(all_elapsed.iter().sum::<f64>() / all_elapsed.len() as f64)
        };
        let median_elapsed = percentile(&all_elapsed, 50.0);
        let max_elapsed = all_elapsed.last().copied();
        let p95_elapsed = percentile(&all_elapsed, 95.0);

        // Cache hit rate: rows with non-null, non-empty run_hash that appear more than once.
        let cache_hit_rate = if total_runs > 1 {
            let mut rows = conn
                .query(
                    "SELECT COUNT(*) FROM materializations WHERE node_id = ?1 AND run_hash != '' AND run_hash IN (SELECT run_hash FROM materializations WHERE node_id = ?1 GROUP BY run_hash HAVING COUNT(*) > 1)",
                    [node_id.to_string()],
                )
                .await
                .map_err(|e| BarcaError::Db(format!("failed to query cache hits: {e}")))?;
            let cached_count: i64 = rows
                .next()
                .await
                .map_err(|e| BarcaError::Db(format!("failed to read row: {e}")))?
                .map(|r| r.get::<i64>(0).unwrap_or(0))
                .unwrap_or(0);
            cached_count as f64 / total_runs as f64
        } else {
            0.0
        };

        // Recent runs (last 10).
        let mut rows = conn
            .query(
                "SELECT elapsed_seconds, status, created_at, error_message, attempts FROM materializations WHERE node_id = ?1 ORDER BY id DESC LIMIT 10",
                [node_id.to_string()],
            )
            .await
            .map_err(|e| BarcaError::Db(format!("failed to query recent runs: {e}")))?;
        let mut recent_runs = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to read row: {e}")))?
        {
            recent_runs.push(AssetRunEntry {
                elapsed_seconds: row.get::<f64>(0).ok(),
                status: row.get::<String>(1).unwrap_or_else(|_| "success".to_string()),
                created_at: row.get::<String>(2).unwrap_or_default(),
                error_message: row.get::<String>(3).ok(),
                attempts: row.get::<i64>(4).unwrap_or(1),
            });
        }

        Ok(AssetStats {
            node_id: node_id.to_string(),
            total_runs,
            avg_elapsed_seconds: avg_elapsed,
            median_elapsed_seconds: median_elapsed,
            max_elapsed_seconds: max_elapsed,
            p95_elapsed_seconds: p95_elapsed,
            cache_hit_rate,
            recent_runs,
        })
    })
}

/// Compute the p-th percentile from a sorted slice of values.
fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    if sorted.len() == 1 {
        return Some(sorted[0]);
    }
    let rank = (p / 100.0) * (sorted.len() - 1) as f64;
    let lower = rank.floor() as usize;
    let upper = rank.ceil() as usize;
    if lower == upper {
        Some(sorted[lower])
    } else {
        let frac = rank - lower as f64;
        Some(sorted[lower] * (1.0 - frac) + sorted[upper] * frac)
    }
}

/// Look up the average elapsed_seconds for a list of node_ids.
/// Used for progress bar ETA estimation.
pub fn get_avg_elapsed_sync(
    db_path: &str,
    node_ids: &[String],
) -> Result<HashMap<String, f64>, BarcaError> {
    if node_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        let mut result = HashMap::new();
        for nid in node_ids {
            let mut rows = conn
                .query(
                    "SELECT AVG(elapsed_seconds) FROM materializations WHERE node_id = ?1 AND elapsed_seconds IS NOT NULL",
                    [nid.clone()],
                )
                .await
                .map_err(|e| BarcaError::Db(format!("failed to query avg elapsed: {e}")))?;
            if let Some(row) = rows
                .next()
                .await
                .map_err(|e| BarcaError::Db(format!("failed to read row: {e}")))?
                && let Ok(avg) = row.get::<f64>(0)
            {
                result.insert(nid.clone(), avg);
            }
        }
        Ok(result)
    })
}

/// Look up the average elapsed_seconds for partitioned nodes using LIKE pattern matching.
/// For base_node_ids like "file.py:fetch", matches all "file.py:fetch[%]" in the DB.
/// Used for ETA estimation when steps carry partition_keys (late expansion).
pub fn get_avg_elapsed_for_partitioned_sync(
    db_path: &str,
    base_node_ids: &[String],
) -> Result<HashMap<String, f64>, BarcaError> {
    if base_node_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let _g = db_guard();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| BarcaError::Db(format!("failed to create runtime: {e}")))?;
    rt.block_on(async {
        let db = Builder::new_local(db_path)
            .build()
            .await
            .map_err(|e| BarcaError::Db(format!("failed to open DB: {e}")))?;
        let conn = db
            .connect()
            .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;
        let mut result = HashMap::new();
        for nid in base_node_ids {
            let pattern = format!("{nid}[%]");
            let mut rows = conn
                .query(
                    "SELECT AVG(elapsed_seconds) FROM materializations WHERE node_id LIKE ?1 AND elapsed_seconds IS NOT NULL",
                    [pattern],
                )
                .await
                .map_err(|e| BarcaError::Db(format!("failed to query avg elapsed: {e}")))?;
            if let Some(row) = rows
                .next()
                .await
                .map_err(|e| BarcaError::Db(format!("failed to read row: {e}")))?
                && let Ok(avg) = row.get::<f64>(0)
            {
                result.insert(nid.clone(), avg);
            }
        }
        Ok(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_persist_and_query() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();

        init_db_sync(&db_path).unwrap();

        let mut outputs: HashMap<String, OutputRef> = HashMap::new();
        outputs.insert(
            "test.py:foo".to_string(),
            OutputRef {
                path: ".barca/artifacts/test.py--foo.json".to_string(),
                format: "json".to_string(),
                size_bytes: 15,
                elapsed_seconds: None,
            },
        );
        persist_outputs_sync(&db_path, &outputs, &HashMap::new()).unwrap();

        // Read it back.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(async {
            let db = Builder::new_local(&db_path).build().await.unwrap();
            let conn = db.connect().unwrap();
            let mut rows = conn
                .query(
                    "SELECT artifact_path, artifact_format, artifact_size_bytes FROM materializations WHERE node_id = ?1",
                    ["test.py:foo".to_string()],
                )
                .await
                .unwrap();
            rows.next().await.unwrap().map(|row| {
                (
                    row.get::<String>(0).unwrap(),
                    row.get::<String>(1).unwrap(),
                    row.get::<i64>(2).unwrap(),
                )
            })
        });

        let (path, format, size) = result.unwrap();
        assert_eq!(path, ".barca/artifacts/test.py--foo.json");
        assert_eq!(format, "json");
        assert_eq!(size, 15);
    }

    #[test]
    fn schedule_state_round_trips_and_upserts() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path).unwrap();

        // Empty to start.
        assert!(get_schedule_state_sync(&db_path).unwrap().is_empty());

        upsert_schedule_state_sync(&db_path, "test.py:daily", 1_000).unwrap();
        upsert_schedule_state_sync(&db_path, "test.py:poll", 2_000).unwrap();
        let state = get_schedule_state_sync(&db_path).unwrap();
        assert_eq!(state.get("test.py:daily"), Some(&1_000));
        assert_eq!(state.get("test.py:poll"), Some(&2_000));

        // Upsert overwrites the same node rather than inserting a duplicate.
        upsert_schedule_state_sync(&db_path, "test.py:daily", 3_000).unwrap();
        let state = get_schedule_state_sync(&db_path).unwrap();
        assert_eq!(state.len(), 2);
        assert_eq!(state.get("test.py:daily"), Some(&3_000));
    }

    #[test]
    fn cost_estimates_round_trip_and_upsert() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path).unwrap();

        assert!(load_cost_estimates_sync(&db_path).unwrap().is_empty());

        let est = |s: f64, n: u64| crate::cost::NodeEstimate {
            estimate_seconds: s,
            cpu_seconds: s * 0.9,
            max_rss_bytes: 1024,
            samples: n,
        };
        upsert_cost_estimates_sync(
            &db_path,
            &[
                ("f.py:fetch[t=A]".to_string(), est(0.25, 3)),
                ("f.py:report".to_string(), est(2.0, 1)),
            ],
        )
        .unwrap();

        let loaded = load_cost_estimates_sync(&db_path).unwrap();
        assert_eq!(loaded.len(), 2);
        let fetch = loaded
            .iter()
            .find(|(n, _)| n == "f.py:fetch[t=A]")
            .unwrap();
        assert!((fetch.1.estimate_seconds - 0.25).abs() < 1e-9);
        assert_eq!(fetch.1.samples, 3);
        assert_eq!(fetch.1.max_rss_bytes, 1024);

        // Upsert overwrites the same node rather than inserting a duplicate.
        upsert_cost_estimates_sync(&db_path, &[("f.py:report".to_string(), est(1.5, 2))]).unwrap();
        let loaded = load_cost_estimates_sync(&db_path).unwrap();
        assert_eq!(loaded.len(), 2);
        let report = loaded.iter().find(|(n, _)| n == "f.py:report").unwrap();
        assert!((report.1.estimate_seconds - 1.5).abs() < 1e-9);
    }

    #[test]
    fn schema_has_timing_columns() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let columns = rt.block_on(async {
            let db = Builder::new_local(&db_path).build().await.unwrap();
            let conn = db.connect().unwrap();
            let mut rows = conn
                .query("PRAGMA table_info(materializations)", ())
                .await
                .unwrap();
            let mut cols = Vec::new();
            while let Some(row) = rows.next().await.unwrap() {
                cols.push(row.get::<String>(1).unwrap());
            }
            cols
        });
        assert!(columns.contains(&"cpu_seconds".to_string()));
        assert!(columns.contains(&"max_rss_bytes".to_string()));
    }

    #[test]
    fn concurrent_writes_are_serialized_safely() {
        // Eight threads write runs at once. The process-wide DB lock must keep
        // them from racing on the SQLite file — all rows land, none error.
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path).unwrap();

        let mut handles = vec![];
        for i in 0..8 {
            let p = db_path.clone();
            handles.push(std::thread::spawn(move || {
                let rid = format!("run{i:02}");
                create_run_sync(&p, &rid, "get", "f.py", None, Some(1)).unwrap();
                finish_run_sync(&p, &rid, "success", 1, 0, 0.1).unwrap();
                upsert_schedule_state_sync(&p, &format!("f.py:node{i}"), i).unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(get_recent_runs_sync(&db_path, 100).unwrap().len(), 8);
        assert_eq!(get_schedule_state_sync(&db_path).unwrap().len(), 8);
    }

    // ─── OutputRef artifact persistence tests ────────────────────────────────

    #[test]
    fn round_trip_persist_output_ref() {
        use crate::dispatch::OutputRef;

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path).unwrap();

        let mut outputs: HashMap<String, OutputRef> = HashMap::new();
        outputs.insert(
            "test.py:foo".to_string(),
            OutputRef {
                path: ".barca/artifacts/test.py--foo.json".to_string(),
                format: "json".to_string(),
                size_bytes: 42,
                elapsed_seconds: None,
            },
        );

        persist_output_refs_sync(&db_path, &outputs);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let result = rt.block_on(async {
            let db = Builder::new_local(&db_path).build().await.unwrap();
            let conn = db.connect().unwrap();
            let mut rows = conn
                .query(
                    "SELECT artifact_path, artifact_format, artifact_size_bytes FROM materializations WHERE node_id = ?1",
                    ["test.py:foo".to_string()],
                )
                .await
                .unwrap();
            rows.next().await.unwrap().map(|row| {
                (
                    row.get::<String>(0).unwrap(),
                    row.get::<String>(1).unwrap(),
                    row.get::<i64>(2).unwrap(),
                )
            })
        });

        let (path, format, size) = result.unwrap();
        assert_eq!(path, ".barca/artifacts/test.py--foo.json");
        assert_eq!(format, "json");
        assert_eq!(size, 42);
    }

    #[test]
    fn persist_multiple_output_refs() {
        use crate::dispatch::OutputRef;

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path).unwrap();

        let mut outputs: HashMap<String, OutputRef> = HashMap::new();
        outputs.insert(
            "a.py:json_asset".to_string(),
            OutputRef {
                path: ".barca/artifacts/a--json_asset.json".to_string(),
                format: "json".to_string(),
                size_bytes: 100,
                elapsed_seconds: None,
            },
        );
        outputs.insert(
            "a.py:df_asset".to_string(),
            OutputRef {
                path: ".barca/artifacts/a--df_asset.parquet".to_string(),
                format: "parquet".to_string(),
                size_bytes: 8192,
                elapsed_seconds: None,
            },
        );
        outputs.insert(
            "a.py:obj_asset".to_string(),
            OutputRef {
                path: ".barca/artifacts/a--obj_asset.pkl".to_string(),
                format: "pickle".to_string(),
                size_bytes: 512,
                elapsed_seconds: None,
            },
        );

        persist_output_refs_sync(&db_path, &outputs);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let count = rt.block_on(async {
            let db = Builder::new_local(&db_path).build().await.unwrap();
            let conn = db.connect().unwrap();
            let mut rows = conn
                .query("SELECT COUNT(*) FROM materializations", ())
                .await
                .unwrap();
            rows.next()
                .await
                .unwrap()
                .map(|row| row.get::<i64>(0).unwrap())
                .unwrap()
        });

        assert_eq!(count, 3);
    }

    #[test]
    fn schema_has_artifact_columns() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let columns = rt.block_on(async {
            let db = Builder::new_local(&db_path).build().await.unwrap();
            let conn = db.connect().unwrap();
            let mut rows = conn
                .query("PRAGMA table_info(materializations)", ())
                .await
                .unwrap();
            let mut cols = Vec::new();
            while let Some(row) = rows.next().await.unwrap() {
                cols.push(row.get::<String>(1).unwrap());
            }
            cols
        });

        assert!(columns.contains(&"artifact_path".to_string()));
        assert!(columns.contains(&"artifact_format".to_string()));
        assert!(columns.contains(&"artifact_size_bytes".to_string()));
        // Error/retry tracking columns.
        assert!(columns.contains(&"error_message".to_string()));
        assert!(columns.contains(&"error_traceback".to_string()));
        assert!(columns.contains(&"attempts".to_string()));
        // Old column still exists for backward compat
        assert!(columns.contains(&"output_json".to_string()));
    }

    #[test]
    fn failed_row_round_trips_and_is_excluded_from_cache() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path).unwrap();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (status, msg, attempts, success_hits) = rt.block_on(async {
            let db = Builder::new_local(&db_path).build().await.unwrap();
            let conn = db.connect().unwrap();
            // Insert a failed materialization.
            conn.execute(
                "INSERT INTO materializations (node_id, run_hash, status, error_message, error_traceback, attempts) VALUES (?1, ?2, 'failed', ?3, ?4, ?5)",
                ["f:boom".to_string(), "rh1".to_string(), "kaboom".to_string(), "Traceback…".to_string(), "3".to_string()],
            )
            .await
            .unwrap();

            // Read it back.
            let mut rows = conn
                .query(
                    "SELECT status, error_message, attempts FROM materializations WHERE node_id = ?1",
                    ["f:boom".to_string()],
                )
                .await
                .unwrap();
            let row = rows.next().await.unwrap().unwrap();
            let status = row.get::<String>(0).unwrap();
            let msg = row.get::<String>(1).unwrap();
            let attempts = row.get::<i64>(2).unwrap();

            // The cache lookup filters on status='success' — a failed row is never served.
            let mut hit_rows = conn
                .query(
                    "SELECT artifact_path FROM materializations WHERE node_id = ?1 AND run_hash = ?2 AND status = 'success' ORDER BY id DESC LIMIT 1",
                    ["f:boom".to_string(), "rh1".to_string()],
                )
                .await
                .unwrap();
            let success_hits = hit_rows.next().await.unwrap().is_some();

            (status, msg, attempts, success_hits)
        });

        assert_eq!(status, "failed");
        assert_eq!(msg, "kaboom");
        assert_eq!(attempts, 3);
        assert!(
            !success_hits,
            "failed rows must not satisfy the cache lookup"
        );
    }

    #[test]
    fn cache_lookup_returns_output_ref() {
        use crate::dispatch::OutputRef;

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path).unwrap();

        // Persist with a run_hash.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let db = Builder::new_local(&db_path).build().await.unwrap();
            let conn = db.connect().unwrap();
            conn.execute(
                "INSERT INTO materializations (node_id, run_hash, artifact_path, artifact_format, artifact_size_bytes) VALUES (?1, ?2, ?3, ?4, ?5)",
                [
                    "test.py:foo".to_string(),
                    "abc123".to_string(),
                    ".barca/artifacts/test.py--foo.parquet".to_string(),
                    "parquet".to_string(),
                    "4096".to_string(),
                ],
            )
            .await
            .unwrap();
        });

        // Look it up by node_id + run_hash — should return OutputRef.
        let result = rt.block_on(async {
            let db = Builder::new_local(&db_path).build().await.unwrap();
            let conn = db.connect().unwrap();
            let mut rows = conn
                .query(
                    "SELECT artifact_path, artifact_format, artifact_size_bytes FROM materializations WHERE node_id = ?1 AND run_hash = ?2 ORDER BY id DESC LIMIT 1",
                    ["test.py:foo".to_string(), "abc123".to_string()],
                )
                .await
                .unwrap();
            rows.next().await.unwrap().map(|row| OutputRef {
                path: row.get::<String>(0).unwrap(),
                format: row.get::<String>(1).unwrap(),
                size_bytes: row.get::<i64>(2).unwrap() as u64,
                elapsed_seconds: None,
            })
        });

        let output_ref = result.unwrap();
        assert_eq!(output_ref.path, ".barca/artifacts/test.py--foo.parquet");
        assert_eq!(output_ref.format, "parquet");
        assert_eq!(output_ref.size_bytes, 4096);
    }
}
