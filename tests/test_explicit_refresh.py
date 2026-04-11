"""Tests for the ExplicitRefresh rule and --stale-policy options.

Design decision (spec): refresh() takes a stale_policy of error | warn | pass:
- error (default): raise StaleUpstreamError if any upstream is stale
- warn: proceed with stale inputs, mark stale_inputs_used=True, emit warning
- pass: proceed silently with stale inputs, mark stale_inputs_used=True

Refresh does NOT cascade downstream. It materialises the target asset (and
its upstreams only when they are already fresh). Effects and sinks cannot
be directly refreshed.
"""

from __future__ import annotations

import sys
import textwrap

import pytest

from barca._store import MetadataStore


def _cleanup(prefix: str) -> None:
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]
    from barca._trace import clear_caches

    clear_caches()


def _write_chain(project_dir, mod_name):
    mod_dir = project_dir / mod_name
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always

        @asset(freshness=Always())
        def up():
            return {"v": 1}

        @asset(inputs={"u": up}, freshness=Always())
        def down(u):
            return {"v2": u["v"] + 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent(f"""\
        [project]
        modules = ["{mod_name}.pipeline"]
        """)
    )


def test_refresh_error_policy_raises_on_stale_upstream(tmp_path):
    """Default stale_policy=error must raise when upstream is stale."""
    project_dir = tmp_path / "err"
    project_dir.mkdir()
    _write_chain(project_dir, "errmod")

    _cleanup("errmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import StaleUpstreamError, refresh, reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        down_id = store.asset_id_by_logical_name("errmod/pipeline.py:down")

        # Up has never been materialised → stale
        with pytest.raises(StaleUpstreamError):
            refresh(store, project_dir, down_id)  # default stale_policy="error"
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("errmod")


def test_refresh_warn_policy_proceeds_and_marks_stale(tmp_path, caplog):
    """stale_policy=warn proceeds, marks mat, and emits a warning.

    Setup: materialise up, then edit up's source so it's stale, then
    refresh down with stale_policy=warn. Down should succeed using the
    previously-cached up value, and its mat should have stale_inputs_used=True.
    """
    project_dir = tmp_path / "warn"
    project_dir.mkdir()
    _write_chain(project_dir, "warnmod")

    _cleanup("warnmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh, reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        up_id = store.asset_id_by_logical_name("warnmod/pipeline.py:up")
        refresh(store, project_dir, up_id)

        # Now make up stale by editing its source
        (project_dir / "warnmod" / "pipeline.py").write_text(
            textwrap.dedent("""\
            from barca import asset, Always

            @asset(freshness=Always())
            def up():
                return {"v": 999}  # changed

            @asset(inputs={"u": up}, freshness=Always())
            def down(u):
                return {"v2": u["v"] + 1}
            """)
        )
        _cleanup("warnmod")
        sys.path.insert(0, str(project_dir))
        reindex(store, project_dir)

        down_id = store.asset_id_by_logical_name("warnmod/pipeline.py:down")

        import logging

        with caplog.at_level(logging.WARNING):
            detail = refresh(store, project_dir, down_id, stale_policy="warn")

        assert detail.latest_materialization is not None
        assert detail.latest_materialization.stale_inputs_used is True
        assert any("stale" in rec.message.lower() for rec in caplog.records)
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("warnmod")


def test_refresh_pass_policy_proceeds_silently(tmp_path, caplog):
    """stale_policy=pass proceeds, marks mat, no warning.

    Same setup as warn test — materialise upstream, then make it stale,
    then refresh downstream with stale_policy=pass.
    """
    project_dir = tmp_path / "pass"
    project_dir.mkdir()
    _write_chain(project_dir, "passmod")

    _cleanup("passmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh, reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        up_id = store.asset_id_by_logical_name("passmod/pipeline.py:up")
        refresh(store, project_dir, up_id)

        # Make up stale
        (project_dir / "passmod" / "pipeline.py").write_text(
            textwrap.dedent("""\
            from barca import asset, Always

            @asset(freshness=Always())
            def up():
                return {"v": 999}

            @asset(inputs={"u": up}, freshness=Always())
            def down(u):
                return {"v2": u["v"] + 1}
            """)
        )
        _cleanup("passmod")
        sys.path.insert(0, str(project_dir))
        reindex(store, project_dir)

        down_id = store.asset_id_by_logical_name("passmod/pipeline.py:down")

        import logging

        with caplog.at_level(logging.WARNING):
            detail = refresh(store, project_dir, down_id, stale_policy="pass")

        assert detail.latest_materialization is not None
        assert detail.latest_materialization.stale_inputs_used is True
        # No warning about stale upstream
        assert not any("stale" in rec.message.lower() for rec in caplog.records)
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("passmod")


def test_refresh_does_not_cascade_downstream(tmp_path):
    """Refreshing upstream does not automatically refresh downstream."""
    project_dir = tmp_path / "nocasc"
    project_dir.mkdir()
    _write_chain(project_dir, "nocascmod")

    _cleanup("nocascmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh, reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        up_id = store.asset_id_by_logical_name("nocascmod/pipeline.py:up")
        down_id = store.asset_id_by_logical_name("nocascmod/pipeline.py:down")

        refresh(store, project_dir, up_id)

        # Up is now fresh
        assert store.latest_successful_materialization(up_id) is not None
        # Down is NOT automatically refreshed
        assert store.latest_successful_materialization(down_id) is None
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("nocascmod")


def test_refresh_when_upstream_is_fresh_proceeds(tmp_path):
    """Refreshing downstream when upstream is already fresh proceeds normally."""
    project_dir = tmp_path / "upfresh"
    project_dir.mkdir()
    _write_chain(project_dir, "upfreshmod")

    _cleanup("upfreshmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh, reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        up_id = store.asset_id_by_logical_name("upfreshmod/pipeline.py:up")
        down_id = store.asset_id_by_logical_name("upfreshmod/pipeline.py:down")

        refresh(store, project_dir, up_id)
        detail = refresh(store, project_dir, down_id)  # default "error" policy

        assert detail.latest_materialization is not None
        assert detail.latest_materialization.status == "success"
        assert detail.latest_materialization.stale_inputs_used is False
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("upfreshmod")


def test_refresh_rejects_effect(tmp_path):
    """Effects cannot be directly refreshed — they flow from their upstream."""
    project_dir = tmp_path / "noeff"
    project_dir.mkdir()

    mod_dir = project_dir / "noeffmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset, effect, Always

        @asset(freshness=Always())
        def src():
            return {"v": 1}

        @effect(inputs={"s": src}, freshness=Always())
        def sink_fn(s):
            pass
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["noeffmod.pipeline"]
        """)
    )

    _cleanup("noeffmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh, reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        effect_id = store.asset_id_by_logical_name("noeffmod/pipeline.py:sink_fn")
        with pytest.raises((ValueError, TypeError)):
            refresh(store, project_dir, effect_id)
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("noeffmod")
