//! Database operations — schema init, output persistence, connection helpers.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use turso::Builder;

pub fn ensure_db_dir() -> String {
    let db_dir = PathBuf::from(".barca");
    fs::create_dir_all(&db_dir).ok();
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
    });
}

pub fn persist_outputs_sync(db_path: &str, outputs: &HashMap<String, serde_json::Value>) {
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
        for (node_id, output) in outputs {
            let output_json = serde_json::to_string(output).unwrap_or_default();
            conn.execute(
                "INSERT INTO materializations (node_id, output_json) VALUES (?1, ?2)",
                [node_id.clone(), output_json],
            )
            .await
            .ok();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_persist_and_query() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db").to_string_lossy().to_string();

        init_db_sync(&db_path);

        let mut outputs = HashMap::new();
        outputs.insert("test.py:foo".to_string(), serde_json::json!({"value": 42}));
        persist_outputs_sync(&db_path, &outputs);

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
                    "SELECT output_json FROM materializations WHERE node_id = ?1",
                    ["test.py:foo".to_string()],
                )
                .await
                .unwrap();
            rows.next()
                .await
                .unwrap()
                .map(|row| row.get::<String>(0).unwrap())
        });

        assert_eq!(result.unwrap(), r#"{"value":42}"#);
    }
}
