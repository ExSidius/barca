"""Fixtures for UI route contract tests."""

from __future__ import annotations

import sys
import textwrap

import pytest
from fastapi.testclient import TestClient

from barca._engine import reindex
from barca._store import MetadataStore
from barca.server.app import create_app


@pytest.fixture(scope="module")
def ui_project(tmp_path_factory):
    """Minimal project with one asset and one sensor for UI contract tests."""
    tmp_path = tmp_path_factory.mktemp("ui_project")
    project_dir = tmp_path / "uiproject"
    project_dir.mkdir()

    mod_dir = project_dir / "uimod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset, sensor

        @asset(schedule="manual")
        def my_asset() -> dict:
            return {"value": 42}

        @sensor(schedule="always")
        def my_sensor():
            return (True, {"detected": True})
    """)
    )

    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["uimod.pipeline"]
    """)
    )

    to_remove = [k for k in sys.modules if k == "uimod" or k.startswith("uimod.")]
    for k in to_remove:
        del sys.modules[k]

    sys.path.insert(0, str(project_dir))

    # Index the project
    db_path = project_dir / ".barca" / "metadata.db"
    store = MetadataStore(str(db_path))
    reindex(store, project_dir)

    yield project_dir

    sys.path.remove(str(project_dir))
    to_remove = [k for k in sys.modules if k == "uimod" or k.startswith("uimod.")]
    for k in to_remove:
        del sys.modules[k]
    from barca._trace import clear_caches

    clear_caches()


@pytest.fixture(scope="module")
def client(ui_project):
    """TestClient for the barca FastAPI app."""
    app = create_app(repo_root=ui_project, interval=3600, log_level="warning")
    with TestClient(app, raise_server_exceptions=True) as c:
        yield c


@pytest.fixture(scope="module")
def asset_id(client):
    """Return the asset_id of my_asset."""
    r = client.get("/api/assets")
    assert r.status_code == 200
    assets = r.json()
    asset = next(a for a in assets if a["function_name"] == "my_asset")
    return asset["asset_id"]


@pytest.fixture(scope="module")
def sensor_id(client):
    """Return the asset_id of my_sensor."""
    r = client.get("/api/assets")
    assert r.status_code == 200
    assets = r.json()
    sensor = next(a for a in assets if a["function_name"] == "my_sensor")
    return sensor["asset_id"]
