"""Tests for sensor decorator, discovery, and reconciliation."""

import sys
import textwrap
from pathlib import Path

import pytest

from barca._engine import reindex
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
    (mod_dir / "pipeline.py").write_text(textwrap.dedent("""\
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
    """))

    (project_dir / "barca.toml").write_text(textwrap.dedent("""\
        [project]
        modules = ["smod.pipeline"]
    """))

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
    sensor_asset = [a for a in assets if a.function_name == "check_file"][0]
    assert sensor_asset.kind == "sensor"


def test_sensor_observation_stored_after_reconcile(sensor_project):
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    result = reconcile(store, sensor_project)

    assert result.executed_sensors >= 1

    assets = store.list_assets()
    sensor_asset = [a for a in assets if a.function_name == "check_file"][0]
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
    (mod_dir / "pipeline.py").write_text(textwrap.dedent("""\
        from barca import sensor, asset

        @sensor(schedule="always")
        def idle_sensor():
            return (False, None)

        @asset(inputs={"data": idle_sensor}, schedule="always")
        def downstream(data):
            return {"got": data}
    """))

    (project_dir / "barca.toml").write_text(textwrap.dedent("""\
        [project]
        modules = ["fmod.pipeline"]
    """))

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
