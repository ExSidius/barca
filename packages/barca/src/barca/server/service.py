"""Pure business logic layer — no FastAPI deps, no request/response objects."""

from __future__ import annotations

import logging
from pathlib import Path

from barca._engine import refresh, reindex
from barca._engine import trigger_sensor as engine_trigger_sensor
from barca._models import (
    AssetDetail,
    AssetSummary,
    JobDetail,
    MaterializationRecord,
    PruneResult,
    RunPassResult,
    SensorObservation,
)
from barca._prune import prune as engine_prune
from barca._run import run_pass as engine_run_pass
from barca._store import MetadataStore

logger = logging.getLogger("barca.server.service")


def list_assets(store: MetadataStore, repo_root: Path) -> list[AssetSummary]:
    reindex(store, repo_root)
    return store.list_assets()


def get_asset(store: MetadataStore, asset_id: int) -> AssetDetail:
    return store.asset_detail(asset_id)


def refresh_asset(
    store: MetadataStore,
    repo_root: Path,
    asset_id: int,
    *,
    max_workers: int | None = None,
    stale_policy: str = "error",
) -> AssetDetail:
    reindex(store, repo_root)
    logger.info("refresh triggered for asset %d (stale_policy=%s)", asset_id, stale_policy)
    result = refresh(
        store,
        repo_root,
        asset_id,
        max_workers=max_workers,
        stale_policy=stale_policy,
    )
    logger.info("refresh complete for asset %d", asset_id)
    return result


def run_pass(store: MetadataStore, repo_root: Path) -> RunPassResult:
    """Execute a single run_pass and return the summary."""
    logger.info("run_pass started")
    result = engine_run_pass(store, repo_root)
    logger.info(
        "run_pass complete: sensors=%d assets=%d effects=%d sinks=%d fresh=%d manual=%d blocked=%d failed=%d sink_failed=%d",
        result.executed_sensors,
        result.executed_assets,
        result.executed_effects,
        result.executed_sinks,
        result.fresh,
        result.manual_skipped,
        result.stale_blocked,
        result.failed,
        result.sink_failed,
    )
    return result


def prune(store: MetadataStore, repo_root: Path) -> PruneResult:
    logger.info("prune started")
    result = engine_prune(store, repo_root)
    logger.info(
        "prune complete: removed_assets=%d removed_materializations=%d",
        result.removed_assets,
        result.removed_materializations,
    )
    return result


def list_jobs(store: MetadataStore, limit: int = 50) -> list[JobDetail]:
    pairs = store.list_recent_materializations(limit)
    return [JobDetail(job=mat, asset=summary) for mat, summary in pairs]


def get_job(store: MetadataStore, job_id: int) -> JobDetail:
    mat, summary = store.get_materialization_with_asset(job_id)
    return JobDetail(job=mat, asset=summary)


# ------------------------------------------------------------------
# Sensors
# ------------------------------------------------------------------


def list_sensors(store: MetadataStore, repo_root: Path) -> list[AssetSummary]:
    reindex(store, repo_root)
    return [a for a in store.list_assets() if a.kind == "sensor"]


def get_sensor_observations(
    store: MetadataStore,
    asset_id: int,
    limit: int = 50,
) -> list[SensorObservation]:
    return store.list_sensor_observations(asset_id, limit)


def list_asset_materializations(
    store: MetadataStore,
    asset_id: int,
    limit: int = 20,
    offset: int = 0,
) -> list[MaterializationRecord]:
    return store.list_materializations(asset_id, limit=limit, offset=offset)


def trigger_sensor(
    store: MetadataStore,
    repo_root: Path,
    asset_id: int,
) -> SensorObservation:
    reindex(store, repo_root)
    logger.info("sensor trigger requested for asset %d", asset_id)
    result = engine_trigger_sensor(store, repo_root, asset_id)
    logger.info(
        "sensor triggered for asset %d (update_detected=%s)",
        asset_id,
        result.update_detected,
    )
    return result
