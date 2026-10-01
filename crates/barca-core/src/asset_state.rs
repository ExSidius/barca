//! Asset-state dry run — for every node, would `barca get` reuse a cached
//! artifact, recompute it, or is that unknowable before running?
//!
//! Computes the exact cache keys the executor would (via
//! [`crate::cache::assign_run_hashes`], the same code path) and looks them up
//! in the metadata DB. Nothing executes.
//!
//! Strictly read-only: the DB is never opened in place. Its file (and WAL) are
//! copied into a temp dir and the copy is queried, so the dry run can't create,
//! migrate, or checkpoint a database another process is writing to.

use crate::{BarcaError, NodeKind, StepId, cache, commands, planner};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Would `barca get` reuse this node's cached artifact?
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CacheState {
    /// A successful materialization matches the current cache key — `get`
    /// would reuse it.
    Fresh,
    /// Materialized before, but not under the current cache key — `get` would
    /// recompute it.
    Stale { cause: StaleCause },
    /// Never successfully materialized.
    Missing,
    /// Tasks and sensors are never cached — they run every time.
    AlwaysRuns,
    /// Statically-partitioned assets are not cache-checked — they re-run.
    Partitioned,
    /// Depends on dynamic partitions (`partitions_from`), whose keys only exist
    /// at run time — the cache key can't be computed ahead of a run.
    Unknown,
}

/// Why a [`CacheState::Stale`] node's cache key changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum StaleCause {
    /// Every upstream is fresh, so the node's own code (or its cone) changed.
    Code,
    /// At least one upstream will be recomputed.
    Upstream,
}

/// The most recent materialization attempt for a node (success or failure),
/// regardless of whether it matches the current cache key.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct LastMaterialization {
    /// `success` or `failed`.
    pub status: String,
    /// When the attempt was recorded (UTC, `YYYY-MM-DD HH:MM:SS`).
    pub created_at: String,
    pub elapsed_seconds: Option<f64>,
    pub error_message: Option<String>,
}

/// One node's state.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AssetState {
    /// Stable node id, e.g. `pipeline.py:fetch`.
    pub id: String,
    pub kind: NodeKind,
    pub cache: CacheState,
    pub last: Option<LastMaterialization>,
}

/// Compute every node's [`AssetState`], in topological order.
pub async fn asset_states(
    cfg: &crate::config::ResolvedConfig,
    file_args: &[String],
) -> Result<Vec<AssetState>, BarcaError> {
    // DAG analysis is pure static parsing — the interpreter path is unused.
    let dag = commands::build_dag(file_args, &PathBuf::from("python3")).await?;
    let plan = planner::plan_from_dag(
        &dag,
        &planner::ResourceConfig {
            pool_size: 1,
            concurrency_groups: HashMap::new(),
        },
    );
    let mut steps: HashMap<String, planner::StreamStep> = plan
        .phases
        .into_iter()
        .flat_map(|p| p.streams)
        .flat_map(|s| s.steps)
        .map(|st| (st.step_id.base_id().to_string(), st))
        .collect();

    let history = load_history(&cfg.db_path).await?;

    let mut run_hashes: HashMap<String, String> = HashMap::new();
    let mut states: HashMap<String, CacheState> = HashMap::new();
    let mut out = Vec::new();

    // Topological order guarantees every upstream is hashed (and classified)
    // before its consumers — the only ordering `assign_run_hashes` needs.
    for id in dag.topo_order() {
        let Some(node) = dag.get_node(id) else {
            continue;
        };
        let kind = node.kind();
        let Some(mut step) = steps.remove(id) else {
            continue;
        };
        let upstream: Vec<&str> = step
            .inputs
            .values()
            .map(|u| StepId::parse(u).base_id().to_string())
            .filter_map(|u| dag.get_node(&u).map(|n| n.id.as_str()))
            .collect();

        let unknown = !step.pending_partitions.is_empty()
            || upstream
                .iter()
                .any(|u| states.get(*u) == Some(&CacheState::Unknown));

        let cache = if unknown {
            CacheState::Unknown
        } else {
            cache::assign_run_hashes(&mut step, &node.definition_hash, &mut run_hashes);
            match kind {
                NodeKind::Task | NodeKind::Sensor => CacheState::AlwaysRuns,
                _ if !step.partition_keys.is_empty() => CacheState::Partitioned,
                _ => {
                    let display = step.step_id.display();
                    let key = &step.run_hashes[&display];
                    if history.successes.contains(&(display.clone(), key.clone())) {
                        CacheState::Fresh
                    } else if !history.ever_succeeded.contains(&display) {
                        CacheState::Missing
                    } else if upstream.iter().any(|u| {
                        matches!(
                            states.get(*u),
                            Some(CacheState::Stale { .. } | CacheState::Missing)
                        )
                    }) {
                        CacheState::Stale {
                            cause: StaleCause::Upstream,
                        }
                    } else {
                        CacheState::Stale {
                            cause: StaleCause::Code,
                        }
                    }
                }
            }
        };

        states.insert(id.to_string(), cache.clone());
        out.push(AssetState {
            id: id.to_string(),
            kind,
            cache,
            last: history.latest.get(id).cloned(),
        });
    }
    Ok(out)
}

/// Materialization history, flattened for cache lookups.
#[derive(Default)]
struct History {
    /// `(node_id, run_hash)` pairs with a successful materialization.
    successes: HashSet<(String, String)>,
    /// Node ids (display ids) with at least one successful materialization.
    ever_succeeded: HashSet<String>,
    /// Latest attempt per base node id (partitions fold into their base).
    latest: HashMap<String, LastMaterialization>,
}

/// Read materialization history from a private copy of the DB. A missing DB
/// is simply empty history — and stays missing.
async fn load_history(db_path: &str) -> Result<History, BarcaError> {
    let src = Path::new(db_path);
    if !src.exists() {
        return Ok(History::default());
    }

    let snapshot = tempfile::tempdir()
        .map_err(|e| BarcaError::Db(format!("failed to create snapshot dir: {e}")))?;
    let copy = snapshot.path().join("metadata.db");
    std::fs::copy(src, &copy).map_err(|e| BarcaError::Db(format!("failed to snapshot DB: {e}")))?;
    let wal = PathBuf::from(format!("{db_path}-wal"));
    if wal.exists() {
        std::fs::copy(&wal, snapshot.path().join("metadata.db-wal"))
            .map_err(|e| BarcaError::Db(format!("failed to snapshot DB WAL: {e}")))?;
    }

    let db = turso::Builder::new_local(&copy.to_string_lossy())
        .build()
        .await
        .map_err(|e| BarcaError::Db(format!("failed to open DB snapshot: {e}")))?;
    let conn = db
        .connect()
        .map_err(|e| BarcaError::Db(format!("failed to connect: {e}")))?;

    let mut history = History::default();
    // A DB created by an older barca may predate the table entirely.
    let Ok(mut rows) = conn
        .query(
            "SELECT node_id, run_hash, status, created_at, elapsed_seconds, error_message \
             FROM materializations ORDER BY id ASC",
            (),
        )
        .await
    else {
        return Ok(history);
    };
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| BarcaError::Db(format!("failed to read materialization: {e}")))?
    {
        let node_id = row.get::<String>(0).unwrap_or_default();
        let status = row.get::<String>(2).unwrap_or_default();
        if status == "success" {
            if let Ok(run_hash) = row.get::<String>(1) {
                history.successes.insert((node_id.clone(), run_hash));
            }
            history.ever_succeeded.insert(node_id.clone());
        }
        // Rows are in insertion order, so the last write per node wins.
        history.latest.insert(
            StepId::parse(&node_id).base_id().to_string(),
            LastMaterialization {
                status,
                created_at: row.get::<String>(3).unwrap_or_default(),
                elapsed_seconds: row.get::<f64>(4).ok(),
                error_message: row.get::<String>(5).ok(),
            },
        );
    }
    Ok(history)
}
