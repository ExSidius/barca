"""Tests for effect decorator and behavior.

@effect is for standalone side-effect functions (email, DB, API calls).
@sink is the separate decorator for writing an asset's output to a file path.
Effects are leaf nodes — they cannot be inputs to other assets.
"""

from __future__ import annotations

import sys
import textwrap

import pytest

from barca._store import MetadataStore


def _cleanup(prefix):
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]
    from barca._trace import clear_caches

    clear_caches()


def test_effect_decorator_sets_kind():
    from barca import effect

    @effect
    def my_effect():
        pass

    assert my_effect.__barca_kind__ == "effect"
    assert my_effect.__barca_metadata__["kind"] == "effect"


def test_effect_cannot_be_used_as_input():
    from barca import asset, effect

    @effect
    def my_effect():
        pass

    with pytest.raises(TypeError, match="leaf nodes"):

        @asset(inputs={"data": my_effect})
        def bad_asset(data):
            return data


def test_effect_does_not_accept_path_argument():
    """@effect is NOT a sink. It takes no path= argument."""
    from barca import effect

    with pytest.raises(TypeError):

        @effect(path="./out.json")
        def bad():
            pass


def test_effect_executes_after_upstream(tmp_path):
    """@effect runs after upstream materialises successfully."""
    project_dir = tmp_path / "effectproject"
    project_dir.mkdir()

    mod_dir = project_dir / "emod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset, effect, Always

        @asset(freshness=Always())
        def data_source():
            return {"value": 42}

        @effect(inputs={"data": data_source}, freshness=Always())
        def push_data(data):
            pass  # would push somewhere
    """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["emod.pipeline"]
    """)
    )

    _cleanup("emod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)

        assert result.executed_assets >= 1
        assert result.executed_effects >= 1

        # Verify effect execution recorded
        assets = store.list_assets()
        effect_asset = next(a for a in assets if a.function_name == "push_data")
        exec_record = store.latest_effect_execution(effect_asset.asset_id)
        assert exec_record is not None
        assert exec_record.status == "success"
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("emod")


def test_inputless_always_effect_runs_once(tmp_path):
    """Design decision D2: an input-less Always effect runs once on first success."""
    project_dir = tmp_path / "heartbeat"
    project_dir.mkdir()

    mod_dir = project_dir / "hbmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import effect, Always

        @effect(freshness=Always())
        def heartbeat():
            pass
    """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["hbmod.pipeline"]
    """)
    )

    _cleanup("hbmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        r1 = run_pass(store, project_dir)
        assert r1.executed_effects == 1

        # Second pass: effect is already fresh (no input changed), skipped
        r2 = run_pass(store, project_dir)
        assert r2.executed_effects == 0
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("hbmod")
