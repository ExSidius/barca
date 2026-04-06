"""Barca CLI — typer app."""

from __future__ import annotations

import os
import sys
from pathlib import Path

import typer
from rich.console import Console
from rich.text import Text

from barca._engine import refresh as do_refresh
from barca._engine import reindex as do_reindex
from barca._engine import reset as do_reset
from barca._engine import trigger_sensor as do_trigger_sensor
from barca._models import JobDetail
from barca._reconciler import reconcile as do_reconcile
from barca._store import MetadataStore
from barca.cli.display import asset_detail, assets_table, job_detail, jobs_table, reconcile_summary, sensor_observations_table

_console = Console()
_err = Console(stderr=True)


def _check_gil() -> None:
    """Warn if running with the GIL enabled (parallel perf will suffer)."""
    is_gil_enabled = getattr(sys, "_is_gil_enabled", None)
    if is_gil_enabled is None or not is_gil_enabled():
        return
    # Suppress the warning if the user already set PYTHON_GIL=0 — the GIL may
    # have been re-enabled by a C extension (e.g. turso) that hasn't declared
    # Py_mod_gil, which is an upstream issue, not a misconfiguration.
    if os.environ.get("PYTHON_GIL") == "0":
        return
    _err.print(
        "barca: WARNING: GIL is enabled. For best parallel performance, use the free-threaded build (python3.14t) and set PYTHON_GIL=0.",
        style="yellow",
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
    _console.print(assets_table(assets))


@app.command()
def reset(
    db: bool = typer.Option(False, "--db", help="Remove .barca/ (database)"),
    artifacts: bool = typer.Option(False, "--artifacts", help="Remove .barcafiles/ (artifacts)"),
    tmp: bool = typer.Option(False, "--tmp", help="Remove tmp/"),
) -> None:
    """Remove generated files and caches."""
    root = _repo_root()
    output = do_reset(root, db=db, artifacts=artifacts, tmp=tmp)
    _console.print(output, end="")


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
        _console.print(reconcile_summary(result))
        if not watch:
            break
        _console.print(Text(f"Sleeping {interval}s...", style="dim"))
        time.sleep(interval)


@app.command()
def serve(
    port: int = typer.Option(8400, "--port", help="HTTP port"),
    interval: int = typer.Option(60, "--interval", help="Seconds between reconcile passes"),
    log_level: str = typer.Option("info", "--log-level", help="Log level (debug, info, warning, error)"),
) -> None:
    """Start the barca server (HTTP API + background scheduler)."""
    import uvicorn

    from barca.server.app import create_app

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
    _console.print(assets_table(assets))


@assets_app.command("show")
def assets_show(asset_id: int) -> None:
    """Show details for an asset."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    detail = store.asset_detail(asset_id)
    _console.print(asset_detail(detail, repo_root=root))


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
            _err.print(f"No asset matching '{name}'", style="bold red")
            raise typer.Exit(1)
        if len(matches) > 1:
            _err.print(f"Multiple assets match '{name}':", style="bold red")
            for m in matches:
                _err.print(f"  {m.asset_id}: {m.logical_name}", style="dim")
            raise typer.Exit(1)
        asset_id = matches[0].asset_id
    elif asset_id is None:
        _err.print("Provide an asset ID or --name", style="bold red")
        raise typer.Exit(1)

    result = do_refresh(store, root, asset_id, max_workers=jobs)
    _console.print(asset_detail(result, repo_root=root))


@jobs_app.command("list")
def jobs_list() -> None:
    """List recent jobs."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    pairs = store.list_recent_materializations(50)
    jobs = [JobDetail(job=mat, asset=summary) for mat, summary in pairs]
    _console.print(jobs_table(jobs))


@jobs_app.command("show")
def jobs_show(job_id: int) -> None:
    """Show details for a job."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    mat, summary = store.get_materialization_with_asset(job_id)
    detail = JobDetail(job=mat, asset=summary)
    _console.print(job_detail(detail))


@sensors_app.command("list")
def sensors_list() -> None:
    """List all indexed sensors."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    assets = store.list_assets()
    sensors = [a for a in assets if a.kind == "sensor"]
    _console.print(assets_table(sensors))


@sensors_app.command("show")
def sensors_show(sensor_id: int) -> None:
    """Show details and observation history for a sensor."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    detail = store.asset_detail(sensor_id)
    if detail.asset.kind != "sensor":
        _err.print(f"Asset #{sensor_id} is not a sensor (kind: {detail.asset.kind})", style="bold red")
        raise typer.Exit(1)
    _console.print(asset_detail(detail))
    observations = store.list_sensor_observations(sensor_id)
    if observations:
        _console.print("\n[bold]Observation history:[/bold]")
        _console.print(sensor_observations_table(observations))


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
            _err.print(f"No sensor matching '{name}'", style="bold red")
            raise typer.Exit(1)
        if len(matches) > 1:
            _err.print(f"Multiple sensors match '{name}':", style="bold red")
            for m in matches:
                _err.print(f"  {m.asset_id}: {m.logical_name}", style="dim")
            raise typer.Exit(1)
        sensor_id = matches[0].asset_id
    elif sensor_id is None:
        _err.print("Provide a sensor ID or --name", style="bold red")
        raise typer.Exit(1)

    obs = do_trigger_sensor(store, root, sensor_id)
    update_style = "green" if obs.update_detected else "dim"
    _console.print(f"Observation [dim]#{obs.observation_id}[/dim] recorded (update_detected: [{update_style}]{obs.update_detected}[/{update_style}])")
