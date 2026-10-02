//! Asset state — for every node: would `barca get` reuse its cached artifact,
//! what happened the last time it ran, and how long it usually takes.
//!
//! The cache decision is [`commands::explain`] — the same `decide_step` a real
//! run uses — so this can never disagree with what `barca get` would do. This
//! module adds what a dry run doesn't say: whether a recompute is because the
//! node was never materialized or because something changed (and what), the
//! latest attempt (failures included), and typical durations.
//!
//! Strictly read-only: everything is read from a private copy of the metadata
//! DB ([`db::DbSnapshot`]). A database another process is writing to is never
//! opened, locked, created, or checkpointed.

use crate::commands::{self, CachePolicy, StepReport};
use crate::{BarcaError, Freshness, NodeKind, StepId, db};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Successful materializations considered for typical durations.
const DURATION_WINDOW: usize = 20;

/// Would `barca get` reuse this node's cached artifact?
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CacheState {
    /// The cached artifact matches the current code and inputs — `get` reuses it.
    Fresh,
    /// Some partition keys are cached; the rest would run.
    Partial {
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        cached: usize,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        total: usize,
    },
    /// Materialized before, but not for the current code and inputs — `get`
    /// would recompute it.
    Stale { cause: StaleCause },
    /// Never successfully materialized.
    Missing,
    /// Tasks and sensors are never cached — they run every time.
    AlwaysRuns,
    /// Depends on dynamic partitions (`partitions_from`) whose source hasn't
    /// been materialized, so the keys — and the cache keys — aren't known yet.
    Unknown,
}

/// Why a [`CacheState::Stale`] node would be recomputed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum StaleCause {
    /// Every upstream is cached, so the node's own code (or code it calls) changed.
    Code,
    /// At least one upstream would be recomputed first.
    Upstream,
}

/// The most recent materialization attempt (success or failure), whether or
/// not it matches the current cache key.
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

/// Typical wall time over the most recent successful materializations.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Durations {
    pub median_seconds: f64,
    pub p95_seconds: f64,
    /// How many materializations these are computed from (at most 20).
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub samples: usize,
}

/// One node's state.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AssetState {
    /// Stable node id, e.g. `pipeline.py:fetch`.
    pub id: String,
    pub kind: NodeKind,
    /// When the node is meant to run (always / manual / a cron schedule).
    pub freshness: Freshness,
    pub cache: CacheState,
    pub last: Option<LastMaterialization>,
    pub durations: Option<Durations>,
}

/// Compute every node's [`AssetState`], in topological order.
pub async fn asset_states(
    cfg: &crate::config::ResolvedConfig,
    file_args: &[String],
    python: &PathBuf,
) -> Result<Vec<AssetState>, BarcaError> {
    let snapshot = db::DbSnapshot::take(&cfg.db_path).await?;
    // Point the dry run at the copy. With no DB there is no copy: point it at
    // a path that doesn't exist, which `explain` treats as "nothing cached"
    // without creating anything.
    let scratch = tempfile::tempdir()
        .map_err(|e| BarcaError::Db(format!("failed to create scratch dir: {e}")))?;
    let mut snap_cfg = cfg.clone();
    snap_cfg.db_path = match &snapshot {
        Some(s) => s.path().to_string(),
        None => scratch.path().join("metadata.db").display().to_string(),
    };

    let explained = commands::explain(
        &snap_cfg,
        None,
        file_args,
        python,
        CachePolicy::CacheAware,
        false,
        "get",
    )
    .await?;
    let history = match &snapshot {
        Some(s) => load_history(s.path()).await?,
        None => History::default(),
    };
    let reports: HashMap<&str, &StepReport> =
        explained.steps.iter().map(|r| (r.id.as_str(), r)).collect();

    let dag = commands::build_dag(file_args, python).await?;
    let mut states: HashMap<String, CacheState> = HashMap::new();
    let mut out = Vec::new();
    // Topological order: every upstream is classified before its consumers,
    // which is what stale-cause attribution needs.
    for id in dag.topo_order() {
        let Some(node) = dag.get_node(id) else {
            continue;
        };
        let cache = match reports.get(id) {
            Some(report) => classify(report, &dag.upstream(id), &states, &history, id),
            None => CacheState::Unknown,
        };
        states.insert(id.to_string(), cache.clone());
        out.push(AssetState {
            id: id.to_string(),
            kind: node.kind(),
            freshness: node.extracted.freshness.clone(),
            cache,
            last: history.latest.get(id).cloned(),
            durations: history.durations(id),
        });
    }
    Ok(out)
}

/// Turn one dry-run decision into a [`CacheState`].
fn classify(
    report: &StepReport,
    upstream: &[&str],
    states: &HashMap<String, CacheState>,
    history: &History,
    id: &str,
) -> CacheState {
    match report.action.as_deref() {
        Some("cached") => CacheState::Fresh,
        Some("partial") => {
            let p = report.partitions.as_ref();
            CacheState::Partial {
                cached: p.map_or(0, |p| p.cached),
                total: p.map_or(0, |p| p.total),
            }
        }
        Some("unknown") => CacheState::Unknown,
        _ => match report.reason.as_deref() {
            Some("task" | "sensor") => CacheState::AlwaysRuns,
            _ if !history.ever_succeeded.contains(id) => CacheState::Missing,
            _ => {
                let upstream_recomputes = upstream.iter().any(|u| {
                    !matches!(
                        states.get(*u),
                        None | Some(CacheState::Fresh | CacheState::AlwaysRuns)
                    )
                });
                CacheState::Stale {
                    cause: if upstream_recomputes {
                        StaleCause::Upstream
                    } else {
                        StaleCause::Code
                    },
                }
            }
        },
    }
}

/// Materialization history, keyed by base node id (partitions fold into their
/// base node).
#[derive(Default)]
struct History {
    /// Base ids with at least one successful materialization.
    ever_succeeded: HashSet<String>,
    /// Latest attempt per base id.
    latest: HashMap<String, LastMaterialization>,
    /// Wall times of successful materializations per base id, oldest first.
    elapsed: HashMap<String, Vec<f64>>,
}

impl History {
    fn durations(&self, id: &str) -> Option<Durations> {
        let all = self.elapsed.get(id)?;
        let mut recent: Vec<f64> = all[all.len().saturating_sub(DURATION_WINDOW)..].to_vec();
        if recent.is_empty() {
            return None;
        }
        recent.sort_by(f64::total_cmp);
        Some(Durations {
            median_seconds: percentile(&recent, 0.5),
            p95_seconds: percentile(&recent, 0.95),
            samples: recent.len(),
        })
    }
}

/// Nearest-rank percentile over sorted values.
fn percentile(sorted: &[f64], q: f64) -> f64 {
    let rank = (q * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

async fn load_history(db_path: &str) -> Result<History, BarcaError> {
    let mut history = History::default();
    for row in db::materialization_history(db_path).await? {
        let base = StepId::parse(&row.node_id).base_id().to_string();
        if row.status == "success" {
            history.ever_succeeded.insert(base.clone());
            if let Some(e) = row.elapsed_seconds {
                history.elapsed.entry(base.clone()).or_default().push(e);
            }
        }
        // Rows arrive in insertion order, so the last write per node wins.
        history.latest.insert(
            base,
            LastMaterialization {
                status: row.status,
                created_at: row.created_at,
                elapsed_seconds: row.elapsed_seconds,
                error_message: row.error_message,
            },
        );
    }
    Ok(history)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_nearest_rank() {
        let v = [1.0, 2.0, 3.0, 4.0, 100.0];
        assert_eq!(percentile(&v, 0.5), 3.0);
        assert_eq!(percentile(&v, 0.95), 100.0);
        assert_eq!(percentile(&[7.0], 0.95), 7.0);
    }

    #[test]
    fn durations_use_the_most_recent_window() {
        let mut h = History::default();
        // 5 old fast runs, then 20 slow ones: the window only sees the slow ones.
        let mut runs: Vec<f64> = vec![1.0; 5];
        runs.extend(vec![10.0; 20]);
        h.elapsed.insert("a".into(), runs);
        let d = h.durations("a").unwrap();
        assert_eq!(d.samples, 20);
        assert_eq!(d.median_seconds, 10.0);
        assert!(h.durations("missing").is_none());
    }
}
