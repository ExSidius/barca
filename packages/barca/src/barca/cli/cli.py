"""Barca CLI — typer app."""

from __future__ import annotations

import sys
from pathlib import Path

import typer

from barca._engine import reindex as do_reindex, refresh as do_refresh, reset as do_reset, trigger_sensor as do_trigger_sensor
from barca._models import JobDetail
from barca._reconciler import reconcile as do_reconcile
from barca._store import MetadataStore

from barca.cli.display import asset_detail, assets_table, job_detail, jobs_table, reconcile_summary, sensor_observations_table


def _check_gil() -> None:
    """Warn if running with the GIL enabled (parallel perf will suffer)."""
    is_gil_enabled = getattr(sys, "_is_gil_enabled", None)
    if is_gil_enabled is not None and is_gil_enabled():
        typer.echo(
            "barca: WARNING: GIL is enabled. For best parallel performance, "
            "use the free-threaded build (python3.14t) or set PYTHON_GIL=0.",
            err=True,
        )


app = typer.Typer(add_completion=False, callback=_check_gil)
assets_app = typer.Typer(add_completion=False)
jobs_app = typer.Typer(add_completion=False)
sensors_app = typer.Typer(add_completion=False)
app.add_typer(assets_app, name="assets")
app.add_typer(jobs_app, name="jobs")
app.add_typer(sensors_app, name="sensors")


def _repo_root() -> Path:
    return Path.cwd()


def _store() -> MetadataStore:
    root = _repo_root()
    db_path = root / ".barca" / "metadata.db"
    return MetadataStore(str(db_path))


@app.command()
def reindex() -> None:
    """Discover and index all barca assets."""
    root = _repo_root()
    store = _store()
    assets = do_reindex(store, root)
    typer.echo(assets_table(assets))


@app.command()
def reset(
    db: bool = typer.Option(False, "--db", help="Remove .barca/ (database)"),
    artifacts: bool = typer.Option(False, "--artifacts", help="Remove .barcafiles/ (artifacts)"),
    tmp: bool = typer.Option(False, "--tmp", help="Remove tmp/"),
) -> None:
    """Remove generated files and caches."""
    root = _repo_root()
    output = do_reset(root, db=db, artifacts=artifacts, tmp=tmp)
    typer.echo(output, nl=False)


@app.command()
def reconcile(
    watch: bool = typer.Option(False, "--watch", help="Run continuously"),
    interval: int = typer.Option(60, "--interval", help="Seconds between reconcile passes (with --watch)"),
) -> None:
    """Run a single reconciliation pass (or loop with --watch)."""
    import time

    root = _repo_root()
    while True:
        store = _store()
        result = do_reconcile(store, root)
        typer.echo(reconcile_summary(result))
        if not watch:
            break
        typer.echo(f"Sleeping {interval}s...")
        time.sleep(interval)


@app.command()
def serve(
    port: int = typer.Option(8400, "--port", help="HTTP port"),
    interval: int = typer.Option(60, "--interval", help="Seconds between reconcile passes"),
    log_level: str = typer.Option("info", "--log-level", help="Log level (debug, info, warning, error)"),
) -> None:
    """Start the barca server (HTTP API + background scheduler)."""
    try:
        import uvicorn
        from barca.server.app import create_app
    except ImportError:
        typer.echo("barca[server] is required for `barca serve`. Install with: uv add barca[server]", err=True)
        raise typer.Exit(1)

    root = _repo_root()
    application = create_app(repo_root=root, interval=interval, log_level=log_level)
    uvicorn.run(application, host="0.0.0.0", port=port)


@assets_app.command("list")
def assets_list() -> None:
    """List all indexed assets."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    assets = store.list_assets()
    typer.echo(assets_table(assets))


@assets_app.command("show")
def assets_show(asset_id: int) -> None:
    """Show details for an asset."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    detail = store.asset_detail(asset_id)
    typer.echo(asset_detail(detail))


@assets_app.command("refresh")
def assets_refresh(
    asset_id: int = typer.Argument(None, help="Asset ID to refresh"),
    name: str = typer.Option(None, "-n", "--name", help="Asset name substring to match"),
    jobs: int = typer.Option(None, "-j", "--jobs", help="Max parallel workers (default: cpu_count)"),
) -> None:
    """Materialize an asset (and upstream deps)."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)

    if name is not None:
        assets = store.list_assets()
        matches = [a for a in assets if name in a.logical_name or name in a.function_name]
        if not matches:
            typer.echo(f"No asset matching '{name}'", err=True)
            raise typer.Exit(1)
        if len(matches) > 1:
            typer.echo(f"Multiple assets match '{name}':", err=True)
            for m in matches:
                typer.echo(f"  {m.asset_id}: {m.logical_name}", err=True)
            raise typer.Exit(1)
        asset_id = matches[0].asset_id
    elif asset_id is None:
        typer.echo("Provide an asset ID or --name", err=True)
        raise typer.Exit(1)

    result = do_refresh(store, root, asset_id, max_workers=jobs)
    typer.echo(asset_detail(result))


@jobs_app.command("list")
def jobs_list() -> None:
    """List recent jobs."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    pairs = store.list_recent_materializations(50)
    jobs = [JobDetail(job=mat, asset=summary) for mat, summary in pairs]
    typer.echo(jobs_table(jobs))


@jobs_app.command("show")
def jobs_show(job_id: int) -> None:
    """Show details for a job."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    mat, summary = store.get_materialization_with_asset(job_id)
    detail = JobDetail(job=mat, asset=summary)
    typer.echo(job_detail(detail))


@sensors_app.command("list")
def sensors_list() -> None:
    """List all indexed sensors."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    assets = store.list_assets()
    sensors = [a for a in assets if a.kind == "sensor"]
    typer.echo(assets_table(sensors))


@sensors_app.command("show")
def sensors_show(sensor_id: int) -> None:
    """Show details and observation history for a sensor."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    detail = store.asset_detail(sensor_id)
    if detail.asset.kind != "sensor":
        typer.echo(f"Asset #{sensor_id} is not a sensor (kind: {detail.asset.kind})", err=True)
        raise typer.Exit(1)
    typer.echo(asset_detail(detail))
    observations = store.list_sensor_observations(sensor_id)
    if observations:
        typer.echo("\nObservation history:")
        typer.echo(sensor_observations_table(observations))


@sensors_app.command("trigger")
def sensors_trigger(
    sensor_id: int = typer.Argument(None, help="Sensor ID to trigger"),
    name: str = typer.Option(None, "-n", "--name", help="Sensor name substring to match"),
) -> None:
    """Manually trigger a sensor and record an observation."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)

    if name is not None:
        assets = store.list_assets()
        matches = [a for a in assets if a.kind == "sensor" and (name in a.logical_name or name in a.function_name)]
        if not matches:
            typer.echo(f"No sensor matching '{name}'", err=True)
            raise typer.Exit(1)
        if len(matches) > 1:
            typer.echo(f"Multiple sensors match '{name}':", err=True)
            for m in matches:
                typer.echo(f"  {m.asset_id}: {m.logical_name}", err=True)
            raise typer.Exit(1)
        sensor_id = matches[0].asset_id
    elif sensor_id is None:
        typer.echo("Provide a sensor ID or --name", err=True)
        raise typer.Exit(1)

    obs = do_trigger_sensor(store, root, sensor_id)
    typer.echo(f"Observation #{obs.observation_id} recorded (update_detected: {obs.update_detected})")
