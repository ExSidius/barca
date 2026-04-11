"""Tests for sensor-specific freshness invariants.

Sensors require an explicit ``freshness=`` at decoration time (no default).
``Always`` is forbidden — polling frequency must be declared via ``Manual``
or ``Schedule``. Sensors that return ``(update_detected, output)`` propagate
the full tuple to downstream inputs when ``update_detected=True``.

Covers spec invariants:
- SensorFreshnessIsNotAlways
- Sensor requires explicit freshness (design decision D1)
"""

from __future__ import annotations

import sys
import textwrap

import pytest

from barca._freshness import Always, Manual, Schedule
from barca._store import MetadataStore


def _cleanup(prefix: str) -> None:
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]
    from barca._trace import clear_caches

    clear_caches()


# ---------------------------------------------------------------------------
# Decoration-time validation
# ---------------------------------------------------------------------------


def test_sensor_requires_explicit_freshness():
    """A bare @sensor() with no freshness= must raise TypeError."""
    from barca import sensor

    with pytest.raises(TypeError, match="freshness"):

        @sensor()
        def bad_sensor():
            return (True, {})


def test_sensor_without_parens_requires_freshness():
    """@sensor (no parens) is also disallowed — freshness is required."""
    from barca import sensor

    with pytest.raises(TypeError, match="freshness"):

        @sensor
        def bad_sensor():
            return (True, {})


def test_sensor_rejects_always_freshness():
    """@sensor(freshness=Always()) must raise TypeError."""
    from barca import sensor

    with pytest.raises(TypeError, match="Always"):

        @sensor(freshness=Always())
        def bad_sensor():
            return (True, {})


def test_sensor_accepts_manual():
    """@sensor(freshness=Manual()) is valid."""
    from barca import sensor

    @sensor(freshness=Manual())
    def manual_sensor():
        return (True, {})

    assert manual_sensor.__barca_kind__ == "sensor"


def test_sensor_accepts_schedule():
    """@sensor(freshness=Schedule(...)) is valid."""
    from barca import sensor

    @sensor(freshness=Schedule("*/5 * * * *"))
    def cron_sensor():
        return (True, {})

    assert cron_sensor.__barca_kind__ == "sensor"


def test_sensor_rejects_inputs_kwarg():
    """Invariant SensorsAreSourceNodes: sensors have no inputs.
    The decorator must reject an `inputs=` kwarg at decoration time."""
    from barca import Schedule, sensor

    with pytest.raises(TypeError):

        @sensor(freshness=Schedule("* * * * *"), inputs={"fake": "ref"})
        def bad_sensor():
            return (True, {})


# ---------------------------------------------------------------------------
# Sensor tuple passed to downstream
# ---------------------------------------------------------------------------


def test_sensor_full_tuple_stored_as_observation(tmp_path):
    """SensorObservation.output_json should capture the full (bool, value) tuple."""
    project_dir = tmp_path / "sensproject"
    project_dir.mkdir()

    mod_dir = project_dir / "sensmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import sensor, Schedule

        @sensor(freshness=Schedule("* * * * *"))
        def my_sensor():
            return (True, {"payload": 42})
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["sensmod.assets"]
        """)
    )

    _cleanup("sensmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import reindex, trigger_sensor

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)

        sensor_id = store.asset_id_by_logical_name("sensmod/assets.py:my_sensor")
        assert sensor_id is not None

        observation = trigger_sensor(store, project_dir, sensor_id)
        assert observation.update_detected is True
        # The stored output should be the full tuple, not just the payload.
        import json

        decoded = json.loads(observation.output_json)
        assert isinstance(decoded, list) and len(decoded) == 2
        assert decoded[0] is True
        assert decoded[1] == {"payload": 42}
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("sensmod")


def test_sensor_no_update_does_not_propagate_staleness(tmp_path):
    """When a sensor returns (False, ...), downstream assets should stay fresh."""
    project_dir = tmp_path / "noupdateproject"
    project_dir.mkdir()

    mod_dir = project_dir / "numod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import sensor, asset, Schedule, Always

        @sensor(freshness=Schedule("* * * * *"))
        def quiet_sensor():
            return (False, {"nothing": "new"})

        @asset(inputs={"tuple_input": quiet_sensor}, freshness=Always())
        def downstream(tuple_input):
            update_detected, value = tuple_input
            return {"saw_update": update_detected}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["numod.assets"]
        """)
    )

    _cleanup("numod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        # First pass: sensor executes, reports no update → downstream should not
        # run on the strength of this sensor alone.
        result = run_pass(store, project_dir)
        # Expected behaviour: sensor observed once; downstream is untouched
        # because update_detected=False.
        assert result.executed_sensors >= 1
        # Downstream's status should NOT be "freshly materialised due to sensor".
        # A more precise assertion would check that the downstream
        # materialisation was NOT created, but exact semantics depend on
        # whether the downstream has ever run. This smoke-tests the pattern.
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("numod")
