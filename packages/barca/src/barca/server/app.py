"""FastAPI application — JSON API + React SPA + JSON SSE watch streams."""

from __future__ import annotations

import asyncio
import json
import logging
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import APIRouter, FastAPI, HTTPException, Query
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse, RedirectResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles

from barca._models import (
    AssetDetail,
    AssetSummary,
    JobDetail,
    MaterializationRecord,
    ReconcileResult,
    SensorObservation,
)
from barca._store import MetadataStore
from barca.server import service
from barca.server.logging import configure_logging
from barca.server.notifier import ChangeNotifier
from barca.server.scheduler import scheduler_loop

logger = logging.getLogger("barca.server.app")

_SERVER_DIR = Path(__file__).parent
_STATIC_DIR = _SERVER_DIR / "static"
_UI_DIST_DIR = _SERVER_DIR / "ui" / "dist"


def _sse(data: dict) -> str:
    """Format a dict as a Server-Sent Event."""
    return f"data: {json.dumps(data)}\n\n"


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
    db_path_str = str(repo_root / ".barca" / "metadata.db")
    db_path = Path(db_path_str)

    reconcile_lock = asyncio.Lock()
    scheduler_task: asyncio.Task | None = None
    notifier = ChangeNotifier()

    @asynccontextmanager
    async def lifespan(app: FastAPI):
        nonlocal scheduler_task
        logger.info("server starting (repo_root=%s, interval=%d)", repo_root, interval)
        try:
            MetadataStore(db_path_str)
        except RuntimeError as exc:
            logger.error("DB schema check failed — run 'barca reset --db' then retry.\n%s", exc)
            raise SystemExit(1) from exc
        scheduler_task = asyncio.create_task(scheduler_loop(repo_root, db_path_str, interval, reconcile_lock))
        wal_task = asyncio.create_task(notifier.watch_wal(db_path))
        yield
        scheduler_task.cancel()
        wal_task.cancel()
        for t in [scheduler_task, wal_task]:
            try:
                await t
            except asyncio.CancelledError:
                pass
        logger.info("server stopped")

    app = FastAPI(title="Barca", version="0.1.0", lifespan=lifespan)

    app.add_middleware(
        CORSMiddleware,
        allow_origins=["*"],
        allow_methods=["*"],
        allow_headers=["*"],
    )

    def _store() -> MetadataStore:
        return MetadataStore(db_path_str)

    # ------------------------------------------------------------------
    # JSON API router  (/api/*)
    # ------------------------------------------------------------------

    api = APIRouter(prefix="/api")

    @api.get("/health")
    async def health() -> dict:
        running = scheduler_task is not None and not scheduler_task.done()
        return {"status": "ok", "scheduler_running": running}

    @api.get("/assets")
    async def assets_list() -> list[AssetSummary]:
        return await asyncio.to_thread(lambda: service.list_assets(_store(), repo_root))

    @api.get("/assets/{asset_id}")
    async def assets_show(asset_id: int) -> AssetDetail:
        try:
            return await asyncio.to_thread(lambda: service.get_asset(_store(), asset_id))
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e)) from e

    @api.post("/assets/{asset_id}/refresh")
    async def assets_refresh(asset_id: int) -> AssetDetail:
        try:
            return await asyncio.to_thread(lambda: service.refresh_asset(_store(), repo_root, asset_id))
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e)) from e

    @api.get("/assets/{asset_id}/materializations")
    async def asset_materializations(
        asset_id: int,
        limit: int = Query(20, ge=1, le=100),
        offset: int = Query(0, ge=0),
    ) -> list[MaterializationRecord]:
        return await asyncio.to_thread(lambda: service.list_asset_materializations(_store(), asset_id, limit=limit, offset=offset))

    @api.post("/reconcile")
    async def reconcile_endpoint() -> ReconcileResult:
        async with reconcile_lock:
            return await asyncio.to_thread(lambda: service.run_reconcile(_store(), repo_root))

    @api.get("/jobs")
    async def jobs_list() -> list[JobDetail]:
        return await asyncio.to_thread(lambda: service.list_jobs(_store()))

    @api.get("/jobs/{job_id}")
    async def jobs_show(job_id: int) -> JobDetail:
        try:
            return await asyncio.to_thread(lambda: service.get_job(_store(), job_id))
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e)) from e

    @api.get("/sensors")
    async def sensors_list() -> list[AssetSummary]:
        return await asyncio.to_thread(lambda: service.list_sensors(_store(), repo_root))

    @api.get("/sensors/{sensor_id}/observations")
    async def sensor_observations(sensor_id: int) -> list[SensorObservation]:
        return await asyncio.to_thread(lambda: service.get_sensor_observations(_store(), sensor_id))

    @api.post("/sensors/{sensor_id}/trigger")
    async def sensor_trigger(sensor_id: int) -> SensorObservation:
        try:
            return await asyncio.to_thread(lambda: service.trigger_sensor(_store(), repo_root, sensor_id))
        except ValueError as e:
            raise HTTPException(status_code=404, detail=str(e)) from e

    # ------------------------------------------------------------------
    # SSE JSON watch streams  (/sse/*)
    # ------------------------------------------------------------------

    sse = APIRouter(prefix="/sse")

    @sse.get("/assets")
    async def sse_assets():
        """Stream asset list updates as JSON whenever the DB changes."""

        async def generate():
            last_snapshot: dict | None = None
            # Send initial state
            try:
                assets = await asyncio.to_thread(lambda: service.list_assets(_store(), repo_root))
                yield _sse({"assets": [a.model_dump() for a in assets]})
                last_snapshot = {a.asset_id: (a.materialization_status, a.materialization_run_hash) for a in assets}
            except Exception:
                logger.exception("initial sse/assets load failed")

            while True:
                ev = notifier.subscribe()
                try:
                    await ev.wait()
                except asyncio.CancelledError:
                    notifier.unsubscribe(ev)
                    return
                try:
                    assets = await asyncio.to_thread(lambda: service.list_assets(_store(), repo_root))
                except Exception:
                    continue
                snapshot = {a.asset_id: (a.materialization_status, a.materialization_run_hash) for a in assets}
                if snapshot != last_snapshot:
                    last_snapshot = snapshot
                    yield _sse({"assets": [a.model_dump() for a in assets]})

        return StreamingResponse(generate(), media_type="text/event-stream")

    @sse.get("/assets/{asset_id}")
    async def sse_asset_detail(asset_id: int):
        """Stream a single asset's detail + materialization history when it changes."""

        async def generate():
            last_hash: str | None = None
            # Initial send
            try:

                def _q(aid=asset_id):
                    s = _store()
                    return service.get_asset(s, aid), service.list_asset_materializations(s, aid, limit=20)

                detail, mats = await asyncio.to_thread(_q)
                yield _sse(
                    {
                        "detail": detail.model_dump(),
                        "materializations": [m.model_dump() for m in mats],
                    }
                )
                last_hash = detail.latest_materialization.run_hash if detail.latest_materialization else None
            except Exception:
                logger.exception("initial sse/assets/%d load failed", asset_id)

            while True:
                ev = notifier.subscribe()
                try:
                    await ev.wait()
                except asyncio.CancelledError:
                    notifier.unsubscribe(ev)
                    return
                try:

                    def _q(aid=asset_id):
                        s = _store()
                        return service.get_asset(s, aid), service.list_asset_materializations(s, aid, limit=20)

                    detail, mats = await asyncio.to_thread(_q)
                except Exception:
                    continue
                current_hash = detail.latest_materialization.run_hash if detail.latest_materialization else None
                if current_hash != last_hash:
                    last_hash = current_hash
                    yield _sse(
                        {
                            "detail": detail.model_dump(),
                            "materializations": [m.model_dump() for m in mats],
                        }
                    )

        return StreamingResponse(generate(), media_type="text/event-stream")

    # ------------------------------------------------------------------
    # Register API + SSE routers
    # ------------------------------------------------------------------

    app.include_router(api)
    app.include_router(sse)

    # Keep /health working for CLI compatibility
    @app.get("/health")
    async def health_compat() -> dict:
        running = scheduler_task is not None and not scheduler_task.done()
        return {"status": "ok", "scheduler_running": running}

    # Redirect root to /ui/
    @app.get("/")
    async def root_redirect():
        return RedirectResponse(url="/ui/")

    # Static files (fonts etc.)
    if _STATIC_DIR.exists():
        app.mount("/static", StaticFiles(directory=str(_STATIC_DIR)), name="static")

    # Serve built React app at /ui/
    if _UI_DIST_DIR.exists():
        # Mount built Vite assets (JS/CSS) at /ui/static/*
        app.mount(
            "/ui/static",
            StaticFiles(directory=str(_UI_DIST_DIR / "static")),
            name="ui_static",
        )

        # SPA fallback: any /ui/* request that isn't an API or built asset returns index.html
        index_html = _UI_DIST_DIR / "index.html"

        @app.get("/ui/{full_path:path}")
        async def ui_spa(full_path: str):
            return FileResponse(index_html)

        @app.get("/ui")
        async def ui_root():
            return FileResponse(index_html)
    else:
        logger.warning(
            "React UI not built — run 'npm run build' in %s. /ui/ will return 404.",
            _UI_DIST_DIR,
        )

    return app
