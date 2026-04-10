"""Tests for effect decorator and behavior."""

import sys
import textwrap

import pytest

from barca._reconciler import reconcile
from barca._store import MetadataStore


def _cleanup(prefix):
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]


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

    with pytest.raises(TypeError, match="ref strings"):

        @asset(inputs={"data": my_effect})
        def bad_asset(data):
            return data


def test_effect_executes_after_upstream(tmp_path):
    project_dir = tmp_path / "effectproject"
    project_dir.mkdir()

    mod_dir = project_dir / "emod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset, effect

        @asset(schedule="always")
        def data_source():
            return {"value": 42}

        @effect(inputs={"data": data_source}, schedule="always")
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
        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = reconcile(store, project_dir)

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
        from barca._trace import clear_caches

        clear_caches()
