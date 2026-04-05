"""Barca CLI — typer app."""

from __future__ import annotations

import sys
from pathlib import Path

import typer

from barca._engine import reindex as do_reindex, refresh as do_refresh, reset as do_reset
from barca._models import JobDetail
from barca._reconciler import reconcile as do_reconcile
from barca._store import MetadataStore

from barca_cli.display import asset_detail, assets_table, job_detail, jobs_table, reconcile_summary


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
app.add_typer(assets_app, name="assets")
app.add_typer(jobs_app, name="jobs")


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
