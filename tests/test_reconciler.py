"""Tests for the reconciler."""

import sys
import textwrap

from barca._reconciler import reconcile
from barca._store import MetadataStore


def _cleanup(prefix):
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]


def test_always_assets_get_materialized(tmp_path):
    project_dir = tmp_path / "alwaysproject"
    project_dir.mkdir()

    mod_dir = project_dir / "amod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset(schedule="always")
        def auto_asset():
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
        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = reconcile(store, project_dir)

        assert result.executed_assets == 1
        assert result.fresh == 0
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("amod")
        from barca._trace import clear_caches

        clear_caches()


def test_manual_assets_are_skipped(tmp_path):
    project_dir = tmp_path / "manualproject"
    project_dir.mkdir()

    mod_dir = project_dir / "mmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset(schedule="manual")
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
        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = reconcile(store, project_dir)

        assert result.executed_assets == 0
        # manual + stale = stale_waiting
        assert result.stale_waiting == 1
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("mmod")
        from barca._trace import clear_caches

        clear_caches()


def test_full_pipeline_sensor_to_effect(tmp_path):
    """Sensor(True) → Asset → Effect: all three should execute."""
    project_dir = tmp_path / "fullproject"
    project_dir.mkdir()

    mod_dir = project_dir / "fullmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import sensor, asset, effect

        @sensor(schedule="always")
        def source():
            return (True, {"value": 10})

        @asset(inputs={"data": source}, schedule="always")
        def transform(data):
            return {"doubled": data["value"] * 2}

        @effect(inputs={"result": transform}, schedule="always")
        def sink(result):
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
        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = reconcile(store, project_dir)

        assert result.executed_sensors == 1
        assert result.executed_assets == 1
        assert result.executed_effects == 1
        assert result.failed == 0
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("fullmod")
        from barca._trace import clear_caches

        clear_caches()


def test_second_reconcile_is_fresh(tmp_path):
    """After a full reconcile, a second pass should find everything fresh."""
    project_dir = tmp_path / "freshproject"
    project_dir.mkdir()

    mod_dir = project_dir / "frmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset(schedule="always")
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
        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        # First pass executes
        r1 = reconcile(store, project_dir)
        assert r1.executed_assets == 1

        # Second pass: same definition, same run_hash → cached
        r2 = reconcile(store, project_dir)
        assert r2.executed_assets == 0
        assert r2.fresh == 1
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("frmod")
        from barca._trace import clear_caches

        clear_caches()
