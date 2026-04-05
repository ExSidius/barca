"""FastAPI application — thin route handlers delegating to service layer."""

from __future__ import annotations

import asyncio
import logging
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI, HTTPException

from barca._models import AssetDetail, AssetSummary, JobDetail, ReconcileResult
from barca._store import MetadataStore
from barca_server.logging import configure_logging
from barca_server.scheduler import scheduler_loop
from barca_server import service

logger = logging.getLogger("barca.server.app")


def create_app(
    *,
    repo_root: Path | None = None,
    interval: int = 60,
    log_level: str = "info",
) -> FastAPI:
    """Create and configure the FastAPI application."""
    configure_logging(log_level)

    if repo_root is None:
        repo_root = Path.cwd()
    db_path = str(repo_root / ".barca" / "metadata.db")

    reconcile_lock = asyncio.Lock()
    scheduler_task: asyncio.Task | None = None

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        nonlocal scheduler_task
        logger.info("server starting (repo_root=%s, interval=%d)", repo_root, interval)
        scheduler_task = asyncio.create_task(
            scheduler_loop(repo_root, db_path, interval, reconcile_lock)
        )
        yield
        scheduler_task.cancel()
        try:
            await scheduler_task
        except asyncio.CancelledError:
            pass
        logger.info("server stopped")

    app = FastAPI(title="Barca", version="0.1.0", lifespan=lifespan)

    # Note: MetadataStore must be created in the same thread that uses it
    # (sqlite3 constraint). All _run_* helpers create the store inside
    # to_thread so the connection lives on the executor thread.

    def _run_list_assets():
        return service.list_assets(MetadataStore(db_path), repo_root)

    def _run_get_asset(asset_id: int):
        return service.get_asset(MetadataStore(db_path), asset_id)

    def _run_refresh_asset(asset_id: int):
        return service.refresh_asset(MetadataStore(db_path), repo_root, asset_id)

    def _run_reconcile():
        return service.run_reconcile(MetadataStore(db_path), repo_root)

    def _run_list_jobs():
        return service.list_jobs(MetadataStore(db_path))

    def _run_get_job(job_id: int):
        return service.get_job(MetadataStore(db_path), job_id)

    # --- Routes (thin wrappers around service layer) ---

    @app.get("/health")
    async def health() -> dict:
        running = scheduler_task is not None and not scheduler_task.done()
        return {"status": "ok", "scheduler_running": running}

    @app.get("/assets")
    async def assets_list() -> list[AssetSummary]:
        return await asyncio.to_thread(_run_list_assets)

    @app.get("/assets/{asset_id}")
    async def assets_show(asset_id: int) -> AssetDetail:
        try:
            return await asyncio.to_thread(_run_get_asset, asset_id)
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    @app.post("/assets/{asset_id}/refresh")
    async def assets_refresh(asset_id: int) -> AssetDetail:
        try:
            return await asyncio.to_thread(_run_refresh_asset, asset_id)
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    @app.post("/reconcile")
    async def reconcile_endpoint() -> ReconcileResult:
        async with reconcile_lock:
            return await asyncio.to_thread(_run_reconcile)

    @app.get("/jobs")
    async def jobs_list() -> list[JobDetail]:
        return await asyncio.to_thread(_run_list_jobs)

    @app.get("/jobs/{job_id}")
    async def jobs_show(job_id: int) -> JobDetail:
        try:
            return await asyncio.to_thread(_run_get_job, job_id)
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e))

    return app
