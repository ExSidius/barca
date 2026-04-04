"""Barca CLI — typer app."""

from __future__ import annotations

from pathlib import Path

import typer

from barca._engine import reindex as do_reindex, refresh as do_refresh, reset as do_reset
from barca._models import JobDetail
from barca._store import MetadataStore

from barca_cli.display import asset_detail, assets_table, job_detail, jobs_table

app = typer.Typer(add_completion=False)
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
def assets_refresh(asset_id: int) -> None:
    """Materialize an asset (and upstream deps)."""
    root = _repo_root()
    store = _store()
    do_reindex(store, root)

    # Check if already fresh
    detail = store.asset_detail(asset_id)
    pending = store.count_pending_materializations(asset_id)

    result = do_refresh(store, root, asset_id)
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
