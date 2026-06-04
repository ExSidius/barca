//! Database operations — schema init, output persistence, connection helpers.

use crate::dispatch::OutputRef;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use turso::Builder;

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
        ] {
            conn.execute(col, ()).await.ok();
        }
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
            conn.execute(
                "INSERT INTO materializations (node_id, run_hash, artifact_path, artifact_format, artifact_size_bytes) VALUES (?1, ?2, ?3, ?4, ?5)",
                [
                    node_id.clone(),
                    run_hash,
                    oref.path.clone(),
                    oref.format.clone(),
                    oref.size_bytes.to_string(),
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
            },
        );
        outputs.insert(
            "a.py:df_asset".to_string(),
            OutputRef {
                path: ".barca/artifacts/a--df_asset.parquet".to_string(),
                format: "parquet".to_string(),
                size_bytes: 8192,
            },
        );
        outputs.insert(
            "a.py:obj_asset".to_string(),
            OutputRef {
                path: ".barca/artifacts/a--obj_asset.pkl".to_string(),
                format: "pickle".to_string(),
                size_bytes: 512,
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
            })
        });

        let output_ref = result.unwrap();
        assert_eq!(output_ref.path, ".barca/artifacts/test.py--foo.parquet");
        assert_eq!(output_ref.format, "parquet");
        assert_eq!(output_ref.size_bytes, 4096);
    }
}
