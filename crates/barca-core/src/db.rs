//! Database operations — schema init, output persistence, connection helpers.

use crate::dispatch::OutputRef;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use turso::Builder;

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
    pub cache_hit_rate: f64,
    pub recent_runs: Vec<AssetRunEntry>,
}

/// One materialization entry for asset stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetRunEntry {
    pub elapsed_seconds: Option<f64>,
    pub status: String,
    pub created_at: String,
}

pub fn ensure_db_dir() -> String {
    let db_dir = PathBuf::from(".barca");
    fs::create_dir_all(&db_dir).ok();
    fs::create_dir_all(db_dir.join("artifacts")).ok();
    db_dir.join("metadata.db").to_string_lossy().to_string()
}

pub fn init_db_sync(db_path: &str) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let db = Builder::new_local(db_path).build().await.unwrap();
        let conn = db.connect().unwrap();
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
                created_at TEXT DEFAULT (datetime('now'))
            )",
            (),
        )
        .await
        .unwrap();
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_mat_node_run ON materializations(node_id, run_hash)",
            (),
        )
        .await
        .unwrap();

        // Migrate existing databases: add artifact columns if missing.
        // These are safe no-ops if the columns already exist.
        for col in [
            "ALTER TABLE materializations ADD COLUMN artifact_path TEXT",
            "ALTER TABLE materializations ADD COLUMN artifact_format TEXT",
            "ALTER TABLE materializations ADD COLUMN artifact_size_bytes INTEGER",
            "ALTER TABLE materializations ADD COLUMN elapsed_seconds REAL",
        ] {
            conn.execute(col, ()).await.ok();
        }

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
        .unwrap();
    });
}

pub fn persist_outputs_sync(
    db_path: &str,
    outputs: &HashMap<String, OutputRef>,
    run_hashes: &HashMap<String, String>,
) {
    if outputs.is_empty() {
        return;
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let db = Builder::new_local(db_path).build().await.unwrap();
        let conn = db.connect().unwrap();
        for (node_id, oref) in outputs {
            let run_hash = run_hashes.get(node_id).cloned().unwrap_or_default();
            let elapsed_str = oref
                .elapsed_seconds
                .map(|e| e.to_string())
                .unwrap_or_default();
            conn.execute(
                "INSERT INTO materializations (node_id, run_hash, artifact_path, artifact_format, artifact_size_bytes, elapsed_seconds) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
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
    });
}

/// Alias for backward compat in tests — delegates to persist_outputs_sync.
pub fn persist_output_refs_sync(db_path: &str, outputs: &HashMap<String, OutputRef>) {
    persist_outputs_sync(db_path, outputs, &HashMap::new());
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
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let db = Builder::new_local(db_path).build().await.unwrap();
        let conn = db.connect().unwrap();
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
    });
}

/// Finalize a run record with status and stats.
pub fn finish_run_sync(
    db_path: &str,
    run_id: &str,
    status: &str,
    steps_executed: usize,
    steps_cached: usize,
    elapsed_seconds: f64,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let db = Builder::new_local(db_path).build().await.unwrap();
        let conn = db.connect().unwrap();
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
    });
}

/// Retrieve recent run records, newest first.
pub fn get_recent_runs_sync(db_path: &str, limit: usize) -> Vec<RunRecord> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let db = Builder::new_local(db_path).build().await.unwrap();
        let conn = db.connect().unwrap();
        let mut rows = conn
            .query(
                "SELECT run_id, command, files, target, status, steps_total, steps_executed, steps_cached, started_at, finished_at, elapsed_seconds FROM runs ORDER BY id DESC LIMIT ?1",
                [limit.to_string()],
            )
            .await
            .unwrap();
        let mut records = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
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
        records
    })
}

/// Get aggregated stats for a specific asset/node.
pub fn get_asset_stats_sync(db_path: &str, node_id: &str) -> AssetStats {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let db = Builder::new_local(db_path).build().await.unwrap();
        let conn = db.connect().unwrap();

        // Aggregate: count, avg elapsed, cache hits.
        let total_runs: i64;
        let avg_elapsed: Option<f64>;
        {
            let mut rows = conn
                .query(
                    "SELECT COUNT(*), AVG(elapsed_seconds) FROM materializations WHERE node_id = ?1",
                    [node_id.to_string()],
                )
                .await
                .unwrap();
            if let Some(row) = rows.next().await.unwrap() {
                total_runs = row.get::<i64>(0).unwrap_or(0);
                avg_elapsed = row.get::<f64>(1).ok();
            } else {
                total_runs = 0;
                avg_elapsed = None;
            }
        }

        // Cache hit rate: rows with non-null, non-empty run_hash that appear more than once.
        let cache_hit_rate = if total_runs > 1 {
            let mut rows = conn
                .query(
                    "SELECT COUNT(*) FROM materializations WHERE node_id = ?1 AND run_hash != '' AND run_hash IN (SELECT run_hash FROM materializations WHERE node_id = ?1 GROUP BY run_hash HAVING COUNT(*) > 1)",
                    [node_id.to_string()],
                )
                .await
                .unwrap();
            let cached_count: i64 = rows.next().await.unwrap()
                .map(|r| r.get::<i64>(0).unwrap_or(0))
                .unwrap_or(0);
            cached_count as f64 / total_runs as f64
        } else {
            0.0
        };

        // Recent runs (last 10).
        let mut rows = conn
            .query(
                "SELECT elapsed_seconds, status, created_at FROM materializations WHERE node_id = ?1 ORDER BY id DESC LIMIT 10",
                [node_id.to_string()],
            )
            .await
            .unwrap();
        let mut recent_runs = Vec::new();
        while let Some(row) = rows.next().await.unwrap() {
            recent_runs.push(AssetRunEntry {
                elapsed_seconds: row.get::<f64>(0).ok(),
                status: row.get::<String>(1).unwrap_or_else(|_| "success".to_string()),
                created_at: row.get::<String>(2).unwrap_or_default(),
            });
        }

        AssetStats {
            node_id: node_id.to_string(),
            total_runs,
            avg_elapsed_seconds: avg_elapsed,
            cache_hit_rate,
            recent_runs,
        }
    })
}

/// Look up the average elapsed_seconds for a list of node_ids.
/// Used for progress bar ETA estimation.
pub fn get_avg_elapsed_sync(db_path: &str, node_ids: &[String]) -> HashMap<String, f64> {
    if node_ids.is_empty() {
        return HashMap::new();
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let db = Builder::new_local(db_path).build().await.unwrap();
        let conn = db.connect().unwrap();
        let mut result = HashMap::new();
        for nid in node_ids {
            let mut rows = conn
                .query(
                    "SELECT AVG(elapsed_seconds) FROM materializations WHERE node_id = ?1 AND elapsed_seconds IS NOT NULL",
                    [nid.clone()],
                )
                .await
                .unwrap();
            if let Some(row) = rows.next().await.unwrap() {
                if let Ok(avg) = row.get::<f64>(0) {
                    result.insert(nid.clone(), avg);
                }
            }
        }
        result
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_persist_and_query() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();

        init_db_sync(&db_path);

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
        persist_outputs_sync(&db_path, &outputs, &HashMap::new());

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

    // ─── OutputRef artifact persistence tests ────────────────────────────────

    #[test]
    fn round_trip_persist_output_ref() {
        use crate::dispatch::OutputRef;

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path);

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
        init_db_sync(&db_path);

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
        init_db_sync(&db_path);

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
        // Old column still exists for backward compat
        assert!(columns.contains(&"output_json".to_string()));
    }

    #[test]
    fn cache_lookup_returns_output_ref() {
        use crate::dispatch::OutputRef;

        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();
        init_db_sync(&db_path);

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
