"""Tests for the ManualBlocksDownstream invariant.

Design decision D3: opportunistic — downstream Always assets are blocked
from auto-running only while the manual upstream is stale. Once the manual
upstream is explicitly refreshed (and fresh), downstream runs normally on
the next run_pass.
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


def test_manual_upstream_blocks_always_downstream_when_stale(tmp_path):
    """Chain A(Manual) → B(Always). First run_pass: A skipped, B blocked."""
    project_dir = tmp_path / "blockproj"
    project_dir.mkdir()

    mod_dir = project_dir / "bmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Manual, Always

        @asset(freshness=Manual())
        def a():
            return {"a": 1}

        @asset(inputs={"a_val": a}, freshness=Always())
        def b(a_val):
            return {"b": a_val["a"] + 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["bmod.pipeline"]
        """)
    )

    _cleanup("bmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)

        # A is manual → not executed
        # B is always, but its upstream A is stale-manual → blocked
        assert result.executed_assets == 0
        assert result.manual_skipped == 1  # A
        assert result.stale_blocked == 1  # B

        # Neither A nor B should have a successful materialisation
        a_id = store.asset_id_by_logical_name("bmod/pipeline.py:a")
        b_id = store.asset_id_by_logical_name("bmod/pipeline.py:b")
        assert store.latest_successful_materialization(a_id) is None
        assert store.latest_successful_materialization(b_id) is None
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("bmod")


def test_manual_upstream_unblocks_when_refreshed(tmp_path):
    """After explicit refresh of A, the next run_pass executes B."""
    project_dir = tmp_path / "unblockproj"
    project_dir.mkdir()

    mod_dir = project_dir / "umod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Manual, Always

        @asset(freshness=Manual())
        def a():
            return {"a": 10}

        @asset(inputs={"a_val": a}, freshness=Always())
        def b(a_val):
            return {"b": a_val["a"] * 2}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["umod.pipeline"]
        """)
    )

    _cleanup("umod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))

        # Pass 1: A blocked (manual), B blocked (upstream stale)
        r1 = run_pass(store, project_dir)
        assert r1.executed_assets == 0

        # Explicitly refresh A
        a_id = store.asset_id_by_logical_name("umod/pipeline.py:a")
        refresh(store, project_dir, a_id)

        # Pass 2: A is now fresh, B's block clears, B executes
        r2 = run_pass(store, project_dir)
        assert r2.executed_assets == 1  # B
        b_id = store.asset_id_by_logical_name("umod/pipeline.py:b")
        assert store.latest_successful_materialization(b_id) is not None
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("umod")


def test_deep_manual_block(tmp_path):
    """Chain A(Manual) → B(Always) → C(Always). Both B and C are blocked."""
    project_dir = tmp_path / "deepproj"
    project_dir.mkdir()

    mod_dir = project_dir / "dmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Manual, Always

        @asset(freshness=Manual())
        def a():
            return {"a": 1}

        @asset(inputs={"a_val": a}, freshness=Always())
        def b(a_val):
            return {"b": 2}

        @asset(inputs={"b_val": b}, freshness=Always())
        def c(b_val):
            return {"c": 3}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["dmod.pipeline"]
        """)
    )

    _cleanup("dmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)

        assert result.executed_assets == 0
        assert result.manual_skipped == 1  # A
        assert result.stale_blocked == 2  # B and C
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("dmod")


def test_explicit_refresh_bypasses_block(tmp_path):
    """Explicitly refreshing B with stale_policy=pass works even when A is stale-manual."""
    project_dir = tmp_path / "bypassproj"
    project_dir.mkdir()

    mod_dir = project_dir / "bpmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Manual, Always

        @asset(freshness=Manual())
        def a():
            return {"a": 1}

        @asset(inputs={"a_val": a}, freshness=Always())
        def b(a_val):
            return {"b": 2}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["bpmod.pipeline"]
        """)
    )

    _cleanup("bpmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh, reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)

        # First we need to materialise A at least once so B has inputs available.
        # Since A is manual, materialise it explicitly:
        a_id = store.asset_id_by_logical_name("bpmod/pipeline.py:a")
        refresh(store, project_dir, a_id)

        # Now edit A's source to make it stale (simulate code change)
        (mod_dir / "pipeline.py").write_text(
            textwrap.dedent("""\
            from barca import asset, Manual, Always

            @asset(freshness=Manual())
            def a():
                return {"a": 999}  # changed

            @asset(inputs={"a_val": a}, freshness=Always())
            def b(a_val):
                return {"b": 2}
            """)
        )
        _cleanup("bpmod")
        sys.path.insert(0, str(project_dir))
        reindex(store, project_dir)

        # Explicit refresh of B with stale_policy=pass should proceed
        b_id = store.asset_id_by_logical_name("bpmod/pipeline.py:b")
        refresh(store, project_dir, b_id, stale_policy="pass")

        mat = store.latest_successful_materialization(b_id)
        assert mat is not None
        assert mat.stale_inputs_used is True
    finally:
        if str(project_dir) in sys.path:
            sys.path.remove(str(project_dir))
        _cleanup("bpmod")
