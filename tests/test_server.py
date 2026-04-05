"""Tests for barca server endpoints using niquests."""

from __future__ import annotations

import sys
import textwrap
import threading
import time
from pathlib import Path

import niquests
import pytest
import uvicorn

from barca_server.app import create_app


@pytest.fixture
def server_project(tmp_path):
    """Create a project with always-scheduled assets for server testing."""
    project_dir = tmp_path / "serverproject"
    project_dir.mkdir()

    mod_dir = project_dir / "srvmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(textwrap.dedent("""\
        from barca import asset

        @asset(schedule="always")
        def greeting() -> dict:
            return {"message": "hello from server"}

        @asset(schedule="manual")
        def manual_only() -> dict:
            return {"message": "manual"}
    """))

    (project_dir / "barca.toml").write_text(textwrap.dedent("""\
        [project]
        modules = ["srvmod.assets"]
    """))

    to_remove = [k for k in sys.modules if k == "srvmod" or k.startswith("srvmod.")]
    for k in to_remove:
        del sys.modules[k]

    sys.path.insert(0, str(project_dir))
    yield project_dir
    sys.path.remove(str(project_dir))
    to_remove = [k for k in sys.modules if k == "srvmod" or k.startswith("srvmod.")]
    for k in to_remove:
        del sys.modules[k]
    from barca._trace import clear_caches
    clear_caches()


@pytest.fixture
def server_url(server_project):
    """Start the barca server in a background thread, yield its base URL."""
    app = create_app(repo_root=server_project, interval=3600, log_level="warning")

    config = uvicorn.Config(app, host="127.0.0.1", port=0, log_level="warning")
    server = uvicorn.Server(config)

    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()

    # Wait for server to start
    for _ in range(50):
        time.sleep(0.1)
        if server.started:
            break
    else:
        raise RuntimeError("server did not start in time")

    # Get the actual bound port
    sockets = server.servers[0].sockets if server.servers else []
    port = sockets[0].getsockname()[1] if sockets else 8400

    base_url = f"http://127.0.0.1:{port}"
    yield base_url

    server.should_exit = True
    thread.join(timeout=5)


def test_health(server_url):
    resp = niquests.get(f"{server_url}/health")
    assert resp.status_code == 200
    data = resp.json()
    assert data["status"] == "ok"
    assert "scheduler_running" in data


def test_assets_list(server_url):
    resp = niquests.get(f"{server_url}/assets")
    assert resp.status_code == 200
    assets = resp.json()
    assert isinstance(assets, list)
    assert len(assets) >= 2
    names = {a["function_name"] for a in assets}
    assert "greeting" in names
    assert "manual_only" in names


def test_asset_detail(server_url):
    # First get asset list to find an ID
    assets = niquests.get(f"{server_url}/assets").json()
    asset_id = assets[0]["asset_id"]

    resp = niquests.get(f"{server_url}/assets/{asset_id}")
    assert resp.status_code == 200
    detail = resp.json()
    assert "asset" in detail
    assert detail["asset"]["asset_id"] == asset_id


def test_asset_detail_not_found(server_url):
    resp = niquests.get(f"{server_url}/assets/99999")
    assert resp.status_code == 404


def test_reconcile(server_url):
    resp = niquests.post(f"{server_url}/reconcile")
    assert resp.status_code == 200
    result = resp.json()
    assert "executed_assets" in result
    assert "executed_sensors" in result
    assert "fresh" in result


def test_refresh_asset(server_url):
    # Get an asset to refresh
    assets = niquests.get(f"{server_url}/assets").json()
    greeting = [a for a in assets if a["function_name"] == "greeting"][0]

    resp = niquests.post(f"{server_url}/assets/{greeting['asset_id']}/refresh")
    assert resp.status_code == 200
    detail = resp.json()
    assert detail["latest_materialization"] is not None
    assert detail["latest_materialization"]["status"] == "success"


def test_jobs_list(server_url):
    # Trigger a refresh first so there's at least one job
    assets = niquests.get(f"{server_url}/assets").json()
    niquests.post(f"{server_url}/assets/{assets[0]['asset_id']}/refresh")

    resp = niquests.get(f"{server_url}/jobs")
    assert resp.status_code == 200
    jobs = resp.json()
    assert isinstance(jobs, list)
    assert len(jobs) >= 1


def test_job_detail(server_url):
    # Trigger a refresh first
    assets = niquests.get(f"{server_url}/assets").json()
    niquests.post(f"{server_url}/assets/{assets[0]['asset_id']}/refresh")

    jobs = niquests.get(f"{server_url}/jobs").json()
    job_id = jobs[0]["job"]["materialization_id"]

    resp = niquests.get(f"{server_url}/jobs/{job_id}")
    assert resp.status_code == 200
    detail = resp.json()
    assert detail["job"]["materialization_id"] == job_id


def test_job_not_found(server_url):
    resp = niquests.get(f"{server_url}/jobs/99999")
    assert resp.status_code == 404
