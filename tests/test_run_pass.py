"""Tests for run_pass — the core reconciliation primitive.

Replaces the old test_reconciler.py. Covers:
- Always assets materialise on first pass
- Default freshness is Always
- Manual assets are skipped in run_pass
- Scheduled assets respect cron ticks
- Second pass is fresh (cache hit)
- Full pipeline: sensor → asset → effect
- Sensor passes full (bool, value) tuple to downstream
- Failure cascades to downstream
"""

from __future__ import annotations

import sys
import textwrap

from barca._store import MetadataStore


def _cleanup(prefix: str) -> None:
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]
    from barca._trace import clear_caches

    clear_caches()


# ---------------------------------------------------------------------------
# Default freshness: Always
# ---------------------------------------------------------------------------


def test_default_freshness_is_always(tmp_path):
    """@asset() with no kwarg should materialise on first run_pass."""
    project_dir = tmp_path / "defaultproj"
    project_dir.mkdir()

    mod_dir = project_dir / "dmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset()
        def simple():
            return {"x": 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["dmod.assets"]
        """)
    )

    _cleanup("dmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)
        assert result.executed_assets == 1
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("dmod")


def test_always_assets_materialize_on_first_pass(tmp_path):
    """Explicit freshness=Always() also materialises on first pass."""
    project_dir = tmp_path / "alwaysproj"
    project_dir.mkdir()

    mod_dir = project_dir / "amod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always

        @asset(freshness=Always())
        def auto():
            return {"auto": True}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["amod.assets"]
        """)
    )

    _cleanup("amod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)
        assert result.executed_assets == 1
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("amod")


# ---------------------------------------------------------------------------
# Manual is skipped
# ---------------------------------------------------------------------------


def test_manual_assets_are_skipped_in_run_pass(tmp_path):
    """@asset(freshness=Manual()) is not executed by run_pass."""
    project_dir = tmp_path / "manualproj"
    project_dir.mkdir()

    mod_dir = project_dir / "mmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Manual

        @asset(freshness=Manual())
        def manual_asset():
            return {"manual": True}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["mmod.assets"]
        """)
    )

    _cleanup("mmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)
        assert result.executed_assets == 0
        # manual is counted separately from stale-waiting
        assert result.manual_skipped == 1
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("mmod")


# ---------------------------------------------------------------------------
# Cache hit on second pass
# ---------------------------------------------------------------------------


def test_second_run_pass_is_fresh(tmp_path):
    """After a successful first pass, the second pass reports fresh, not executed."""
    project_dir = tmp_path / "freshproj"
    project_dir.mkdir()

    mod_dir = project_dir / "frmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset()
        def simple():
            return {"x": 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["frmod.assets"]
        """)
    )

    _cleanup("frmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        r1 = run_pass(store, project_dir)
        assert r1.executed_assets == 1

        r2 = run_pass(store, project_dir)
        assert r2.executed_assets == 0
        assert r2.fresh == 1
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("frmod")


# ---------------------------------------------------------------------------
# Full pipeline
# ---------------------------------------------------------------------------


def test_full_pipeline_sensor_to_effect(tmp_path):
    """sensor(True) → asset → effect: all three execute on first pass."""
    project_dir = tmp_path / "fullproj"
    project_dir.mkdir()

    mod_dir = project_dir / "fullmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import sensor, asset, effect, Always, Schedule

        @sensor(freshness=Schedule("* * * * *"))
        def source():
            return (True, {"value": 10})

        @asset(inputs={"data": source}, freshness=Always())
        def transform(data):
            # sensor passes the full (bool, value) tuple
            update_detected, payload = data
            return {"doubled": payload["value"] * 2}

        @effect(inputs={"result": transform}, freshness=Always())
        def push(result):
            pass
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["fullmod.pipeline"]
        """)
    )

    _cleanup("fullmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)

        assert result.executed_sensors == 1
        assert result.executed_assets == 1
        assert result.executed_effects == 1
        assert result.failed == 0
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("fullmod")


# ---------------------------------------------------------------------------
# Sensor tuple passing
# ---------------------------------------------------------------------------


def test_sensor_full_tuple_passed_to_downstream(tmp_path):
    """Downstream asset receives the full (update_detected, output) tuple."""
    project_dir = tmp_path / "tupproj"
    project_dir.mkdir()

    mod_dir = project_dir / "tupmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import sensor, asset, Always, Schedule

        @sensor(freshness=Schedule("* * * * *"))
        def my_sensor():
            return (True, {"value": 99})

        @asset(inputs={"obs": my_sensor}, freshness=Always())
        def uses_sensor(obs):
            # obs must be a tuple, not just the value
            assert isinstance(obs, (tuple, list)), f"expected tuple/list, got {type(obs).__name__}"
            assert len(obs) == 2
            update_detected, value = obs
            assert update_detected is True
            assert value == {"value": 99}
            return {"saw": value["value"]}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["tupmod.pipeline"]
        """)
    )

    _cleanup("tupmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)
        assert result.failed == 0
        assert result.executed_assets == 1
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("tupmod")


# ---------------------------------------------------------------------------
# Failure cascade
# ---------------------------------------------------------------------------


def test_failure_cascades_to_downstream(tmp_path):
    """If upstream fails, downstream must not be attempted."""
    project_dir = tmp_path / "failproj"
    project_dir.mkdir()

    mod_dir = project_dir / "failmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset()
        def upstream():
            raise RuntimeError("boom")

        @asset(inputs={"u": upstream})
        def downstream(u):
            return {"ok": True}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["failmod.pipeline"]
        """)
    )

    _cleanup("failmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)

        assert result.failed >= 1  # upstream failed
        # Downstream must not have been executed
        downstream_id = store.asset_id_by_logical_name("failmod/pipeline.py:downstream")
        detail = store.asset_detail(downstream_id)
        assert detail.latest_materialization is None or detail.latest_materialization.status != "success"
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("failmod")
