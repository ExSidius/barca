"""FastAPI application — JSON API (/api/) + HTML UI (/ui/) + SSE watch streams."""

from __future__ import annotations

import asyncio
import logging
import traceback
from contextlib import asynccontextmanager
from pathlib import Path

from datastar_py import ServerSentEventGenerator as SSE
from datastar_py.fastapi import DatastarResponse
from fastapi import APIRouter, FastAPI, HTTPException, Query, Request
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import HTMLResponse, RedirectResponse
from fastapi.staticfiles import StaticFiles
from fastapi.templating import Jinja2Templates

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

_TEMPLATES_DIR = Path(__file__).parent / "templates"
_STATIC_DIR = Path(__file__).parent / "static"


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
        # Validate DB schema before accepting any traffic — fail fast with a clear message.
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

    templates = Jinja2Templates(directory=str(_TEMPLATES_DIR))

    # ------------------------------------------------------------------
    # Helper: run a blocking function in an executor thread
    # (MetadataStore must be created in the thread that uses it)
    # ------------------------------------------------------------------

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
    # UI router  (/ui/*)  — HTML pages + SSE fragments
    # ------------------------------------------------------------------

    ui = APIRouter(prefix="/ui")

    # --- Redirect root to UI ---
    @app.get("/")
    async def root_redirect():
        return RedirectResponse(url="/ui/")

    @app.get("/health")
    async def health_compat() -> dict:
        """Keep /health working for CLI compatibility."""
        running = scheduler_task is not None and not scheduler_task.done()
        return {"status": "ok", "scheduler_running": running}

    # --- Shared error page helper ---

    def _error_page(request: Request, exc: Exception, status_code: int = 500) -> HTMLResponse:
        tb = traceback.format_exc()
        html = f"""<!DOCTYPE html>
<html lang="en" class="dark">
<head>
  <meta charset="UTF-8"/>
  <title>Error {status_code} — Barca</title>
  <link rel="preconnect" href="https://fonts.googleapis.com"/>
  <link href="https://fonts.googleapis.com/css2?family=Geist+Mono:wght@400;500&display=swap" rel="stylesheet"/>
  <link rel="stylesheet" href="/static/app.css"/>
</head>
<body style="margin:0;background:#0a0a0a;color:#a1a1aa;font-family:'Geist Mono',monospace;padding:2rem;">
  <div style="max-width:860px;margin:0 auto;">
    <div style="display:flex;align-items:center;gap:.75rem;margin-bottom:1.5rem;">
      <a href="/ui/" style="color:#a1a1aa;text-decoration:none;font-size:.85rem;">← Barca</a>
      <span style="color:#3f3f46;">/</span>
      <span style="color:#f87171;font-size:.85rem;font-weight:600;">{status_code} Error</span>
    </div>
    <h1 style="color:#f87171;font-size:1rem;margin:0 0 .5rem;">{type(exc).__name__}: {exc}</h1>
    <pre style="background:#111;border:1px solid #262626;border-radius:.5rem;padding:1rem;
                font-size:.75rem;overflow:auto;color:#a1a1aa;white-space:pre-wrap;">{tb}</pre>
  </div>
</body>
</html>"""
        return HTMLResponse(content=html, status_code=status_code)

    # --- Full-page HTML routes ---

    @ui.get("/", response_class=HTMLResponse)
    async def dashboard(request: Request):
        try:

            def _run():
                store = _store()
                assets = store.list_assets()
                jobs = service.list_jobs(store, limit=5)
                running = scheduler_task is not None and not scheduler_task.done()
                return assets, jobs, running

            assets, jobs, scheduler_running = await asyncio.to_thread(_run)
            return templates.TemplateResponse(
                request=request,
                name="pages/dashboard.html",
                context={
                    "assets": assets,
                    "recent_jobs": jobs,
                    "scheduler_running": scheduler_running,
                    "total_assets": sum(1 for a in assets if a.kind == "asset"),
                    "total_sensors": sum(1 for a in assets if a.kind == "sensor"),
                    "total_effects": sum(1 for a in assets if a.kind == "effect"),
                },
            )
        except Exception as exc:
            logger.exception("dashboard failed")
            return _error_page(request, exc)

    @ui.get("/assets", response_class=HTMLResponse)
    async def assets_page(request: Request):
        try:
            assets = await asyncio.to_thread(lambda: service.list_assets(_store(), repo_root))
            return templates.TemplateResponse(
                request=request,
                name="pages/assets.html",
                context={"assets": assets},
            )
        except Exception as exc:
            logger.exception("assets page failed")
            return _error_page(request, exc)

    # NOTE: /assets/watch-all must be registered BEFORE /assets/{asset_id}
    # so FastAPI doesn't match "watch-all" as an integer asset_id param.
    @ui.get("/assets/watch-all")
    async def assets_watch_all():
        async def generate():
            last_snapshot: dict | None = None
            while True:
                ev = notifier.subscribe()
                try:
                    await ev.wait()
                except asyncio.CancelledError:
                    notifier.unsubscribe(ev)
                    return
                try:
                    assets = await asyncio.to_thread(lambda: _store().list_assets())
                except Exception:
                    continue
                snapshot = {a.asset_id: (a.materialization_status, a.materialization_run_hash) for a in assets}
                if snapshot != last_snapshot:
                    last_snapshot = snapshot
                    html = templates.get_template("components/asset_table_body.html").render(assets=assets)
                    yield SSE.patch_elements(html, selector="#assets-table")

        return DatastarResponse(generate())

    @ui.get("/assets/{asset_id}", response_class=HTMLResponse)
    async def asset_detail_page(request: Request, asset_id: int):
        def _run():
            store = _store()
            detail = service.get_asset(store, asset_id)
            mats = service.list_asset_materializations(store, asset_id, limit=20)
            inputs = store.get_asset_inputs(detail.asset.definition_id)
            return detail, mats, inputs

        try:
            detail, mats, inputs = await asyncio.to_thread(_run)
        except ValueError as exc:
            raise HTTPException(status_code=404, detail=str(exc)) from exc
        except Exception as exc:
            logger.exception("asset detail page failed for %d", asset_id)
            return _error_page(request, exc)
        return templates.TemplateResponse(
            request=request,
            name="pages/asset_detail.html",
            context={"detail": detail, "materializations": mats, "inputs": inputs, "asset_id": asset_id},
        )

    @ui.get("/jobs", response_class=HTMLResponse)
    async def jobs_page(request: Request):
        try:
            jobs = await asyncio.to_thread(lambda: service.list_jobs(_store()))
            return templates.TemplateResponse(
                request=request,
                name="pages/jobs.html",
                context={"jobs": jobs},
            )
        except Exception as exc:
            logger.exception("jobs page failed")
            return _error_page(request, exc)

    @ui.get("/jobs/{job_id}", response_class=HTMLResponse)
    async def job_detail_page(request: Request, job_id: int):
        try:
            job = await asyncio.to_thread(lambda: service.get_job(_store(), job_id))
        except ValueError as exc:
            raise HTTPException(status_code=404, detail=str(exc)) from exc
        except Exception as exc:
            logger.exception("job detail page failed for %d", job_id)
            return _error_page(request, exc)
        return templates.TemplateResponse(
            request=request,
            name="pages/job_detail.html",
            context={"job": job, "job_id": job_id},
        )

    @ui.get("/sensors", response_class=HTMLResponse)
    async def sensors_page(request: Request):
        try:
            sensors = await asyncio.to_thread(lambda: service.list_sensors(_store(), repo_root))
            return templates.TemplateResponse(
                request=request,
                name="pages/sensors.html",
                context={"sensors": sensors},
            )
        except Exception as exc:
            logger.exception("sensors page failed")
            return _error_page(request, exc)

    @ui.get("/sensors/{sensor_id}", response_class=HTMLResponse)
    async def sensor_detail_page(request: Request, sensor_id: int):
        def _run():
            store = _store()
            detail = service.get_asset(store, sensor_id)
            obs = service.get_sensor_observations(store, sensor_id, limit=50)
            return detail, obs

        try:
            detail, observations = await asyncio.to_thread(_run)
        except ValueError as exc:
            raise HTTPException(status_code=404, detail=str(exc)) from exc
        except Exception as exc:
            logger.exception("sensor detail page failed for %d", sensor_id)
            return _error_page(request, exc)
        return templates.TemplateResponse(
            request=request,
            name="pages/sensor_detail.html",
            context={"detail": detail, "observations": observations, "sensor_id": sensor_id},
        )

    # --- SSE fragment routes (Datastar targets) ---

    @ui.get("/fragments/assets")
    async def assets_fragment(kind: str = "all", q: str = ""):
        def _run():
            assets = service.list_assets(_store(), repo_root)
            if kind != "all":
                assets = [a for a in assets if a.kind == kind]
            if q:
                assets = [a for a in assets if q.lower() in a.logical_name.lower() or q.lower() in a.function_name.lower()]
            return assets

        assets = await asyncio.to_thread(_run)
        html = templates.get_template("components/asset_table_body.html").render(assets=assets)
        return DatastarResponse(SSE.patch_elements(html, selector="#assets-table"))

    @ui.get("/fragments/jobs")
    async def jobs_fragment(status: str = "all"):
        def _run():
            jobs = service.list_jobs(_store())
            if status != "all":
                jobs = [j for j in jobs if j.job.status == status]
            return jobs

        jobs = await asyncio.to_thread(_run)
        html = templates.get_template("components/job_table_body.html").render(jobs=jobs)
        return DatastarResponse(SSE.patch_elements(html, selector="#jobs-table"))

    # --- SSE action streams ---

    @ui.post("/assets/{asset_id}/refresh")
    async def asset_refresh_stream(asset_id: int):
        async def generate():
            yield SSE.patch_signals({"refreshing": True, "refreshError": ""})
            try:

                def _run():
                    return service.refresh_asset(_store(), repo_root, asset_id)

                detail = await asyncio.to_thread(_run)
                mats = await asyncio.to_thread(lambda: service.list_asset_materializations(_store(), asset_id, limit=20))
                status_html = templates.get_template("components/asset_status_region.html").render(detail=detail)
                mat_html = templates.get_template("components/mat_table.html").render(materializations=mats)
                yield SSE.patch_elements(status_html, selector="#asset-status-region")
                yield SSE.patch_elements(mat_html, selector="#mat-history")
            except Exception as e:
                logger.exception("refresh failed for asset %d", asset_id)
                yield SSE.patch_signals({"refreshError": str(e)})
            finally:
                yield SSE.patch_signals({"refreshing": False})

        return DatastarResponse(generate())

    @ui.post("/reconcile")
    async def reconcile_stream():
        async def generate():
            yield SSE.patch_signals({"reconciling": True})
            try:
                async with reconcile_lock:
                    result = await asyncio.to_thread(lambda: service.run_reconcile(_store(), repo_root))
                html = templates.get_template("components/reconcile_result.html").render(result=result)
                yield SSE.patch_elements(html, selector="#reconcile-result")
                yield SSE.patch_signals({"reconcileOpen": True})
            except Exception as e:
                logger.exception("reconcile failed")
                yield SSE.patch_signals({"reconcileError": str(e)})
            finally:
                yield SSE.patch_signals({"reconciling": False})

        return DatastarResponse(generate())

    @ui.post("/sensors/{sensor_id}/trigger")
    async def sensor_trigger_stream(sensor_id: int):
        async def generate():
            yield SSE.patch_signals({"triggering": True, "triggerError": ""})
            try:

                def _run():
                    return service.trigger_sensor(_store(), repo_root, sensor_id)

                obs = await asyncio.to_thread(_run)
                obs_html = templates.get_template("components/sensor_observation.html").render(obs=obs, label="Latest trigger")
                yield SSE.patch_elements(obs_html, selector="#trigger-result")
                # Reload observation history
                all_obs = await asyncio.to_thread(lambda: service.get_sensor_observations(_store(), sensor_id, limit=50))
                history_html = templates.get_template("components/obs_table.html").render(observations=all_obs)
                yield SSE.patch_elements(history_html, selector="#obs-history")
            except Exception as e:
                logger.exception("sensor trigger failed for %d", sensor_id)
                yield SSE.patch_signals({"triggerError": str(e)})
            finally:
                yield SSE.patch_signals({"triggering": False})

        return DatastarResponse(generate())

    # --- Live watch streams (event-driven via notifier) ---

    @ui.get("/assets/{asset_id}/watch")
    async def asset_watch(asset_id: int):
        async def generate():
            last_hash: str | None = None
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
                        return service.get_asset(s, aid), s.list_materializations(aid, limit=20)

                    detail, mats = await asyncio.to_thread(_q)
                except Exception:
                    continue
                current_hash = detail.latest_materialization.run_hash if detail.latest_materialization else None
                if current_hash != last_hash:
                    last_hash = current_hash
                    yield SSE.patch_elements(
                        templates.get_template("components/asset_status_region.html").render(detail=detail),
                        selector="#asset-status-region",
                    )
                    yield SSE.patch_elements(
                        templates.get_template("components/mat_table.html").render(materializations=mats),
                        selector="#mat-history",
                    )

        return DatastarResponse(generate())

    # ------------------------------------------------------------------
    # Register routers + static files (order matters: routers before static)
    # ------------------------------------------------------------------

    app.include_router(api)
    app.include_router(ui)

    if _STATIC_DIR.exists():
        app.mount("/static", StaticFiles(directory=str(_STATIC_DIR)), name="static")

    return app
