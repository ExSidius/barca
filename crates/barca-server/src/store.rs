use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context};
use barca_core::hashing::now_ts;
use barca_core::models::{AssetDetail, AssetSummary, IndexedAsset, MaterializationRecord};
use turso::{Builder, Connection, Value};

pub struct MetadataStore {
    conn: Connection,
    _db_path: PathBuf,
}

impl MetadataStore {
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create metadata directory for turso database at {}",
                    parent.display()
                )
            })?;
        }
        let db = Builder::new_local(path.to_string_lossy().as_ref())
            .build()
            .await
            .with_context(|| format!("failed to open turso database at {}", path.display()))?;
        let conn = db.connect().map_err(|err| anyhow!(err.to_string()))?;
        let store = Self {
            conn,
            _db_path: path.to_path_buf(),
        };
        store.init_schema().await?;
        Ok(store)
    }

    async fn init_schema(&self) -> anyhow::Result<()> {
        self.conn
            .execute_batch(
                r#"
                CREATE TABLE IF NOT EXISTS assets (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    logical_name TEXT NOT NULL UNIQUE,
                    continuity_key TEXT NOT NULL UNIQUE,
                    module_path TEXT NOT NULL,
                    file_path TEXT NOT NULL,
                    function_name TEXT NOT NULL,
                    asset_slug TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS asset_definitions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    asset_id INTEGER NOT NULL,
                    definition_hash TEXT NOT NULL,
                    continuity_key TEXT NOT NULL,
                    source_text TEXT NOT NULL,
                    module_source_text TEXT NOT NULL,
                    decorator_metadata_json TEXT NOT NULL,
                    return_type TEXT,
                    serializer_kind TEXT NOT NULL,
                    python_version TEXT NOT NULL,
                    uv_lock_hash TEXT,
                    status TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS materializations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    asset_id INTEGER NOT NULL,
                    definition_id INTEGER NOT NULL,
                    run_hash TEXT NOT NULL,
                    status TEXT NOT NULL,
                    artifact_path TEXT,
                    artifact_format TEXT,
                    artifact_checksum TEXT,
                    last_error TEXT,
                    created_at INTEGER NOT NULL
                );
                "#,
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(())
    }

    pub async fn upsert_indexed_asset(&self, asset: &IndexedAsset) -> anyhow::Result<()> {
        let existing_asset_id = self
            .asset_id_by_continuity_key(&asset.continuity_key)
            .await?
            .unwrap_or(-1);
        let created_at = now_ts();

        let asset_id = if existing_asset_id == -1 {
            self.conn
                .execute(
                    "INSERT INTO assets (logical_name, continuity_key, module_path, file_path, function_name, asset_slug, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    (
                        asset.logical_name.as_str(),
                        asset.continuity_key.as_str(),
                        asset.module_path.as_str(),
                        asset.file_path.as_str(),
                        asset.function_name.as_str(),
                        asset.asset_slug.as_str(),
                        created_at,
                    ),
                )
                .await
                .map_err(|err| anyhow!(err.to_string()))?;
            self.conn.last_insert_rowid()
        } else {
            self.conn
                .execute(
                    "UPDATE assets SET logical_name = ?1, module_path = ?2, file_path = ?3, function_name = ?4, asset_slug = ?5 WHERE id = ?6",
                    (
                        asset.logical_name.as_str(),
                        asset.module_path.as_str(),
                        asset.file_path.as_str(),
                        asset.function_name.as_str(),
                        asset.asset_slug.as_str(),
                        existing_asset_id,
                    ),
                )
                .await
                .map_err(|err| anyhow!(err.to_string()))?;
            existing_asset_id
        };

        self.conn
            .execute(
                "UPDATE asset_definitions SET status = 'historical' WHERE asset_id = ?1",
                (asset_id,),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;

        let existing_definition_id = self
            .definition_id_by_hash(asset_id, &asset.definition_hash)
            .await?;

        if let Some(definition_id) = existing_definition_id {
            self.conn
                .execute(
                    "UPDATE asset_definitions SET status = 'current' WHERE id = ?1",
                    (definition_id,),
                )
                .await
                .map_err(|err| anyhow!(err.to_string()))?;
            return Ok(());
        }

        self.conn
            .execute(
                "INSERT INTO asset_definitions
                 (asset_id, definition_hash, continuity_key, source_text, module_source_text, decorator_metadata_json, return_type, serializer_kind, python_version, uv_lock_hash, status, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'current', ?11)",
                (
                    asset_id,
                    asset.definition_hash.as_str(),
                    asset.continuity_key.as_str(),
                    asset.source_text.as_str(),
                    asset.module_source_text.as_str(),
                    asset.decorator_metadata_json.as_str(),
                    asset.return_type.as_deref(),
                    asset.serializer_kind.as_str(),
                    asset.python_version.as_str(),
                    asset.uv_lock_hash.as_deref(),
                    created_at,
                ),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;

        Ok(())
    }

    pub async fn list_assets(&self) -> anyhow::Result<Vec<AssetSummary>> {
        let mut rows = self
            .conn
            .query(
                r#"
                SELECT
                    a.id,
                    a.logical_name,
                    a.module_path,
                    a.file_path,
                    a.function_name,
                    d.definition_hash
                FROM assets a
                JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
                ORDER BY a.logical_name ASC
                "#,
                (),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;

        let mut assets = Vec::new();
        while let Some(row) = rows.next().await.map_err(|err| anyhow!(err.to_string()))? {
            let asset_id = int_col(&row, 0)?;
            let latest = self.latest_materialization(asset_id).await?;
            assets.push(AssetSummary {
                asset_id,
                logical_name: text_col(&row, 1)?,
                module_path: text_col(&row, 2)?,
                file_path: text_col(&row, 3)?,
                function_name: text_col(&row, 4)?,
                definition_hash: text_col(&row, 5)?,
                materialization_status: latest.as_ref().map(|item| item.status.clone()),
                materialization_run_hash: latest.as_ref().map(|item| item.run_hash.clone()),
                materialization_created_at: latest.as_ref().map(|item| item.created_at),
            });
        }
        Ok(assets)
    }

    pub async fn asset_detail(&self, asset_id: i64) -> anyhow::Result<AssetDetail> {
        let mut rows = self
            .conn
            .query(
                r#"
                SELECT
                    a.id,
                    a.logical_name,
                    a.continuity_key,
                    a.module_path,
                    a.file_path,
                    a.function_name,
                    a.asset_slug,
                    d.id,
                    d.definition_hash,
                    d.source_text,
                    d.module_source_text,
                    d.decorator_metadata_json,
                    d.return_type,
                    d.serializer_kind,
                    d.python_version,
                    d.uv_lock_hash
                FROM assets a
                JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
                WHERE a.id = ?1
                "#,
                (asset_id,),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;

        let row = rows
            .next()
            .await
            .map_err(|err| anyhow!(err.to_string()))?
            .ok_or_else(|| anyhow!("asset {asset_id} not found"))?;

        Ok(AssetDetail {
            asset: IndexedAsset {
                asset_id: int_col(&row, 0)?,
                logical_name: text_col(&row, 1)?,
                continuity_key: text_col(&row, 2)?,
                module_path: text_col(&row, 3)?,
                file_path: text_col(&row, 4)?,
                function_name: text_col(&row, 5)?,
                asset_slug: text_col(&row, 6)?,
                definition_id: int_col(&row, 7)?,
                definition_hash: text_col(&row, 8)?,
                run_hash: text_col(&row, 8)?,
                source_text: text_col(&row, 9)?,
                module_source_text: text_col(&row, 10)?,
                decorator_metadata_json: text_col(&row, 11)?,
                return_type: opt_text_col(&row, 12)?,
                serializer_kind: text_col(&row, 13)?,
                python_version: text_col(&row, 14)?,
                uv_lock_hash: opt_text_col(&row, 15)?,
            },
            latest_materialization: self.latest_materialization(asset_id).await?,
        })
    }

    pub async fn successful_materialization_for_run(
        &self,
        asset_id: i64,
        run_hash: &str,
    ) -> anyhow::Result<Option<MaterializationRecord>> {
        let mut rows = self
            .conn
            .query(
                r#"
                SELECT id, asset_id, definition_id, run_hash, status, artifact_path, artifact_format, artifact_checksum, last_error, created_at
                FROM materializations
                WHERE asset_id = ?1 AND run_hash = ?2 AND status = 'success'
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                (asset_id, run_hash),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        if let Some(row) = rows.next().await.map_err(|err| anyhow!(err.to_string()))? {
            return Ok(Some(materialization_from_row(&row)?));
        }
        Ok(None)
    }

    pub async fn active_materialization_for_run(
        &self,
        asset_id: i64,
        run_hash: &str,
    ) -> anyhow::Result<Option<MaterializationRecord>> {
        let mut rows = self
            .conn
            .query(
                r#"
                SELECT id, asset_id, definition_id, run_hash, status, artifact_path, artifact_format, artifact_checksum, last_error, created_at
                FROM materializations
                WHERE asset_id = ?1 AND run_hash = ?2 AND status IN ('queued', 'running')
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                (asset_id, run_hash),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        if let Some(row) = rows.next().await.map_err(|err| anyhow!(err.to_string()))? {
            return Ok(Some(materialization_from_row(&row)?));
        }
        Ok(None)
    }

    pub async fn insert_queued_materialization(
        &self,
        asset_id: i64,
        definition_id: i64,
        run_hash: &str,
    ) -> anyhow::Result<i64> {
        self.conn
            .execute(
                "INSERT INTO materializations (asset_id, definition_id, run_hash, status, created_at) VALUES (?1, ?2, ?3, 'queued', ?4)",
                (asset_id, definition_id, run_hash, now_ts()),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(self.conn.last_insert_rowid())
    }

    pub async fn claim_next_queued_materialization(
        &self,
    ) -> anyhow::Result<Option<MaterializationRecord>> {
        let mut rows = self
            .conn
            .query(
                r#"
                SELECT id, asset_id, definition_id, run_hash, status, artifact_path, artifact_format, artifact_checksum, last_error, created_at
                FROM materializations
                WHERE status = 'queued'
                ORDER BY created_at ASC, id ASC
                LIMIT 1
                "#,
                (),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;

        let Some(row) = rows.next().await.map_err(|err| anyhow!(err.to_string()))? else {
            return Ok(None);
        };

        let record = materialization_from_row(&row)?;
        self.conn
            .execute(
                "UPDATE materializations SET status = 'running' WHERE id = ?1",
                (record.materialization_id,),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(Some(MaterializationRecord {
            status: "running".to_string(),
            ..record
        }))
    }

    pub async fn requeue_running_materializations(&self) -> anyhow::Result<()> {
        self.conn
            .execute(
                "UPDATE materializations SET status = 'queued' WHERE status = 'running'",
                (),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(())
    }

    pub async fn mark_materialization_success(
        &self,
        materialization_id: i64,
        artifact_path: &str,
        artifact_format: &str,
        artifact_checksum: &str,
    ) -> anyhow::Result<()> {
        self.conn
            .execute(
                "UPDATE materializations SET status = 'success', artifact_path = ?1, artifact_format = ?2, artifact_checksum = ?3 WHERE id = ?4",
                (artifact_path, artifact_format, artifact_checksum, materialization_id),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(())
    }

    pub async fn mark_materialization_failed(
        &self,
        materialization_id: i64,
        error: &str,
    ) -> anyhow::Result<()> {
        self.conn
            .execute(
                "UPDATE materializations SET status = 'failed', last_error = ?1 WHERE id = ?2",
                (error, materialization_id),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(())
    }

    pub async fn list_materializations_for_asset(
        &self,
        asset_id: i64,
        limit: u32,
    ) -> anyhow::Result<Vec<MaterializationRecord>> {
        let limit = limit.min(100) as i64;
        let mut rows = self
            .conn
            .query(
                r#"
                SELECT id, asset_id, definition_id, run_hash, status, artifact_path, artifact_format, artifact_checksum, last_error, created_at
                FROM materializations
                WHERE asset_id = ?1
                ORDER BY created_at DESC
                LIMIT ?2
                "#,
                (asset_id, limit),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;

        let mut result = Vec::new();
        while let Some(row) = rows.next().await.map_err(|err| anyhow!(err.to_string()))? {
            result.push(materialization_from_row(&row)?);
        }
        Ok(result)
    }

    pub async fn list_recent_materializations(
        &self,
        limit: u32,
    ) -> anyhow::Result<Vec<(MaterializationRecord, AssetSummary)>> {
        let limit = limit.min(100) as i64;
        let mut rows = self
            .conn
            .query(
                r#"
                SELECT
                    m.id, m.asset_id, m.definition_id, m.run_hash, m.status,
                    m.artifact_path, m.artifact_format, m.artifact_checksum, m.last_error, m.created_at,
                    a.id, a.logical_name, a.module_path, a.file_path, a.function_name, d.definition_hash
                FROM materializations m
                JOIN assets a ON a.id = m.asset_id
                JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
                ORDER BY m.created_at DESC
                LIMIT ?1
                "#,
                (limit,),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;

        let mut result = Vec::new();
        while let Some(row) = rows.next().await.map_err(|err| anyhow!(err.to_string()))? {
            let mat = MaterializationRecord {
                materialization_id: int_col(&row, 0)?,
                asset_id: int_col(&row, 1)?,
                definition_id: int_col(&row, 2)?,
                run_hash: text_col(&row, 3)?,
                status: text_col(&row, 4)?,
                artifact_path: opt_text_col(&row, 5)?,
                artifact_format: opt_text_col(&row, 6)?,
                artifact_checksum: opt_text_col(&row, 7)?,
                last_error: opt_text_col(&row, 8)?,
                created_at: int_col(&row, 9)?,
            };
            let asset_id = int_col(&row, 10)?;
            let latest = self.latest_materialization(asset_id).await?;
            let summary = AssetSummary {
                asset_id,
                logical_name: text_col(&row, 11)?,
                module_path: text_col(&row, 12)?,
                file_path: text_col(&row, 13)?,
                function_name: text_col(&row, 14)?,
                definition_hash: text_col(&row, 15)?,
                materialization_status: latest.as_ref().map(|item| item.status.clone()),
                materialization_run_hash: latest.as_ref().map(|item| item.run_hash.clone()),
                materialization_created_at: latest.as_ref().map(|item| item.created_at),
            };
            result.push((mat, summary));
        }
        Ok(result)
    }

    async fn latest_materialization(
        &self,
        asset_id: i64,
    ) -> anyhow::Result<Option<MaterializationRecord>> {
        let mut rows = self
            .conn
            .query(
                r#"
                SELECT id, asset_id, definition_id, run_hash, status, artifact_path, artifact_format, artifact_checksum, last_error, created_at
                FROM materializations
                WHERE asset_id = ?1
                ORDER BY created_at DESC
                LIMIT 1
                "#,
                (asset_id,),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        if let Some(row) = rows.next().await.map_err(|err| anyhow!(err.to_string()))? {
            return Ok(Some(materialization_from_row(&row)?));
        }
        Ok(None)
    }

    async fn asset_id_by_continuity_key(
        &self,
        continuity_key: &str,
    ) -> anyhow::Result<Option<i64>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id FROM assets WHERE continuity_key = ?1 LIMIT 1",
                (continuity_key,),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        if let Some(row) = rows.next().await.map_err(|err| anyhow!(err.to_string()))? {
            return Ok(Some(int_col(&row, 0)?));
        }
        Ok(None)
    }

    async fn definition_id_by_hash(
        &self,
        asset_id: i64,
        definition_hash: &str,
    ) -> anyhow::Result<Option<i64>> {
        let mut rows = self
            .conn
            .query(
                "SELECT id FROM asset_definitions WHERE asset_id = ?1 AND definition_hash = ?2 LIMIT 1",
                (asset_id, definition_hash),
            )
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        if let Some(row) = rows.next().await.map_err(|err| anyhow!(err.to_string()))? {
            return Ok(Some(int_col(&row, 0)?));
        }
        Ok(None)
    }
}

fn materialization_from_row(row: &turso::Row) -> anyhow::Result<MaterializationRecord> {
    Ok(MaterializationRecord {
        materialization_id: int_col(row, 0)?,
        asset_id: int_col(row, 1)?,
        definition_id: int_col(row, 2)?,
        run_hash: text_col(row, 3)?,
        status: text_col(row, 4)?,
        artifact_path: opt_text_col(row, 5)?,
        artifact_format: opt_text_col(row, 6)?,
        artifact_checksum: opt_text_col(row, 7)?,
        last_error: opt_text_col(row, 8)?,
        created_at: int_col(row, 9)?,
    })
}

fn int_col(row: &turso::Row, idx: usize) -> anyhow::Result<i64> {
    match row.get_value(idx).map_err(|err| anyhow!(err.to_string()))? {
        Value::Integer(value) => Ok(value),
        other => Err(anyhow!("expected integer at {idx}, got {other:?}")),
    }
}

fn text_col(row: &turso::Row, idx: usize) -> anyhow::Result<String> {
    match row.get_value(idx).map_err(|err| anyhow!(err.to_string()))? {
        Value::Text(value) => Ok(value),
        other => Err(anyhow!("expected text at {idx}, got {other:?}")),
    }
}

fn opt_text_col(row: &turso::Row, idx: usize) -> anyhow::Result<Option<String>> {
    match row.get_value(idx).map_err(|err| anyhow!(err.to_string()))? {
        Value::Null => Ok(None),
        Value::Text(value) => Ok(Some(value)),
        other => Err(anyhow!("expected nullable text at {idx}, got {other:?}")),
    }
}
