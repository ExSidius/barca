"""Barca CLI — typer app."""

from __future__ import annotations

import os
import sys
from pathlib import Path

import typer
from rich.console import Console

from barca._dev import dev_watch as do_dev_watch
from barca._engine import refresh as do_refresh
from barca._engine import reindex as do_reindex
from barca._engine import reset as do_reset
from barca._engine import trigger_sensor as do_trigger_sensor
from barca._models import JobDetail, StaleUpstreamError
from barca._prune import prune as do_prune
from barca._run import run_loop as do_run_loop
from barca._run import run_pass as do_run_pass
from barca._store import MetadataStore
from barca.cli.display import (
    asset_detail,
    assets_table,
    job_detail,
    jobs_table,
    reindex_diff_panel,
    run_pass_summary,
    sensor_observations_table,
)

_console = Console()
_err = Console(stderr=True)


def _check_gil() -> None:
    """Warn if running with the GIL enabled."""
    is_gil_enabled = getattr(sys, "_is_gil_enabled", None)
    if is_gil_enabled is None or not is_gil_enabled():
        return
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
    try:
        return MetadataStore(str(db_path))
    except RuntimeError as exc:
        _console.print(f"[bold red]✗ Schema error:[/bold red] {exc}", err=True)
        raise typer.Exit(1) from exc


# ============================================================
# Top-level commands
# ============================================================


@app.command()
def reindex() -> None:
    """Discover assets and show the three-way diff of what changed."""
    root = _repo_root()
    store = _store()
    diff = do_reindex(store, root)
    _console.print(reindex_diff_panel(diff))
    _console.print(assets_table(store.list_assets()))


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
def run(
    once: bool = typer.Option(False, "--once", help="Run a single pass and exit"),
    interval: float = typer.Option(0.5, "--interval", help="Seconds between passes in loop mode"),
) -> None:
    """Run production mode: maintain the DAG at declared freshness levels.

    Without ``--once``, ``barca run`` is a long-running process that
    continuously calls ``run_pass``. Use Ctrl-C to stop.
    """
    root = _repo_root()
    store = _store()
    if once:
        result = do_run_pass(store, root)
        _console.print(run_pass_summary(result))
        return

    # Long-running loop — block until Ctrl-C
    from threading import Event

    stop = Event()
    try:
        do_run_loop(store, root, stop_event=stop, interval=interval)
    except KeyboardInterrupt:
        stop.set()
        _console.print("\n[dim]run loop stopped[/dim]")


@app.command()
def dev() -> None:
    """Run development mode: watch for file changes and update staleness live.

    Dev mode never materialises anything — it only tracks which assets are
    stale. Use ``barca run`` or ``barca assets refresh`` to actually
    materialise.
    """
    root = _repo_root()
    store = _store()
    from threading import Event

    stop = Event()
    _console.print("[dim]barca dev: watching for changes... Ctrl-C to stop[/dim]")
    try:
        do_dev_watch(store, root, stop_event=stop)
    except KeyboardInterrupt:
        stop.set()
        _console.print("\n[dim]dev mode stopped[/dim]")


@app.command()
def prune(
    yes: bool = typer.Option(False, "--yes", "-y", help="Skip confirmation prompt"),
) -> None:
    """Remove unreachable history and artifacts.

    This is the only operation that permanently deletes materialisation
    history. Recommended before production deployments or to recover
    disk space.
    """
    root = _repo_root()
    store = _store()

    if not yes:
        confirm = typer.confirm(
            "barca prune will permanently delete history for removed assets. Continue?",
            default=False,
        )
        if not confirm:
            _console.print("[dim]aborted[/dim]")
            return

    result = do_prune(store, root)
    if result.removed_assets == 0 and result.removed_materializations == 0 and result.removed_artifact_files == 0:
        _console.print("[dim]nothing to prune[/dim]")
    else:
        _console.print(
            f"pruned [bold]{result.removed_assets}[/bold] assets, [bold]{result.removed_materializations}[/bold] materializations, [bold]{result.removed_artifact_files}[/bold] artifact dirs"
        )


@app.command()
def serve(
    port: int = typer.Option(8400, "--port", help="HTTP port"),
    interval: int = typer.Option(60, "--interval", help="Seconds between background passes"),
    log_level: str = typer.Option("info", "--log-level", help="Log level"),
) -> None:
    """Start the barca HTTP server with background run_pass scheduler."""
    import uvicorn

    from barca.server.app import create_app

    root = _repo_root()
    application = create_app(repo_root=root, interval=interval, log_level=log_level)
    uvicorn.run(application, host="0.0.0.0", port=port)


# ============================================================
# assets subcommands
# ============================================================


@assets_app.command("list")
def assets_list() -> None:
    """List all indexed assets."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)
    _console.print(assets_table(store.list_assets()))


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
    asset_id: str = typer.Argument(None, help="Asset ID or function name to refresh"),
    name: str = typer.Option(None, "-n", "--name", help="Asset name substring to match"),
    jobs: int = typer.Option(None, "-j", "--jobs", help="Max parallel workers (default: cpu_count)"),
    stale_policy: str = typer.Option(
        "error",
        "--stale-policy",
        help="How to handle stale upstreams: error|warn|pass (default: error)",
    ),
) -> None:
    """Materialize an asset explicitly.

    Does NOT cascade upstream. If upstream is stale, the behaviour depends
    on ``--stale-policy``:

    - ``error`` (default): abort with an error listing stale upstreams
    - ``warn``: proceed with stale inputs and emit a warning
    - ``pass``: proceed silently with stale inputs
    """
    if stale_policy not in ("error", "warn", "pass"):
        _err.print(
            f"invalid --stale-policy={stale_policy!r}; must be error|warn|pass",
            style="bold red",
        )
        raise typer.Exit(2)

    root = _repo_root()
    store = _store()
    do_reindex(store, root)

    # Accept either an integer id OR a function name as a positional argument.
    resolved_id: int | None = None
    if asset_id is not None:
        if isinstance(asset_id, int):
            resolved_id = asset_id
        else:
            try:
                resolved_id = int(str(asset_id))
            except ValueError as exc:
                matches = [a for a in store.list_assets() if a.function_name == asset_id or a.logical_name == asset_id]
                if not matches:
                    _err.print(f"No asset matching '{asset_id}'", style="bold red")
                    raise typer.Exit(1) from exc
                if len(matches) > 1:
                    _err.print(f"Multiple assets match '{asset_id}'", style="bold red")
                    raise typer.Exit(1) from exc
                resolved_id = matches[0].asset_id

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
        resolved_id = matches[0].asset_id

    if resolved_id is None:
        _err.print("Provide an asset ID/name or --name", style="bold red")
        raise typer.Exit(1)

    try:
        result = do_refresh(store, root, resolved_id, max_workers=jobs, stale_policy=stale_policy)
    except StaleUpstreamError as exc:
        _err.print(f"[bold red]stale upstream:[/bold red] {exc}", style="red")
        raise typer.Exit(1) from exc
    _console.print(asset_detail(result, repo_root=root))


# ============================================================
# jobs subcommands
# ============================================================


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


# ============================================================
# sensors subcommands
# ============================================================


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
        _err.print(
            f"Asset #{sensor_id} is not a sensor (kind: {detail.asset.kind})",
            style="bold red",
        )
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
