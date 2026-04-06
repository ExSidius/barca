"""Tests for sensor decorator, discovery, and reconciliation."""

import sys
import textwrap

import pytest

from barca._engine import refresh, reindex, trigger_sensor
from barca._reconciler import reconcile
from barca._store import MetadataStore


def _cleanup(prefix):
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]


@pytest.fixture
def sensor_project(tmp_path):
    project_dir = tmp_path / "sensorproject"
    project_dir.mkdir()

    mod_dir = project_dir / "smod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import sensor, asset, effect

        @sensor(schedule="always")
        def check_file():
            return (True, {"path": "/tmp/data.csv", "rows": 42})

        @asset(inputs={"data": check_file}, schedule="always")
        def process(data):
            return {"processed": data["rows"] * 2}

        @effect(inputs={"result": process}, schedule="always")
        def notify(result):
            pass  # would send notification
    """)
    )

    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["smod.pipeline"]
    """)
    )

    _cleanup("smod")
    sys.path.insert(0, str(project_dir))
    yield project_dir
    sys.path.remove(str(project_dir))
    _cleanup("smod")
    from barca._trace import clear_caches

    clear_caches()


def test_sensor_decorator_sets_kind():
    from barca import sensor

    @sensor
    def my_sensor():
        return (True, "data")

    assert my_sensor.__barca_kind__ == "sensor"
    assert my_sensor.__barca_metadata__["kind"] == "sensor"


def test_sensors_discovered_by_inspector(sensor_project):
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    assets = reindex(store, sensor_project)

    kinds = {a.function_name: a.kind for a in assets}
    assert kinds["check_file"] == "sensor"
    assert kinds["process"] == "asset"
    assert kinds["notify"] == "effect"


def test_sensor_indexed_with_kind(sensor_project):
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    reindex(store, sensor_project)

    assets = store.list_assets()
    sensor_asset = next(a for a in assets if a.function_name == "check_file")
    assert sensor_asset.kind == "sensor"


def test_sensor_observation_stored_after_reconcile(sensor_project):
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    result = reconcile(store, sensor_project)

    assert result.executed_sensors >= 1

    assets = store.list_assets()
    sensor_asset = next(a for a in assets if a.function_name == "check_file")
    obs = store.latest_sensor_observation(sensor_asset.asset_id)
    assert obs is not None
    assert obs.update_detected is True


def test_sensor_false_does_not_trigger_downstream(tmp_path):
    """A sensor returning (False, None) should not trigger downstream."""
    project_dir = tmp_path / "falseproject"
    project_dir.mkdir()

    mod_dir = project_dir / "fmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import sensor, asset

        @sensor(schedule="always")
        def idle_sensor():
            return (False, None)

        @asset(inputs={"data": idle_sensor}, schedule="always")
        def downstream(data):
            return {"got": data}
    """)
    )

    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["fmod.pipeline"]
    """)
    )

    _cleanup("fmod")
    sys.path.insert(0, str(project_dir))
    try:
        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = reconcile(store, project_dir)

        assert result.executed_sensors >= 1
        # Downstream should NOT have been executed because sensor returned False
        assert result.executed_assets == 0
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("fmod")
        from barca._trace import clear_caches

        clear_caches()


def test_list_sensor_observations(sensor_project):
    """list_sensor_observations returns observations in reverse chronological order."""
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    # Run reconcile twice to create multiple observations
    reconcile(store, sensor_project)
    reconcile(store, sensor_project)

    assets = store.list_assets()
    sensor_asset = next(a for a in assets if a.function_name == "check_file")
    observations = store.list_sensor_observations(sensor_asset.asset_id)

    assert len(observations) >= 2
    # Verify reverse chronological order
    for i in range(len(observations) - 1):
        assert observations[i].created_at >= observations[i + 1].created_at

    # Verify limit works
    limited = store.list_sensor_observations(sensor_asset.asset_id, limit=1)
    assert len(limited) == 1


def test_asset_detail_includes_latest_observation(sensor_project):
    """asset_detail populates latest_observation for sensors, not for assets."""
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    reconcile(store, sensor_project)

    assets = store.list_assets()
    sensor_asset = next(a for a in assets if a.function_name == "check_file")
    process_asset = next(a for a in assets if a.function_name == "process")

    sensor_detail = store.asset_detail(sensor_asset.asset_id)
    assert sensor_detail.latest_observation is not None
    assert sensor_detail.latest_observation.update_detected is True

    asset_detail = store.asset_detail(process_asset.asset_id)
    assert asset_detail.latest_observation is None


def test_trigger_sensor_returns_observation(sensor_project):
    """trigger_sensor executes the sensor and returns a valid observation."""
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    reindex(store, sensor_project)

    assets = store.list_assets()
    sensor_asset = next(a for a in assets if a.function_name == "check_file")

    obs = trigger_sensor(store, sensor_project, sensor_asset.asset_id)
    assert obs is not None
    assert obs.update_detected is True
    assert obs.output_json is not None
    assert "rows" in obs.output_json


def test_trigger_sensor_rejects_non_sensor(sensor_project):
    """trigger_sensor raises ValueError when called on a regular asset."""
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    reindex(store, sensor_project)

    assets = store.list_assets()
    process_asset = next(a for a in assets if a.function_name == "process")

    with pytest.raises(ValueError, match="not a sensor"):
        trigger_sensor(store, sensor_project, process_asset.asset_id)


def test_refresh_rejects_sensor(sensor_project):
    """refresh raises ValueError when called on a sensor."""
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    reindex(store, sensor_project)

    assets = store.list_assets()
    sensor_asset = next(a for a in assets if a.function_name == "check_file")

    with pytest.raises(ValueError, match="sensor"):
        refresh(store, sensor_project, sensor_asset.asset_id)
