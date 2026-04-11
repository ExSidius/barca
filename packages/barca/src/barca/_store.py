"""MetadataStore — Turso/libSQL persistence layer.

Schema v0.4.0 changes from v0.3.0:

- ``assets.schedule`` renamed to ``assets.freshness`` (same string encoding
  but values are ``always``/``manual``/``schedule:<cron>`` instead of
  ``always``/``manual``/``cron:<cron>``).
- ``assets.purity`` added (``pure`` | ``unsafe``).
- ``assets.parent_asset_id`` added — for sinks, points to the parent @asset.
- ``assets.sink_path``, ``assets.sink_serializer`` added — for sinks.
- ``assets.active`` added — 0 means the asset was removed from the DAG but
  history is preserved. Only ``barca prune`` deletes history.
- ``materializations.stale_inputs_used`` added.
- New tables: ``sink_executions``, ``asset_renames``.
"""

from __future__ import annotations

import os
import warnings

# turso/lib.py has a `return` inside a `finally` block (SyntaxWarning) and its
# C extension hasn't declared Py_mod_gil (RuntimeWarning on free-threaded Python).
warnings.filterwarnings("ignore", category=SyntaxWarning, module="turso")
warnings.filterwarnings("ignore", category=RuntimeWarning, message=".*GIL.*")
import turso  # noqa: E402

from barca._hashing import now_ts  # noqa: E402
from barca._models import (  # noqa: E402
    AssetDetail,
    AssetInput,
    AssetSummary,
    EffectExecution,
    IndexedAsset,
    MaterializationRecord,
    PruneResult,
    SensorObservation,
    SinkExecution,
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
    kind TEXT NOT NULL DEFAULT 'asset',
    freshness TEXT NOT NULL DEFAULT 'always',
    purity TEXT NOT NULL DEFAULT 'pure',
    parent_asset_id INTEGER,
    sink_path TEXT,
    sink_serializer TEXT,
    active INTEGER NOT NULL DEFAULT 1,
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
    stale_inputs_used INTEGER NOT NULL DEFAULT 0,
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
    upstream_asset_id INTEGER NOT NULL,
    collect_mode INTEGER NOT NULL DEFAULT 0,
    is_partition_source INTEGER NOT NULL DEFAULT 0
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
CREATE TABLE IF NOT EXISTS sink_executions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id INTEGER NOT NULL,
    definition_id INTEGER NOT NULL,
    run_hash TEXT NOT NULL,
    status TEXT NOT NULL,
    destination_path TEXT NOT NULL,
    last_error TEXT,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS asset_renames (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    asset_id INTEGER NOT NULL,
    old_continuity_key TEXT NOT NULL,
    new_continuity_key TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
"""


def _connect(db_path: str):
    turso_url = os.environ.get("BARCA_TURSO_URL")
    if turso_url:
        from turso.sync import connect as sync_connect

        conn = sync_connect(
            db_path,
            remote_url=turso_url,
            auth_token=os.environ.get("BARCA_TURSO_TOKEN", ""),
        )
    else:
        conn = turso.connect(db_path)
    conn.execute("PRAGMA busy_timeout = 10000")
    return conn


class MetadataStore:
    """SQLite/libSQL persistence layer for all Barca metadata."""

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
        self._run_migrations()
        self._validate_schema()

    def _run_migrations(self) -> None:
        """Additive column migrations. Idempotent — ALTER silently skips if present."""

        def _add(table: str, col: str, col_def: str) -> None:
            try:
                self.conn.execute(f"ALTER TABLE {table} ADD COLUMN {col} {col_def}")
                self.conn.commit()
            except Exception:
                pass

        _add("assets", "kind", "TEXT NOT NULL DEFAULT 'asset'")
        _add("assets", "freshness", "TEXT NOT NULL DEFAULT 'always'")
        _add("assets", "purity", "TEXT NOT NULL DEFAULT 'pure'")
        _add("assets", "parent_asset_id", "INTEGER")
        _add("assets", "sink_path", "TEXT")
        _add("assets", "sink_serializer", "TEXT")
        _add("assets", "active", "INTEGER NOT NULL DEFAULT 1")

        _add("asset_definitions", "codebase_hash", "TEXT NOT NULL DEFAULT ''")
        _add("asset_definitions", "dependency_cone_hash", "TEXT NOT NULL DEFAULT ''")

        _add("materializations", "partition_key_json", "TEXT")
        _add("materializations", "stale_inputs_used", "INTEGER NOT NULL DEFAULT 0")

        _add("asset_inputs", "collect_mode", "INTEGER NOT NULL DEFAULT 0")
        _add("asset_inputs", "is_partition_source", "INTEGER NOT NULL DEFAULT 0")

    def _validate_schema(self) -> None:
        expected: dict[str, list[str]] = {
            "assets": [
                "id",
                "logical_name",
                "continuity_key",
                "module_path",
                "file_path",
                "function_name",
                "asset_slug",
                "kind",
                "freshness",
                "purity",
                "parent_asset_id",
                "sink_path",
                "sink_serializer",
                "active",
                "created_at",
            ],
            "asset_definitions": [
                "id",
                "asset_id",
                "definition_hash",
                "continuity_key",
                "source_text",
                "module_source_text",
                "decorator_metadata_json",
                "return_type",
                "serializer_kind",
                "python_version",
                "codebase_hash",
                "dependency_cone_hash",
                "status",
                "created_at",
            ],
            "materializations": [
                "id",
                "asset_id",
                "definition_id",
                "run_hash",
                "status",
                "artifact_path",
                "artifact_format",
                "artifact_checksum",
                "last_error",
                "partition_key_json",
                "stale_inputs_used",
                "created_at",
            ],
            "sensor_observations": [
                "id",
                "asset_id",
                "definition_id",
                "update_detected",
                "output_json",
                "created_at",
            ],
            "effect_executions": [
                "id",
                "asset_id",
                "definition_id",
                "status",
                "last_error",
                "created_at",
            ],
            "sink_executions": [
                "id",
                "asset_id",
                "definition_id",
                "run_hash",
                "status",
                "destination_path",
                "last_error",
                "created_at",
            ],
            "asset_inputs": [
                "id",
                "definition_id",
                "parameter_name",
                "upstream_asset_ref",
                "upstream_asset_id",
                "collect_mode",
                "is_partition_source",
            ],
        }
        missing: list[str] = []
        for table, cols in expected.items():
            try:
                rows = self.conn.execute(f"PRAGMA table_info({table})").fetchall()
                present = {r[1] for r in rows}
                for col in cols:
                    if col not in present:
                        missing.append(f"  {table}.{col}")
            except Exception as exc:
                missing.append(f"  {table} (could not inspect: {exc})")

        if missing:
            col_list = "\n".join(missing)
            raise RuntimeError(f"Database schema is out of date (missing columns):\n{col_list}\n\nRun  barca reset --db  to drop and recreate the database.")

    # ------------------------------------------------------------------
    # Assets
    # ------------------------------------------------------------------

    def upsert_indexed_asset(self, asset: IndexedAsset) -> None:
        created_at = now_ts()
        existing_id = self._asset_id_by_continuity_key(asset.continuity_key)

        # Extract freshness from decorator metadata
        freshness_str = "always"
        try:
            import json as _json

            meta = _json.loads(asset.decorator_metadata_json)
            # Prefer the new "freshness" key; fall back to legacy "schedule"
            freshness_str = meta.get("freshness") or meta.get("schedule") or "always"
        except Exception:
            pass

        if existing_id is None:
            self.conn.execute(
                """INSERT OR IGNORE INTO assets
                   (logical_name, continuity_key, module_path, file_path, function_name,
                    asset_slug, kind, freshness, purity, parent_asset_id, sink_path,
                    sink_serializer, active, created_at)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)""",
                (
                    asset.logical_name,
                    asset.continuity_key,
                    asset.module_path,
                    asset.file_path,
                    asset.function_name,
                    asset.asset_slug,
                    asset.kind,
                    freshness_str,
                    asset.purity,
                    asset.parent_asset_id,
                    asset.sink_path,
                    asset.sink_serializer,
                    created_at,
                ),
            )
            self.conn.commit()
            existing_id = self._asset_id_by_continuity_key(asset.continuity_key)

        asset_id = existing_id
        if asset_id is None:
            raise RuntimeError(f"failed to upsert asset '{asset.continuity_key}' — INSERT OR IGNORE produced no row")

        self.conn.execute(
            """UPDATE assets SET
               logical_name = ?, module_path = ?, file_path = ?, function_name = ?,
               asset_slug = ?, kind = ?, freshness = ?, purity = ?, parent_asset_id = ?,
               sink_path = ?, sink_serializer = ?, active = 1
               WHERE id = ?""",
            (
                asset.logical_name,
                asset.module_path,
                asset.file_path,
                asset.function_name,
                asset.asset_slug,
                asset.kind,
                freshness_str,
                asset.purity,
                asset.parent_asset_id,
                asset.sink_path,
                asset.sink_serializer,
                asset_id,
            ),
        )
        self.conn.commit()

        # Mark prior definitions as historical
        self.conn.execute(
            "UPDATE asset_definitions SET status = 'historical' WHERE asset_id = ?",
            (asset_id,),
        )
        self.conn.commit()

        # If this definition_hash already exists, just mark it current
        existing_def_id = self._definition_id_by_hash(asset_id, asset.definition_hash)
        if existing_def_id is not None:
            self.conn.execute(
                "UPDATE asset_definitions SET status = 'current' WHERE id = ?",
                (existing_def_id,),
            )
            self.conn.commit()
            return

        self.conn.execute(
            """INSERT INTO asset_definitions
               (asset_id, definition_hash, continuity_key, source_text, module_source_text,
                decorator_metadata_json, return_type, serializer_kind, python_version,
                codebase_hash, dependency_cone_hash, status, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'current', ?)""",
            (
                asset_id,
                asset.definition_hash,
                asset.continuity_key,
                asset.source_text,
                asset.module_source_text,
                asset.decorator_metadata_json,
                asset.return_type,
                asset.serializer_kind,
                asset.python_version,
                asset.codebase_hash,
                asset.dependency_cone_hash,
                created_at,
            ),
        )
        self.conn.commit()

    def list_assets(self, *, include_inactive: bool = False) -> list[AssetSummary]:
        clause = "" if include_inactive else "WHERE a.active = 1"
        rows = self.conn.execute(
            f"""SELECT a.id, a.logical_name, a.module_path, a.file_path, a.function_name,
                       d.definition_hash, a.kind, a.freshness, a.purity, a.parent_asset_id,
                       d.decorator_metadata_json, d.id
                FROM assets a
                JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
                {clause}
                ORDER BY a.logical_name ASC"""
        ).fetchall()

        assets = []
        for row in rows:
            asset_id = row[0]
            latest = self._latest_materialization(asset_id)

            # Compute partitions_state: "pending" if the asset has a
            # dynamic partition whose upstream hasn't materialised yet.
            partitions_state = self._compute_partitions_state(row[10], row[11])

            assets.append(
                AssetSummary(
                    asset_id=asset_id,
                    logical_name=row[1],
                    kind=row[6] or "asset",
                    module_path=row[2],
                    file_path=row[3],
                    function_name=row[4],
                    definition_hash=row[5],
                    freshness=row[7] or "always",
                    purity=row[8] or "pure",
                    parent_asset_id=row[9],
                    partitions_state=partitions_state,
                    materialization_status=latest.status if latest else None,
                    materialization_run_hash=latest.run_hash if latest else None,
                    materialization_created_at=latest.created_at if latest else None,
                )
            )
        return assets

    def _compute_partitions_state(self, decorator_metadata_json: str, definition_id: int) -> str | None:
        """Return 'pending' or 'resolved' for dynamically-partitioned assets.

        Returns None for assets with no partitions or only static partitions.
        """
        try:
            import json as _json

            meta = _json.loads(decorator_metadata_json)
        except Exception:
            return None
        parts = meta.get("partitions")
        if not parts or not isinstance(parts, dict):
            return None
        has_dynamic = False
        for spec in parts.values():
            if isinstance(spec, dict) and spec.get("kind") == "dynamic":
                has_dynamic = True
                break
        if not has_dynamic:
            return None

        # Find the partition-source inputs and check if they have success mats
        inputs = self.get_asset_inputs(definition_id)
        for inp in inputs:
            if inp.is_partition_source and inp.upstream_asset_id:
                mat = self.latest_successful_materialization(inp.upstream_asset_id)
                if mat is None:
                    return "pending"
        return "resolved"

    def asset_detail(self, asset_id: int) -> AssetDetail:
        row = self.conn.execute(
            """SELECT a.id, a.logical_name, a.continuity_key, a.module_path, a.file_path,
                      a.function_name, a.asset_slug,
                      d.id, d.definition_hash, d.source_text, d.module_source_text,
                      d.decorator_metadata_json, d.return_type, d.serializer_kind,
                      d.python_version, d.codebase_hash, d.dependency_cone_hash,
                      a.kind, a.purity, a.parent_asset_id, a.sink_path, a.sink_serializer
               FROM assets a
               JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
               WHERE a.id = ?""",
            (asset_id,),
        ).fetchone()

        if row is None:
            raise ValueError(f"asset {asset_id} not found")

        kind = row[17] or "asset"
        latest_observation = self.latest_sensor_observation(asset_id) if kind == "sensor" else None

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
                purity=row[18] or "pure",
                parent_asset_id=row[19],
                sink_path=row[20],
                sink_serializer=row[21],
                definition_id=row[7],
                definition_hash=row[8],
                run_hash=row[8],  # placeholder
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
        )

    def asset_id_by_logical_name(self, logical_name: str) -> int | None:
        row = self.conn.execute(
            "SELECT id FROM assets WHERE logical_name = ? AND active = 1 LIMIT 1",
            (logical_name,),
        ).fetchone()
        return row[0] if row else None

    def deactivate_asset(self, asset_id: int) -> None:
        """Mark an asset inactive (removed from current DAG). History is preserved."""
        self.conn.execute(
            "UPDATE assets SET active = 0 WHERE id = ?",
            (asset_id,),
        )
        self.conn.commit()

    def reactivate_asset(self, asset_id: int) -> None:
        self.conn.execute(
            "UPDATE assets SET active = 1 WHERE id = ?",
            (asset_id,),
        )
        self.conn.commit()

    def rename_asset(
        self,
        old_asset_id: int,
        new_continuity_key: str,
        new_logical_name: str,
        new_module_path: str,
        new_file_path: str,
        new_function_name: str,
    ) -> None:
        """Rename an asset in place. Preserves asset_id and all FK history."""
        old_row = self.conn.execute(
            "SELECT continuity_key FROM assets WHERE id = ?",
            (old_asset_id,),
        ).fetchone()
        old_ck = old_row[0] if old_row else ""

        self.conn.execute(
            """UPDATE assets SET
               logical_name = ?, continuity_key = ?, module_path = ?,
               file_path = ?, function_name = ?, active = 1
               WHERE id = ?""",
            (
                new_logical_name,
                new_continuity_key,
                new_module_path,
                new_file_path,
                new_function_name,
                old_asset_id,
            ),
        )
        self.conn.execute(
            """INSERT INTO asset_renames
               (asset_id, old_continuity_key, new_continuity_key, created_at)
               VALUES (?, ?, ?, ?)""",
            (old_asset_id, old_ck, new_continuity_key, now_ts()),
        )
        self.conn.commit()

    def list_active_asset_ids(self) -> set[int]:
        rows = self.conn.execute("SELECT id FROM assets WHERE active = 1").fetchall()
        return {r[0] for r in rows}

    def prune_unreachable(self, active_ids: set[int]) -> PruneResult:
        """Delete all rows belonging to assets NOT in ``active_ids``."""
        # Collect ids to prune (currently inactive rows)
        rows = self.conn.execute("SELECT id FROM assets WHERE id NOT IN ({})".format(",".join(str(i) for i in active_ids) or "NULL")).fetchall()
        prune_ids = [r[0] for r in rows]

        result = PruneResult()
        for pid in prune_ids:
            r = self.conn.execute("SELECT COUNT(*) FROM materializations WHERE asset_id = ?", (pid,)).fetchone()
            result.removed_materializations += r[0]
            self.conn.execute("DELETE FROM materializations WHERE asset_id = ?", (pid,))

            r = self.conn.execute("SELECT COUNT(*) FROM sensor_observations WHERE asset_id = ?", (pid,)).fetchone()
            result.removed_observations += r[0]
            self.conn.execute("DELETE FROM sensor_observations WHERE asset_id = ?", (pid,))

            r = self.conn.execute("SELECT COUNT(*) FROM effect_executions WHERE asset_id = ?", (pid,)).fetchone()
            result.removed_effect_executions += r[0]
            self.conn.execute("DELETE FROM effect_executions WHERE asset_id = ?", (pid,))

            r = self.conn.execute("SELECT COUNT(*) FROM sink_executions WHERE asset_id = ?", (pid,)).fetchone()
            result.removed_sink_executions += r[0]
            self.conn.execute("DELETE FROM sink_executions WHERE asset_id = ?", (pid,))

            # asset_definitions FK cleanup
            def_rows = self.conn.execute("SELECT id FROM asset_definitions WHERE asset_id = ?", (pid,)).fetchall()
            for (def_id,) in def_rows:
                self.conn.execute("DELETE FROM asset_inputs WHERE definition_id = ?", (def_id,))
            self.conn.execute("DELETE FROM asset_definitions WHERE asset_id = ?", (pid,))

            self.conn.execute("DELETE FROM assets WHERE id = ?", (pid,))
            result.removed_assets += 1

        self.conn.commit()
        return result

    # ------------------------------------------------------------------
    # Materializations
    # ------------------------------------------------------------------

    def insert_queued_materialization(
        self,
        asset_id: int,
        definition_id: int,
        run_hash: str,
        partition_key_json: str | None = None,
    ) -> int:
        self.conn.execute(
            """INSERT INTO materializations
               (asset_id, definition_id, run_hash, status, partition_key_json, created_at)
               VALUES (?, ?, ?, 'queued', ?, ?)""",
            (asset_id, definition_id, run_hash, partition_key_json, now_ts()),
        )
        self.conn.commit()
        row = self.conn.execute("SELECT last_insert_rowid()").fetchone()
        return row[0]

    def insert_queued_materializations_batch(
        self,
        asset_id: int,
        definition_id: int,
        run_hash: str,
        partition_keys: list[str],
    ) -> int:
        created_at = now_ts()
        for pk in partition_keys:
            self.conn.execute(
                """INSERT INTO materializations
                   (asset_id, definition_id, run_hash, status, partition_key_json, created_at)
                   VALUES (?, ?, ?, 'queued', ?, ?)""",
                (asset_id, definition_id, run_hash, pk, created_at),
            )
        self.conn.commit()
        return len(partition_keys)

    def successful_materialization_for_run(self, asset_id: int, run_hash: str) -> MaterializationRecord | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, stale_inputs_used, created_at
               FROM materializations
               WHERE asset_id = ? AND run_hash = ? AND status = 'success'
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id, run_hash),
        ).fetchone()
        return _mat_from_row(row) if row else None

    def active_materialization_for_run(self, asset_id: int, run_hash: str) -> MaterializationRecord | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, stale_inputs_used, created_at
               FROM materializations
               WHERE asset_id = ? AND run_hash = ? AND status IN ('queued', 'running')
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id, run_hash),
        ).fetchone()
        return _mat_from_row(row) if row else None

    def active_materialization_for_asset(self, asset_id: int) -> MaterializationRecord | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, stale_inputs_used, created_at
               FROM materializations
               WHERE asset_id = ? AND status IN ('queued', 'running')
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id,),
        ).fetchone()
        return _mat_from_row(row) if row else None

    def latest_successful_materialization(self, asset_id: int) -> MaterializationRecord | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, stale_inputs_used, created_at
               FROM materializations
               WHERE asset_id = ? AND status = 'success'
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id,),
        ).fetchone()
        return _mat_from_row(row) if row else None

    def mark_materialization_success(
        self,
        materialization_id: int,
        artifact_path: str,
        artifact_format: str,
        artifact_checksum: str,
        *,
        stale_inputs_used: bool = False,
    ) -> None:
        self.conn.execute(
            """UPDATE materializations
               SET status = 'success', artifact_path = ?, artifact_format = ?,
                   artifact_checksum = ?, stale_inputs_used = ?
               WHERE id = ?""",
            (
                artifact_path,
                artifact_format,
                artifact_checksum,
                1 if stale_inputs_used else 0,
                materialization_id,
            ),
        )
        self.conn.commit()

    def mark_materialization_failed(self, materialization_id: int, error: str) -> None:
        self.conn.execute(
            "UPDATE materializations SET status = 'failed', last_error = ? WHERE id = ?",
            (error, materialization_id),
        )
        self.conn.commit()

    def update_materialization_run_hash(self, materialization_id: int, run_hash: str) -> None:
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
                      m.last_error, m.partition_key_json, m.stale_inputs_used, m.created_at,
                      a.id, a.logical_name, a.module_path, a.file_path, a.function_name,
                      d.definition_hash, a.freshness, a.purity, a.parent_asset_id, a.kind
               FROM materializations m
               JOIN assets a ON a.id = m.asset_id
               JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
               ORDER BY m.created_at DESC, m.id DESC LIMIT ?""",
            (limit,),
        ).fetchall()

        result = []
        for row in rows:
            mat = _mat_from_row(row[:12])
            aid = row[12]
            latest = self._latest_materialization(aid)
            summary = AssetSummary(
                asset_id=aid,
                logical_name=row[13],
                kind=row[21] or "asset",
                module_path=row[14],
                file_path=row[15],
                function_name=row[16],
                definition_hash=row[17],
                freshness=row[18] or "always",
                purity=row[19] or "pure",
                parent_asset_id=row[20],
                materialization_status=latest.status if latest else None,
                materialization_run_hash=latest.run_hash if latest else None,
                materialization_created_at=latest.created_at if latest else None,
            )
            result.append((mat, summary))
        return result

    def get_materialization_with_asset(self, materialization_id: int) -> tuple[MaterializationRecord, AssetSummary]:
        row = self.conn.execute(
            """SELECT m.id, m.asset_id, m.definition_id, m.run_hash, m.status,
                      m.artifact_path, m.artifact_format, m.artifact_checksum,
                      m.last_error, m.partition_key_json, m.stale_inputs_used, m.created_at,
                      a.id, a.logical_name, a.module_path, a.file_path, a.function_name,
                      d.definition_hash, a.freshness, a.purity, a.parent_asset_id, a.kind
               FROM materializations m
               JOIN assets a ON a.id = m.asset_id
               JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
               WHERE m.id = ?""",
            (materialization_id,),
        ).fetchone()

        if row is None:
            raise ValueError(f"job {materialization_id} not found")

        mat = _mat_from_row(row[:12])
        aid = row[12]
        latest = self._latest_materialization(aid)
        summary = AssetSummary(
            asset_id=aid,
            logical_name=row[13],
            kind=row[21] or "asset",
            module_path=row[14],
            file_path=row[15],
            function_name=row[16],
            definition_hash=row[17],
            freshness=row[18] or "always",
            purity=row[19] or "pure",
            parent_asset_id=row[20],
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
            """SELECT parameter_name, upstream_asset_ref, upstream_asset_id,
                      collect_mode, is_partition_source
               FROM asset_inputs WHERE definition_id = ? ORDER BY parameter_name""",
            (definition_id,),
        ).fetchall()
        return [
            AssetInput(
                parameter_name=r[0],
                upstream_asset_ref=r[1],
                upstream_asset_id=r[2],
                collect_mode=bool(r[3]),
                is_partition_source=bool(r[4]),
            )
            for r in rows
        ]

    def list_all_asset_inputs(self) -> list[tuple[int, AssetInput]]:
        """Return (downstream_asset_id, AssetInput) for all active assets in a single query."""
        rows = self.conn.execute(
            """SELECT a.id, ai.parameter_name, ai.upstream_asset_ref,
                      ai.upstream_asset_id, ai.collect_mode, ai.is_partition_source
               FROM assets a
               JOIN asset_definitions d ON d.asset_id = a.id AND d.status = 'current'
               JOIN asset_inputs ai ON ai.definition_id = d.id
               WHERE a.active = 1
                 AND ai.upstream_asset_id IS NOT NULL
                 AND ai.upstream_asset_id != -1
               ORDER BY a.id, ai.parameter_name"""
        ).fetchall()
        return [
            (
                r[0],
                AssetInput(
                    parameter_name=r[1],
                    upstream_asset_ref=r[2],
                    upstream_asset_id=r[3],
                    collect_mode=bool(r[4]),
                    is_partition_source=bool(r[5]),
                ),
            )
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
                """INSERT INTO asset_inputs
                   (definition_id, parameter_name, upstream_asset_ref, upstream_asset_id,
                    collect_mode, is_partition_source)
                   VALUES (?, ?, ?, ?, ?, ?)""",
                (
                    definition_id,
                    inp.parameter_name,
                    inp.upstream_asset_ref,
                    upstream_id,
                    1 if inp.collect_mode else 0,
                    1 if inp.is_partition_source else 0,
                ),
            )
        self.conn.commit()

    def insert_materialization_inputs(self, materialization_id: int, inputs: list[dict]) -> None:
        for inp in inputs:
            self.conn.execute(
                """INSERT INTO materialization_inputs
                   (materialization_id, parameter_name, upstream_materialization_id, upstream_asset_id)
                   VALUES (?, ?, ?, ?)""",
                (
                    materialization_id,
                    inp["parameter_name"],
                    inp["upstream_materialization_id"],
                    inp["upstream_asset_id"],
                ),
            )
        self.conn.commit()

    # ------------------------------------------------------------------
    # Sensor observations
    # ------------------------------------------------------------------

    def insert_sensor_observation(
        self,
        asset_id: int,
        definition_id: int,
        update_detected: bool,
        output_json: str | None = None,
    ) -> int:
        self.conn.execute(
            """INSERT INTO sensor_observations
               (asset_id, definition_id, update_detected, output_json, created_at)
               VALUES (?, ?, ?, ?, ?)""",
            (asset_id, definition_id, 1 if update_detected else 0, output_json, now_ts()),
        )
        self.conn.commit()
        row = self.conn.execute("SELECT last_insert_rowid()").fetchone()
        return row[0]

    def latest_sensor_observation(self, asset_id: int) -> SensorObservation | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, update_detected, output_json, created_at
               FROM sensor_observations WHERE asset_id = ?
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id,),
        ).fetchone()
        if row is None:
            return None
        return SensorObservation(
            observation_id=row[0],
            asset_id=row[1],
            definition_id=row[2],
            update_detected=bool(row[3]),
            output_json=row[4],
            created_at=row[5],
        )

    def list_sensor_observations(self, asset_id: int, limit: int = 50) -> list[SensorObservation]:
        rows = self.conn.execute(
            """SELECT id, asset_id, definition_id, update_detected, output_json, created_at
               FROM sensor_observations WHERE asset_id = ?
               ORDER BY created_at DESC, id DESC LIMIT ?""",
            (asset_id, limit),
        ).fetchall()
        return [
            SensorObservation(
                observation_id=r[0],
                asset_id=r[1],
                definition_id=r[2],
                update_detected=bool(r[3]),
                output_json=r[4],
                created_at=r[5],
            )
            for r in rows
        ]

    # ------------------------------------------------------------------
    # Effect executions
    # ------------------------------------------------------------------

    def insert_effect_execution(
        self,
        asset_id: int,
        definition_id: int,
        status: str,
        last_error: str | None = None,
    ) -> int:
        self.conn.execute(
            """INSERT INTO effect_executions
               (asset_id, definition_id, status, last_error, created_at)
               VALUES (?, ?, ?, ?, ?)""",
            (asset_id, definition_id, status, last_error, now_ts()),
        )
        self.conn.commit()
        row = self.conn.execute("SELECT last_insert_rowid()").fetchone()
        return row[0]

    def latest_effect_execution(self, asset_id: int) -> EffectExecution | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, status, last_error, created_at
               FROM effect_executions WHERE asset_id = ?
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id,),
        ).fetchone()
        if row is None:
            return None
        return EffectExecution(
            execution_id=row[0],
            asset_id=row[1],
            definition_id=row[2],
            status=row[3],
            last_error=row[4],
            created_at=row[5],
        )

    # ------------------------------------------------------------------
    # Sink executions
    # ------------------------------------------------------------------

    def insert_sink_execution(
        self,
        asset_id: int,
        definition_id: int,
        run_hash: str,
        status: str,
        destination_path: str,
        last_error: str | None = None,
    ) -> int:
        self.conn.execute(
            """INSERT INTO sink_executions
               (asset_id, definition_id, run_hash, status, destination_path, last_error, created_at)
               VALUES (?, ?, ?, ?, ?, ?, ?)""",
            (asset_id, definition_id, run_hash, status, destination_path, last_error, now_ts()),
        )
        self.conn.commit()
        row = self.conn.execute("SELECT last_insert_rowid()").fetchone()
        return row[0]

    def latest_sink_execution(self, asset_id: int) -> SinkExecution | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status, destination_path, last_error, created_at
               FROM sink_executions WHERE asset_id = ?
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id,),
        ).fetchone()
        if row is None:
            return None
        return SinkExecution(
            execution_id=row[0],
            asset_id=row[1],
            definition_id=row[2],
            run_hash=row[3],
            status=row[4],
            destination_path=row[5],
            last_error=row[6],
            created_at=row[7],
        )

    def successful_sink_execution_for_run(self, asset_id: int, run_hash: str) -> SinkExecution | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status, destination_path, last_error, created_at
               FROM sink_executions
               WHERE asset_id = ? AND run_hash = ? AND status = 'success'
               ORDER BY created_at DESC, id DESC LIMIT 1""",
            (asset_id, run_hash),
        ).fetchone()
        if row is None:
            return None
        return SinkExecution(
            execution_id=row[0],
            asset_id=row[1],
            definition_id=row[2],
            run_hash=row[3],
            status=row[4],
            destination_path=row[5],
            last_error=row[6],
            created_at=row[7],
        )

    # ------------------------------------------------------------------
    # Materialization queries (notebook helpers)
    # ------------------------------------------------------------------

    def list_materializations(self, asset_id: int, limit: int = 50, offset: int = 0) -> list[MaterializationRecord]:
        rows = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, stale_inputs_used, created_at
               FROM materializations WHERE asset_id = ?
               ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?""",
            (asset_id, limit, offset),
        ).fetchall()
        return [_mat_from_row(r) for r in rows]

    def latest_successful_materialization_for_partition(self, asset_id: int, partition_key_json: str) -> MaterializationRecord | None:
        row = self.conn.execute(
            """SELECT id, asset_id, definition_id, run_hash, status,
                      artifact_path, artifact_format, artifact_checksum,
                      last_error, partition_key_json, stale_inputs_used, created_at
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
                      last_error, partition_key_json, stale_inputs_used, created_at
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
        stale_inputs_used=bool(row[10]),
        created_at=row[11],
    )
