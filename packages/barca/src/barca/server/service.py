"""Pure business logic layer — no FastAPI deps, no request/response objects."""

from __future__ import annotations

import logging
from pathlib import Path

from barca._engine import refresh, reindex
from barca._engine import trigger_sensor as engine_trigger_sensor
from barca._models import AssetDetail, AssetSummary, JobDetail, MaterializationRecord, ReconcileResult, SensorObservation
from barca._reconciler import reconcile
from barca._store import MetadataStore

logger = logging.getLogger("barca.server.service")


def list_assets(store: MetadataStore, repo_root: Path) -> list[AssetSummary]:
    """Reindex and return all assets."""
    reindex(store, repo_root)
    return store.list_assets()


def get_asset(store: MetadataStore, asset_id: int) -> AssetDetail:
    """Return detail for a single asset."""
    return store.asset_detail(asset_id)


def refresh_asset(
    store: MetadataStore,
    repo_root: Path,
    asset_id: int,
    *,
    max_workers: int | None = None,
) -> AssetDetail:
    """Reindex then refresh (materialize) a single asset."""
    reindex(store, repo_root)
    logger.info("refresh triggered for asset %d", asset_id)
    result = refresh(store, repo_root, asset_id, max_workers=max_workers)
    logger.info("refresh complete for asset %d", asset_id)
    return result


def run_reconcile(store: MetadataStore, repo_root: Path) -> ReconcileResult:
    """Run a single reconciliation pass."""
    logger.info("reconcile started")
    result = reconcile(store, repo_root)
    logger.info(
        "reconcile complete: sensors=%d assets=%d effects=%d fresh=%d stale_waiting=%d failed=%d",
        result.executed_sensors,
        result.executed_assets,
        result.executed_effects,
        result.fresh,
        result.stale_waiting,
        result.failed,
    )
    return result


def list_jobs(store: MetadataStore, limit: int = 50) -> list[JobDetail]:
    """Return recent materializations with their asset summaries."""
    pairs = store.list_recent_materializations(limit)
    return [JobDetail(job=mat, asset=summary) for mat, summary in pairs]


def get_job(store: MetadataStore, job_id: int) -> JobDetail:
    """Return detail for a single job."""
    mat, summary = store.get_materialization_with_asset(job_id)
    return JobDetail(job=mat, asset=summary)


# ------------------------------------------------------------------
# Sensors
# ------------------------------------------------------------------


def list_sensors(store: MetadataStore, repo_root: Path) -> list[AssetSummary]:
    """Reindex and return all sensors."""
    reindex(store, repo_root)
    return [a for a in store.list_assets() if a.kind == "sensor"]


def get_sensor_observations(
    store: MetadataStore,
    asset_id: int,
    limit: int = 50,
) -> list[SensorObservation]:
    """Return observation history for a sensor."""
    return store.list_sensor_observations(asset_id, limit)


def list_asset_materializations(
    store: MetadataStore,
    asset_id: int,
    limit: int = 20,
    offset: int = 0,
) -> list[MaterializationRecord]:
    """Return paginated materialization history for an asset."""
    return store.list_materializations(asset_id, limit=limit, offset=offset)


def trigger_sensor(
    store: MetadataStore,
    repo_root: Path,
    asset_id: int,
) -> SensorObservation:
    """Reindex then trigger a sensor manually."""
    reindex(store, repo_root)
    logger.info("sensor trigger requested for asset %d", asset_id)
    result = engine_trigger_sensor(store, repo_root, asset_id)
    logger.info("sensor triggered for asset %d (update_detected=%s)", asset_id, result.update_detected)
    return result
