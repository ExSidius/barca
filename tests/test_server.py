"""Tests for barca server endpoints using niquests."""

from __future__ import annotations

import sys
import textwrap
import threading
import time

import niquests
import pytest
import uvicorn

from barca.server.app import create_app


@pytest.fixture
def server_project(tmp_path):
    """Create a project with always-scheduled assets for server testing."""
    project_dir = tmp_path / "serverproject"
    project_dir.mkdir()

    mod_dir = project_dir / "srvmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always, Manual

        @asset(freshness=Always())
        def greeting() -> dict:
            return {"message": "hello from server"}

        @asset(freshness=Manual())
        def manual_only() -> dict:
            return {"message": "manual"}
    """)
    )

    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["srvmod.assets"]
    """)
    )

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
    resp = niquests.get(f"{server_url}/api/assets")
    assert resp.status_code == 200
    assets = resp.json()
    assert isinstance(assets, list)
    assert len(assets) >= 2
    names = {a["function_name"] for a in assets}
    assert "greeting" in names
    assert "manual_only" in names


def test_asset_detail(server_url):
    # First get asset list to find an ID
    assets = niquests.get(f"{server_url}/api/assets").json()
    asset_id = assets[0]["asset_id"]

    resp = niquests.get(f"{server_url}/api/assets/{asset_id}")
    assert resp.status_code == 200
    detail = resp.json()
    assert "asset" in detail
    assert detail["asset"]["asset_id"] == asset_id


def test_asset_detail_not_found(server_url):
    resp = niquests.get(f"{server_url}/api/assets/99999")
    assert resp.status_code == 404


def test_run_pass(server_url):
    """POST /api/run/pass triggers a single run_pass and returns a RunPassResult."""
    resp = niquests.post(f"{server_url}/api/run/pass")
    assert resp.status_code == 200
    result = resp.json()
    assert "executed_assets" in result
    assert "executed_sensors" in result
    assert "fresh" in result


def test_prune(server_url):
    """POST /api/prune removes unreachable history."""
    resp = niquests.post(f"{server_url}/api/prune")
    assert resp.status_code == 200
    result = resp.json()
    assert "removed_assets" in result or "removed_materializations" in result


def test_refresh_asset_with_stale_policy(server_url):
    """POST /api/assets/{id}/refresh?stale_policy=warn proceeds with stale inputs."""
    assets = niquests.get(f"{server_url}/api/assets").json()
    asset_id = assets[0]["asset_id"]

    # Default policy is error, but with no stale upstreams this should work
    resp = niquests.post(f"{server_url}/api/assets/{asset_id}/refresh?stale_policy=warn")
    assert resp.status_code == 200


def test_refresh_asset(server_url):
    # Get an asset to refresh
    assets = niquests.get(f"{server_url}/api/assets").json()
    greeting = next(a for a in assets if a["function_name"] == "greeting")

    resp = niquests.post(f"{server_url}/api/assets/{greeting['asset_id']}/refresh")
    assert resp.status_code == 200
    detail = resp.json()
    assert detail["latest_materialization"] is not None
    assert detail["latest_materialization"]["status"] == "success"


def test_jobs_list(server_url):
    # Trigger a refresh first so there's at least one job
    assets = niquests.get(f"{server_url}/api/assets").json()
    niquests.post(f"{server_url}/api/assets/{assets[0]['asset_id']}/refresh")

    resp = niquests.get(f"{server_url}/api/jobs")
    assert resp.status_code == 200
    jobs = resp.json()
    assert isinstance(jobs, list)
    assert len(jobs) >= 1


def test_job_detail(server_url):
    # Trigger a refresh first
    assets = niquests.get(f"{server_url}/api/assets").json()
    niquests.post(f"{server_url}/api/assets/{assets[0]['asset_id']}/refresh")

    jobs = niquests.get(f"{server_url}/api/jobs").json()
    job_id = jobs[0]["job"]["materialization_id"]

    resp = niquests.get(f"{server_url}/api/jobs/{job_id}")
    assert resp.status_code == 200
    detail = resp.json()
    assert detail["job"]["materialization_id"] == job_id


def test_job_not_found(server_url):
    resp = niquests.get(f"{server_url}/api/jobs/99999")
    assert resp.status_code == 404


# ------------------------------------------------------------------
# Sensor endpoint tests
# ------------------------------------------------------------------


@pytest.fixture
def sensor_server_project(tmp_path):
    """Create a project with a file-timestamp sensor and downstream asset."""
    project_dir = tmp_path / "sensorserverproject"
    project_dir.mkdir()

    # Create a data file the sensor will watch
    data_file = project_dir / "data.txt"
    data_file.write_text("initial content")

    mod_dir = project_dir / "senmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent(f"""\
        import os
        from barca import sensor, asset, Always, Schedule

        DATA_FILE = r"{data_file}"

        @sensor(freshness=Schedule("* * * * *"))
        def file_watcher():
            mtime = os.path.getmtime(DATA_FILE)
            content = open(DATA_FILE).read()
            return (True, {{"mtime": mtime, "content": content}})

        @asset(inputs={{"data": file_watcher}}, freshness=Always())
        def transform(data):
            update_detected, payload = data
            return {{"upper": payload["content"].upper(), "mtime": payload["mtime"]}}
    """)
    )

    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["senmod.pipeline"]
    """)
    )

    to_remove = [k for k in sys.modules if k == "senmod" or k.startswith("senmod.")]
    for k in to_remove:
        del sys.modules[k]

    sys.path.insert(0, str(project_dir))
    yield project_dir, data_file
    sys.path.remove(str(project_dir))
    to_remove = [k for k in sys.modules if k == "senmod" or k.startswith("senmod.")]
    for k in to_remove:
        del sys.modules[k]
    from barca._trace import clear_caches

    clear_caches()


@pytest.fixture
def sensor_server_url(sensor_server_project):
    """Start a server with sensor project."""
    project_dir, _ = sensor_server_project
    app = create_app(repo_root=project_dir, interval=3600, log_level="warning")

    config = uvicorn.Config(app, host="127.0.0.1", port=0, log_level="warning")
    server = uvicorn.Server(config)

    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()

    for _ in range(50):
        time.sleep(0.1)
        if server.started:
            break
    else:
        raise RuntimeError("server did not start in time")

    sockets = server.servers[0].sockets if server.servers else []
    port = sockets[0].getsockname()[1] if sockets else 8400

    base_url = f"http://127.0.0.1:{port}"
    yield base_url

    server.should_exit = True
    thread.join(timeout=5)


def test_sensors_list_endpoint(sensor_server_url):
    """GET /api/sensors returns only sensor-kind nodes."""
    resp = niquests.get(f"{sensor_server_url}/api/sensors")
    assert resp.status_code == 200
    sensors = resp.json()
    assert isinstance(sensors, list)
    assert len(sensors) >= 1
    assert all(s["kind"] == "sensor" for s in sensors)
    assert any(s["function_name"] == "file_watcher" for s in sensors)


def test_sensor_trigger_endpoint(sensor_server_url):
    """POST /api/sensors/{id}/trigger returns an observation."""
    sensors = niquests.get(f"{sensor_server_url}/api/sensors").json()
    sensor_id = sensors[0]["asset_id"]

    resp = niquests.post(f"{sensor_server_url}/api/sensors/{sensor_id}/trigger")
    assert resp.status_code == 200
    obs = resp.json()
    assert obs["update_detected"] is True
    assert obs["output_json"] is not None


def test_sensor_observations_endpoint(sensor_server_url):
    """GET /api/sensors/{id}/observations returns observation history."""
    sensors = niquests.get(f"{sensor_server_url}/api/sensors").json()
    sensor_id = sensors[0]["asset_id"]

    # Trigger twice to create history
    niquests.post(f"{sensor_server_url}/api/sensors/{sensor_id}/trigger")
    niquests.post(f"{sensor_server_url}/api/sensors/{sensor_id}/trigger")

    resp = niquests.get(f"{sensor_server_url}/api/sensors/{sensor_id}/observations")
    assert resp.status_code == 200
    observations = resp.json()
    assert len(observations) >= 2


def test_sensor_trigger_rejects_non_sensor(sensor_server_url):
    """POST /api/sensors/{id}/trigger returns 404 for non-sensor assets."""
    assets = niquests.get(f"{sensor_server_url}/api/assets").json()
    non_sensor = next(a for a in assets if a["kind"] != "sensor")

    resp = niquests.post(f"{sensor_server_url}/api/sensors/{non_sensor['asset_id']}/trigger")
    assert resp.status_code == 404


def test_e2e_sensor_run_pass_rematerialize(sensor_server_project, sensor_server_url):
    """Full e2e: sensor observations accumulate, downstream asset materializes.

    The sensor uses ``Schedule("* * * * *")`` (every minute). Two run_passes
    in the same second won't cross a cron tick, so for the second pass we
    use the explicit ``POST /sensors/{id}/trigger`` endpoint to force the
    sensor to re-observe.
    """
    _, data_file = sensor_server_project

    resp = niquests.post(f"{sensor_server_url}/api/run/pass")
    assert resp.status_code == 200

    sensors = niquests.get(f"{sensor_server_url}/api/sensors").json()
    sensor_id = sensors[0]["asset_id"]
    observations = niquests.get(f"{sensor_server_url}/api/sensors/{sensor_id}/observations").json()
    assert len(observations) >= 1
    assert observations[0]["update_detected"] is True
    initial_obs_count = len(observations)

    assets = niquests.get(f"{sensor_server_url}/api/assets").json()
    transform = next(a for a in assets if a["function_name"] == "transform")
    detail = niquests.get(f"{sensor_server_url}/api/assets/{transform['asset_id']}").json()
    assert detail["latest_materialization"] is not None
    assert detail["latest_materialization"]["status"] == "success"

    # Update the data file and force a sensor re-observation via the
    # explicit trigger endpoint (bypasses cron eligibility)
    time.sleep(1)
    data_file.write_text("updated content")

    resp = niquests.post(f"{sensor_server_url}/api/sensors/{sensor_id}/trigger")
    assert resp.status_code == 200

    observations2 = niquests.get(f"{sensor_server_url}/api/sensors/{sensor_id}/observations").json()
    assert len(observations2) > initial_obs_count

    latest_obs = observations2[0]
    assert latest_obs["update_detected"] is True
    assert "updated content" in (latest_obs["output_json"] or "")

    # Now a subsequent run_pass should see the sensor's new observation and
    # re-materialize the downstream
    resp = niquests.post(f"{sensor_server_url}/api/run/pass")
    assert resp.status_code == 200
