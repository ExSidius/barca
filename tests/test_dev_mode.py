"""Tests for barca dev mode — file watcher that updates staleness without materializing.

Covers spec rule DevModeTracksstaleness.

Dev mode split into two pieces for testability:
- `dev_watch(store, repo_root)` — long-running file watcher (hard to unit test)
- `handle_file_change(store, repo_root)` — pure function (unit testable)

We test the pure function directly. File-watcher integration is a smoke test.
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


def test_handle_file_change_marks_assets_stale(tmp_path):
    """After handle_file_change, edited assets are marked stale in store.list_assets()."""
    project_dir = tmp_path / "devproj"
    project_dir.mkdir()

    mod_dir = project_dir / "devmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always

        @asset(freshness=Always())
        def watched():
            return {"v": 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["devmod.assets"]
        """)
    )

    _cleanup("devmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._dev import handle_file_change
        from barca._engine import refresh, reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        asset_id = store.asset_id_by_logical_name("devmod/assets.py:watched")
        refresh(store, project_dir, asset_id)

        # Capture initial definition_hash
        detail_before = store.asset_detail(asset_id)
        hash_before = detail_before.asset.definition_hash

        # Confirm a successful materialisation exists
        mat_before = store.latest_successful_materialization(asset_id)
        assert mat_before is not None

        # Edit source
        (mod_dir / "assets.py").write_text(
            textwrap.dedent("""\
            from barca import asset, Always

            @asset(freshness=Always())
            def watched():
                return {"v": 2}  # changed
            """)
        )
        _cleanup("devmod")
        sys.path.insert(0, str(project_dir))
        # Nuke pycache so Python sees the edited source
        import shutil as _shutil

        for pc in (mod_dir / "__pycache__",):
            if pc.exists():
                _shutil.rmtree(pc)

        # Handle the change (pure function)
        handle_file_change(store, project_dir)

        # Definition hash should have changed — asset is effectively stale
        detail_after = store.asset_detail(asset_id)
        assert detail_after.asset.definition_hash != hash_before, "expected definition_hash to change after source edit"

        # And NO new successful materialisation since the edit — dev mode
        # must not materialise. The pre-edit mat should still be the latest.
        mat_after = store.latest_successful_materialization(asset_id)
        assert mat_after is not None
        assert mat_after.materialization_id == mat_before.materialization_id
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("devmod")


def test_handle_file_change_does_not_materialize(tmp_path):
    """Dev mode must never create new materialisations."""
    project_dir = tmp_path / "devnomat"
    project_dir.mkdir()

    mod_dir = project_dir / "dnmmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always

        @asset(freshness=Always())
        def nomat():
            return {"v": 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["dnmmod.assets"]
        """)
    )

    _cleanup("dnmmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._dev import handle_file_change

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        handle_file_change(store, project_dir)

        asset_id = store.asset_id_by_logical_name("dnmmod/assets.py:nomat")
        assert asset_id is not None
        # No materialisation should exist — dev mode only indexes
        assert store.latest_successful_materialization(asset_id) is None
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("dnmmod")
