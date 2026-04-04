"""MetadataStore — Turso/libSQL persistence layer."""

from __future__ import annotations

import os

import libsql_experimental as libsql

from barca._hashing import now_ts
from barca._models import (
    AssetDetail,
    AssetInput,
    AssetSummary,
    IndexedAsset,
    JobDetail,
    MaterializationRecord,
)

_SCHEMA = """
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
    codebase_hash TEXT NOT NULL DEFAULT '',
    dependency_cone_hash TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS codebase_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    codebase_hash TEXT NOT NULL UNIQUE,
    snapshot_path TEXT NOT NULL,
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
    partition_key_json TEXT,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS job_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    materialization_id INTEGER NOT NULL,
    asset_id INTEGER NOT NULL,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS asset_inputs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    definition_id INTEGER NOT NULL,
    parameter_name TEXT NOT NULL,
    upstream_asset_ref TEXT NOT NULL,
    upstream_asset_id INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS materialization_inputs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    materialization_id INTEGER NOT NULL,
    parameter_name TEXT NOT NULL,
    upstream_materialization_id INTEGER NOT NULL,
    upstream_asset_id INTEGER NOT NULL
);
"""


class MetadataStore:
    def __init__(self, db_path: str):
        parent = os.path.dirname(db_path)
        if parent:
            os.makedirs(parent, exist_ok=True)
        self.conn = libsql.connect(db_path)
        self._init_schema()

    def _init_schema(self) -> None:
        for statement in _SCHEMA.strip().split(";"):
            statement = statement.strip()
            if statement:
                self.conn.execute(statement)
        self.conn.commit()

    # ------------------------------------------------------------------
    # Assets
    # ------------------------------------------------------------------

    def upsert_indexed_asset(self, asset: IndexedAsset) -> None:
        created_at = now_ts()
        existing_id = self._asset_id_by_continuity_key(asset.continuity_key)

        if existing_id is None:
            self.conn.execute(
                "INSERT INTO assets (logical_name, continuity_key, module_path, file_path, function_name, asset_slug, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
                (asset.logical_name, asset.continuity_key, asset.module_path, asset.file_path, asset.function_name, asset.asset_slug, created_at),
            )
            self.conn.commit()
            row = self.conn.execute("SELECT last_insert_rowid()").fetchone()
            asset_id = row[0]
        else:
            asset_id = existing_id
            self.conn.execute(
                "UPDATE assets SET logical_name = ?, module_path = ?, file_path = ?, function_name = ?, asset_slug = ? WHERE id = ?",
                (asset.logical_name, asset.module_path, asset.file_path, asset.function_name, asset.asset_slug, asset_id),
            )
            self.conn.commit()

        # Mark all existing definitions as historical
        self.conn.execute(
            "UPDATE asset_definitions SET status = 'historical' WHERE asset_id = ?",
            (asset_id,),
        )
        self.conn.commit()

        # Check if this definition_hash already exists
        existing_def_id = self._definition_id_by_hash(asset_id, asset.definition_hash)
        if existing_def_id is not None:
            self.conn.execute(
                "UPDATE asset_definitions SET status = 'current' WHERE id = ?",
                (existing_def_id,),
            )
            self.conn.commit()
            return

        # Insert new definition
        self.conn.execute(
            """INSERT INTO asset_definitions
               (asset_id, definition_hash, continuity_key, source_text, module_source_text,
                decorator_metadata_json, return_type, serializer_kind, python_version,
                codebase_hash, dependency_cone_hash, status, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'current', ?)""",
            (
                asset_id, asset.definition_hash, asset.continuity_key,
                asset.source_text, asset.module_source_text,
                asset.decorator_metadata_json, asset.return_type,
                asset.serializer_kind, asset.python_version,
                asset.codebase_hash, asset.dependency_cone_hash, created_at,
            ),
        )
        self.conn.commit()

    def list_assets(self) -> list[AssetSummary]:
        rows = self.conn.execute(
            """SELECT a.id, a.logical_name, a.module_path, a.file_path, a.function_name, d.definition_hash
               FROM assets a
               JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
               ORDER BY a.logical_name ASC""",
        ).fetchall()

        assets = []
        for row in rows:
            asset_id = row[0]
            latest = self._latest_materialization(asset_id)
            assets.append(AssetSummary(
                asset_id=asset_id,
                logical_name=row[1],
                module_path=row[2],
                file_path=row[3],
                function_name=row[4],
                definition_hash=row[5],
                materialization_status=latest.status if latest else None,
                materialization_run_hash=latest.run_hash if latest else None,
                materialization_created_at=latest.created_at if latest else None,
            ))
        return assets

    def asset_detail(self, asset_id: int) -> AssetDetail:
        row = self.conn.execute(
            """SELECT a.id, a.logical_name, a.continuity_key, a.module_path, a.file_path,
                      a.function_name, a.asset_slug,
                      d.id, d.definition_hash, d.source_text, d.module_source_text,
                      d.decorator_metadata_json, d.return_type, d.serializer_kind,
                      d.python_version, d.codebase_hash, d.dependency_cone_hash
               FROM assets a
               JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
               WHERE a.id = ?""",
            (asset_id,),
        ).fetchone()

        if row is None:
            raise ValueError(f"asset {asset_id} not found")

        return AssetDetail(
            asset=IndexedAsset(
                asset_id=row[0],
                logical_name=row[1],
                continuity_key=row[2],
                module_path=row[3],
                file_path=row[4],
                function_name=row[5],
                asset_slug=row[6],
                definition_id=row[7],
                definition_hash=row[8],
                run_hash=row[8],  # placeholder — same as definition_hash
                source_text=row[9],
                module_source_text=row[10],
                decorator_metadata_json=row[11],
                return_type=row[12],
                serializer_kind=row[13],
                python_version=row[14],
                codebase_hash=row[15] or "",
                dependency_cone_hash=row[16] or "",
            ),
            latest_materialization=self._latest_materialization(asset_id),
        )

    def asset_id_by_logical_name(self, logical_name: str) -> int | None:
        row = self.conn.execute(
            "SELECT id FROM assets WHERE logical_name = ? LIMIT 1",
            (logical_name,),
        ).fetchone()
        return row[0] if row else None

    # ------------------------------------------------------------------
    # Materializations
    # ------------------------------------------------------------------

    def insert_queued_materialization(
        self, asset_id: int, definition_id: int, run_hash: str,
        partition_key_json: str | None = None,
    ) -> int:
        self.conn.execute(
            "INSERT INTO materializations (asset_id, definition_id, run_hash, status, partition_key_json, created_at) VALUES (?, ?, ?, 'queued', ?, ?)",
            (asset_id, definition_id, run_hash, partition_key_json, now_ts()),
        )
        self.conn.commit()
        row = self.conn.execute("SELECT last_insert_rowid()").fetchone()
        return row[0]

    def insert_queued_materializations_batch(
        self, asset_id: int, definition_id: int, run_hash: str,
        partition_keys: list[str],
    ) -> int:
        created_at = now_ts()
        for pk in partition_keys:
            self.conn.execute(
                "INSERT INTO materializations (asset_id, definition_id, run_hash, status, partition_key_json, created_at) VALUES (?, ?, ?, 'queued', ?, ?)",
                (asset_id, definition_id, run_hash, pk, created_at),
            )
        self.conn.commit()
        return len(partition_keys)

    def successful_materialization_for_run(
        self, asset_id: int, run_hash: str,
    ) -> MaterializationRecord | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, created_at
               FROM materializations
               WHERE asset_id = ? AND run_hash = ? AND status = 'success'
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id, run_hash),
        ).fetchone()
        return _mat_from_row(row) if row else None

    def active_materialization_for_run(
        self, asset_id: int, run_hash: str,
    ) -> MaterializationRecord | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, created_at
               FROM materializations
               WHERE asset_id = ? AND run_hash = ? AND status IN ('queued', 'running')
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id, run_hash),
        ).fetchone()
        return _mat_from_row(row) if row else None

    def active_materialization_for_asset(
        self, asset_id: int,
    ) -> MaterializationRecord | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, created_at
               FROM materializations
               WHERE asset_id = ? AND status IN ('queued', 'running')
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id,),
        ).fetchone()
        return _mat_from_row(row) if row else None

    def latest_successful_materialization(
        self, asset_id: int,
    ) -> MaterializationRecord | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, created_at
               FROM materializations
               WHERE asset_id = ? AND status = 'success'
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id,),
        ).fetchone()
        return _mat_from_row(row) if row else None

    def mark_materialization_success(
        self, materialization_id: int, artifact_path: str,
        artifact_format: str, artifact_checksum: str,
    ) -> None:
        self.conn.execute(
            "UPDATE materializations SET status = 'success', artifact_path = ?, artifact_format = ?, artifact_checksum = ? WHERE id = ?",
            (artifact_path, artifact_format, artifact_checksum, materialization_id),
        )
        self.conn.commit()

    def mark_materialization_failed(
        self, materialization_id: int, error: str,
    ) -> None:
        self.conn.execute(
            "UPDATE materializations SET status = 'failed', last_error = ? WHERE id = ?",
            (error, materialization_id),
        )
        self.conn.commit()

    def update_materialization_run_hash(
        self, materialization_id: int, run_hash: str,
    ) -> None:
        self.conn.execute(
            "UPDATE materializations SET run_hash = ? WHERE id = ?",
            (run_hash, materialization_id),
        )
        self.conn.commit()

    def count_pending_materializations(self, asset_id: int) -> int:
        row = self.conn.execute(
            "SELECT COUNT(*) FROM materializations WHERE asset_id = ? AND status IN ('queued', 'running')",
            (asset_id,),
        ).fetchone()
        return row[0]

    def list_recent_materializations(self, limit: int = 50) -> list[tuple[MaterializationRecord, AssetSummary]]:
        rows = self.conn.execute(
            """SELECT m.id, m.asset_id, m.definition_id, m.run_hash, m.status,
                      m.artifact_path, m.artifact_format, m.artifact_checksum,
                      m.last_error, m.partition_key_json, m.created_at,
                      a.id, a.logical_name, a.module_path, a.file_path, a.function_name,
                      d.definition_hash
               FROM materializations m
               JOIN assets a ON a.id = m.asset_id
               JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
               ORDER BY m.created_at DESC, m.id DESC LIMIT ?""",
            (limit,),
        ).fetchall()

        result = []
        for row in rows:
            mat = _mat_from_row(row[:11])
            aid = row[11]
            latest = self._latest_materialization(aid)
            summary = AssetSummary(
                asset_id=aid,
                logical_name=row[12],
                module_path=row[13],
                file_path=row[14],
                function_name=row[15],
                definition_hash=row[16],
                materialization_status=latest.status if latest else None,
                materialization_run_hash=latest.run_hash if latest else None,
                materialization_created_at=latest.created_at if latest else None,
            )
            result.append((mat, summary))
        return result

    def get_materialization_with_asset(
        self, materialization_id: int,
    ) -> tuple[MaterializationRecord, AssetSummary]:
        row = self.conn.execute(
            """SELECT m.id, m.asset_id, m.definition_id, m.run_hash, m.status,
                      m.artifact_path, m.artifact_format, m.artifact_checksum,
                      m.last_error, m.partition_key_json, m.created_at,
                      a.id, a.logical_name, a.module_path, a.file_path, a.function_name,
                      d.definition_hash
               FROM materializations m
               JOIN assets a ON a.id = m.asset_id
               JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
               WHERE m.id = ?""",
            (materialization_id,),
        ).fetchone()

        if row is None:
            raise ValueError(f"job {materialization_id} not found")

        mat = _mat_from_row(row[:11])
        aid = row[11]
        latest = self._latest_materialization(aid)
        summary = AssetSummary(
            asset_id=aid,
            logical_name=row[12],
            module_path=row[13],
            file_path=row[14],
            function_name=row[15],
            definition_hash=row[16],
            materialization_status=latest.status if latest else None,
            materialization_run_hash=latest.run_hash if latest else None,
            materialization_created_at=latest.created_at if latest else None,
        )
        return (mat, summary)

    # ------------------------------------------------------------------
    # Asset inputs
    # ------------------------------------------------------------------

    def get_asset_inputs(self, definition_id: int) -> list[AssetInput]:
        rows = self.conn.execute(
            "SELECT parameter_name, upstream_asset_ref, upstream_asset_id FROM asset_inputs WHERE definition_id = ? ORDER BY parameter_name",
            (definition_id,),
        ).fetchall()
        return [
            AssetInput(parameter_name=r[0], upstream_asset_ref=r[1], upstream_asset_id=r[2])
            for r in rows
        ]

    def upsert_asset_inputs(self, definition_id: int, inputs: list[AssetInput]) -> None:
        self.conn.execute(
            "DELETE FROM asset_inputs WHERE definition_id = ?",
            (definition_id,),
        )
        for inp in inputs:
            upstream_id = inp.upstream_asset_id if inp.upstream_asset_id is not None else -1
            self.conn.execute(
                "INSERT INTO asset_inputs (definition_id, parameter_name, upstream_asset_ref, upstream_asset_id) VALUES (?, ?, ?, ?)",
                (definition_id, inp.parameter_name, inp.upstream_asset_ref, upstream_id),
            )
        self.conn.commit()

    def insert_materialization_inputs(
        self, materialization_id: int,
        inputs: list[dict],
    ) -> None:
        for inp in inputs:
            self.conn.execute(
                "INSERT INTO materialization_inputs (materialization_id, parameter_name, upstream_materialization_id, upstream_asset_id) VALUES (?, ?, ?, ?)",
                (materialization_id, inp["parameter_name"], inp["upstream_materialization_id"], inp["upstream_asset_id"]),
            )
        self.conn.commit()

    # ------------------------------------------------------------------
    # Private helpers
    # ------------------------------------------------------------------

    def _latest_materialization(self, asset_id: int) -> MaterializationRecord | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, created_at
               FROM materializations
               WHERE asset_id = ?
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id,),
        ).fetchone()
        return _mat_from_row(row) if row else None

    def _asset_id_by_continuity_key(self, continuity_key: str) -> int | None:
        row = self.conn.execute(
            "SELECT id FROM assets WHERE continuity_key = ? LIMIT 1",
            (continuity_key,),
        ).fetchone()
        return row[0] if row else None

    def _definition_id_by_hash(self, asset_id: int, definition_hash: str) -> int | None:
        row = self.conn.execute(
            "SELECT id FROM asset_definitions WHERE asset_id = ? AND definition_hash = ? LIMIT 1",
            (asset_id, definition_hash),
        ).fetchone()
        return row[0] if row else None


def _mat_from_row(row) -> MaterializationRecord:
    return MaterializationRecord(
        materialization_id=row[0],
        asset_id=row[1],
        definition_id=row[2],
        run_hash=row[3],
        status=row[4],
        artifact_path=row[5],
        artifact_format=row[6],
        artifact_checksum=row[7],
        last_error=row[8],
        partition_key_json=row[9],
        created_at=row[10],
    )
