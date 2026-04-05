"""MetadataStore — Turso/libSQL persistence layer."""

from __future__ import annotations

import os

import turso

from barca._hashing import now_ts
from barca._models import (
    AssetDetail,
    AssetInput,
    AssetSummary,
    EffectExecution,
    IndexedAsset,
    JobDetail,
    MaterializationRecord,
    SensorObservation,
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
CREATE TABLE IF NOT EXISTS sensor_observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id INTEGER NOT NULL,
    definition_id INTEGER NOT NULL,
    update_detected INTEGER NOT NULL,
    output_json TEXT,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS effect_executions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id INTEGER NOT NULL,
    definition_id INTEGER NOT NULL,
    status TEXT NOT NULL,
    last_error TEXT,
    created_at INTEGER NOT NULL
);
"""


def _connect(db_path: str):
    """Connect using Turso with optional remote sync."""
    turso_url = os.environ.get("BARCA_TURSO_URL")
    if turso_url:
        from turso.sync import connect as sync_connect

        return sync_connect(
            db_path,
            remote_url=turso_url,
            auth_token=os.environ.get("BARCA_TURSO_TOKEN", ""),
        )
    return turso.connect(db_path)


class MetadataStore:
    """SQLite/libSQL persistence layer for all Barca metadata (assets, definitions, materializations, observations)."""

    def __init__(self, db_path: str):
        parent = os.path.dirname(db_path)
        if parent:
            os.makedirs(parent, exist_ok=True)
        self.conn = _connect(db_path)
        self._init_schema()

    def _init_schema(self) -> None:
        for statement in _SCHEMA.strip().split(";"):
            statement = statement.strip()
            if statement:
                self.conn.execute(statement)
        self.conn.commit()
        # Migrations for kind and schedule columns
        for col, default in [("kind", "'asset'"), ("schedule", "'manual'")]:
            try:
                self.conn.execute(f"ALTER TABLE assets ADD COLUMN {col} TEXT NOT NULL DEFAULT {default}")
                self.conn.commit()
            except Exception:
                pass  # column already exists

    # ------------------------------------------------------------------
    # Assets
    # ------------------------------------------------------------------

    def upsert_indexed_asset(self, asset: IndexedAsset) -> None:
        """Insert or update an asset and its current definition. Marks prior definitions as 'historical'."""
        created_at = now_ts()
        existing_id = self._asset_id_by_continuity_key(asset.continuity_key)

        # Extract schedule from decorator metadata
        schedule_str = "manual"
        try:
            import json as _json
            meta = _json.loads(asset.decorator_metadata_json)
            schedule_str = meta.get("schedule", "manual")
        except Exception:
            pass

        if existing_id is None:
            self.conn.execute(
                "INSERT INTO assets (logical_name, continuity_key, module_path, file_path, function_name, asset_slug, kind, schedule, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (asset.logical_name, asset.continuity_key, asset.module_path, asset.file_path, asset.function_name, asset.asset_slug, asset.kind, schedule_str, created_at),
            )
            self.conn.commit()
            row = self.conn.execute("SELECT last_insert_rowid()").fetchone()
            asset_id = row[0]
        else:
            asset_id = existing_id
            self.conn.execute(
                "UPDATE assets SET logical_name = ?, module_path = ?, file_path = ?, function_name = ?, asset_slug = ?, kind = ?, schedule = ? WHERE id = ?",
                (asset.logical_name, asset.module_path, asset.file_path, asset.function_name, asset.asset_slug, asset.kind, schedule_str, asset_id),
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
        """Return all assets with their current definition and latest materialization status."""
        rows = self.conn.execute(
            """SELECT a.id, a.logical_name, a.module_path, a.file_path, a.function_name,
                      d.definition_hash, a.kind, a.schedule
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
                kind=row[6] or "asset",
                module_path=row[2],
                file_path=row[3],
                function_name=row[4],
                definition_hash=row[5],
                schedule=row[7] or "manual",
                materialization_status=latest.status if latest else None,
                materialization_run_hash=latest.run_hash if latest else None,
                materialization_created_at=latest.created_at if latest else None,
            ))
        return assets

    def asset_detail(self, asset_id: int) -> AssetDetail:
        """Return full detail for an asset. Raises ValueError if not found."""
        row = self.conn.execute(
            """SELECT a.id, a.logical_name, a.continuity_key, a.module_path, a.file_path,
                      a.function_name, a.asset_slug,
                      d.id, d.definition_hash, d.source_text, d.module_source_text,
                      d.decorator_metadata_json, d.return_type, d.serializer_kind,
                      d.python_version, d.codebase_hash, d.dependency_cone_hash, a.kind
               FROM assets a
               JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
               WHERE a.id = ?""",
            (asset_id,),
        ).fetchone()

        if row is None:
            raise ValueError(f"asset {asset_id} not found")

        kind = row[17] or "asset"
        latest_observation = self.latest_sensor_observation(asset_id) if kind == "sensor" else None
        latest_execution = self.latest_effect_execution(asset_id) if kind == "effect" else None

        return AssetDetail(
            asset=IndexedAsset(
                asset_id=row[0],
                logical_name=row[1],
                continuity_key=row[2],
                module_path=row[3],
                file_path=row[4],
                function_name=row[5],
                asset_slug=row[6],
                kind=kind,
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
            latest_observation=latest_observation,
            latest_execution=latest_execution,
        )

    def asset_id_by_logical_name(self, logical_name: str) -> int | None:
        """Look up an asset ID by its logical name. Returns None if not found."""
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
        """Create a new materialization record with status 'queued'. Returns the materialization ID."""
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
        """Create queued materialization records for multiple partition keys. Returns the count."""
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
        """Find a successful materialization matching the exact run_hash (cache lookup). Returns None on miss."""
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
        """Find a queued or running materialization for this run_hash. Returns None if none active."""
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
        """Find any queued or running materialization for this asset. Returns None if none active."""
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
        """Return the most recent successful materialization for an asset, or None."""
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
        """Transition a materialization to 'success' and record its artifact metadata."""
        self.conn.execute(
            "UPDATE materializations SET status = 'success', artifact_path = ?, artifact_format = ?, artifact_checksum = ? WHERE id = ?",
            (artifact_path, artifact_format, artifact_checksum, materialization_id),
        )
        self.conn.commit()

    def mark_materialization_failed(
        self, materialization_id: int, error: str,
    ) -> None:
        """Transition a materialization to 'failed' and record the error message."""
        self.conn.execute(
            "UPDATE materializations SET status = 'failed', last_error = ? WHERE id = ?",
            (error, materialization_id),
        )
        self.conn.commit()

    def update_materialization_run_hash(
        self, materialization_id: int, run_hash: str,
    ) -> None:
        """Update the run_hash on a materialization (used when upstream IDs become known after queuing)."""
        self.conn.execute(
            "UPDATE materializations SET run_hash = ? WHERE id = ?",
            (run_hash, materialization_id),
        )
        self.conn.commit()

    def count_pending_materializations(self, asset_id: int) -> int:
        """Count queued or running materializations for an asset."""
        row = self.conn.execute(
            "SELECT COUNT(*) FROM materializations WHERE asset_id = ? AND status IN ('queued', 'running')",
            (asset_id,),
        ).fetchone()
        return row[0]

    def list_recent_materializations(self, limit: int = 50) -> list[tuple[MaterializationRecord, AssetSummary]]:
        """Return recent materializations paired with their asset summaries, ordered by newest first."""
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
        """Return a single materialization with its asset summary. Raises ValueError if not found."""
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
        """Return all declared input dependencies for a definition, ordered by parameter name."""
        rows = self.conn.execute(
            "SELECT parameter_name, upstream_asset_ref, upstream_asset_id FROM asset_inputs WHERE definition_id = ? ORDER BY parameter_name",
            (definition_id,),
        ).fetchall()
        return [
            AssetInput(parameter_name=r[0], upstream_asset_ref=r[1], upstream_asset_id=r[2])
            for r in rows
        ]

    def upsert_asset_inputs(self, definition_id: int, inputs: list[AssetInput]) -> None:
        """Replace all input declarations for a definition (delete + insert)."""
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
        """Record which upstream materializations were consumed by a materialization."""
        for inp in inputs:
            self.conn.execute(
                "INSERT INTO materialization_inputs (materialization_id, parameter_name, upstream_materialization_id, upstream_asset_id) VALUES (?, ?, ?, ?)",
                (materialization_id, inp["parameter_name"], inp["upstream_materialization_id"], inp["upstream_asset_id"]),
            )
        self.conn.commit()

    # ------------------------------------------------------------------
    # Sensor observations
    # ------------------------------------------------------------------

    def insert_sensor_observation(
        self, asset_id: int, definition_id: int, update_detected: bool,
        output_json: str | None = None,
    ) -> int:
        """Record a sensor observation. Returns the observation ID."""
        self.conn.execute(
            "INSERT INTO sensor_observations (asset_id, definition_id, update_detected, output_json, created_at) VALUES (?, ?, ?, ?, ?)",
            (asset_id, definition_id, 1 if update_detected else 0, output_json, now_ts()),
        )
        self.conn.commit()
        row = self.conn.execute("SELECT last_insert_rowid()").fetchone()
        return row[0]

    def latest_sensor_observation(self, asset_id: int) -> SensorObservation | None:
        """Return the most recent sensor observation, or None if the sensor has never run."""
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, update_detected, output_json, created_at
               FROM sensor_observations WHERE asset_id = ?
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id,),
        ).fetchone()
        if row is None:
            return None
        return SensorObservation(
            observation_id=row[0], asset_id=row[1], definition_id=row[2],
            update_detected=bool(row[3]), output_json=row[4], created_at=row[5],
        )

    def list_sensor_observations(self, asset_id: int, limit: int = 50) -> list[SensorObservation]:
        """Return recent sensor observations ordered by newest first."""
        rows = self.conn.execute(
            """SELECT id, asset_id, definition_id, update_detected, output_json, created_at
               FROM sensor_observations WHERE asset_id = ?
               ORDER BY created_at DESC, id DESC LIMIT ?""",
            (asset_id, limit),
        ).fetchall()
        return [
            SensorObservation(
                observation_id=r[0], asset_id=r[1], definition_id=r[2],
                update_detected=bool(r[3]), output_json=r[4], created_at=r[5],
            )
            for r in rows
        ]

    # ------------------------------------------------------------------
    # Effect executions
    # ------------------------------------------------------------------

    def insert_effect_execution(
        self, asset_id: int, definition_id: int, status: str,
        last_error: str | None = None,
    ) -> int:
        """Record an effect execution ('success' or 'failed'). Returns the execution ID."""
        self.conn.execute(
            "INSERT INTO effect_executions (asset_id, definition_id, status, last_error, created_at) VALUES (?, ?, ?, ?, ?)",
            (asset_id, definition_id, status, last_error, now_ts()),
        )
        self.conn.commit()
        row = self.conn.execute("SELECT last_insert_rowid()").fetchone()
        return row[0]

    def latest_effect_execution(self, asset_id: int) -> EffectExecution | None:
        """Return the most recent effect execution, or None if never executed."""
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, status, last_error, created_at
               FROM effect_executions WHERE asset_id = ?
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id,),
        ).fetchone()
        if row is None:
            return None
        return EffectExecution(
            execution_id=row[0], asset_id=row[1], definition_id=row[2],
            status=row[3], last_error=row[4], created_at=row[5],
        )

    # ------------------------------------------------------------------
    # Materialization queries (notebook helpers)
    # ------------------------------------------------------------------

    def list_materializations(self, asset_id: int, limit: int = 50) -> list[MaterializationRecord]:
        """Return all materializations for an asset ordered by newest first."""
        rows = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, created_at
               FROM materializations WHERE asset_id = ?
               ORDER BY created_at DESC, id DESC LIMIT ?""",
            (asset_id, limit),
        ).fetchall()
        return [_mat_from_row(r) for r in rows]

    def latest_successful_materialization_for_partition(
        self, asset_id: int, partition_key_json: str,
    ) -> MaterializationRecord | None:
        """Return the most recent successful materialization for a specific partition key, or None."""
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, created_at
               FROM materializations
               WHERE asset_id = ? AND status = 'success' AND partition_key_json = ?
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id, partition_key_json),
        ).fetchone()
        return _mat_from_row(row) if row else None

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
