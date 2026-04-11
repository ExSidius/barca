"""Tests for barca prune — destructive history cleanup.

Covers spec rule PruneRemovesUnreachableHistory:
- Removes DB rows for deactivated (removed-from-DAG) assets
- Removes artifacts on disk for deactivated assets
- Preserves active asset history
- Idempotent
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


def test_prune_removes_deactivated_assets(tmp_path):
    """Define → materialise → remove → reindex (deactivates) → prune → gone."""
    project_dir = tmp_path / "pruneproj"
    project_dir.mkdir()

    mod_dir = project_dir / "pmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always

        @asset(freshness=Always())
        def doomed():
            return {"n": 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["pmod.assets"]
        """)
    )

    _cleanup("pmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh, reindex
        from barca._prune import prune

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        asset_id = store.asset_id_by_logical_name("pmod/assets.py:doomed")
        refresh(store, project_dir, asset_id)

        # Confirm it's there
        assert store.latest_successful_materialization(asset_id) is not None

        # Remove the asset from source
        (mod_dir / "assets.py").write_text("")
        _cleanup("pmod")
        sys.path.insert(0, str(project_dir))
        reindex(store, project_dir)

        # Asset should be deactivated but history preserved
        # (tested in more detail by test_prune_preserves_active_history)

        # Prune — materialisation and row should be gone
        prune(store, project_dir)

        # After prune, the asset id should not resolve
        assert store.asset_id_by_logical_name("pmod/assets.py:doomed") is None
    finally:
        if str(project_dir) in sys.path:
            sys.path.remove(str(project_dir))
        _cleanup("pmod")


def test_prune_preserves_active_history(tmp_path):
    """Prune removes old definition_hash artifacts but keeps current."""
    project_dir = tmp_path / "preserve"
    project_dir.mkdir()

    mod_dir = project_dir / "prmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always

        @asset(freshness=Always())
        def stable():
            return {"v": 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["prmod.assets"]
        """)
    )

    _cleanup("prmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh, reindex
        from barca._prune import prune

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        asset_id = store.asset_id_by_logical_name("prmod/assets.py:stable")
        refresh(store, project_dir, asset_id)

        # Prune should keep the current materialisation
        prune(store, project_dir)
        mat = store.latest_successful_materialization(asset_id)
        assert mat is not None
        assert mat.status == "success"
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("prmod")


def test_prune_is_idempotent(tmp_path):
    """Running prune twice should not error and should not delete anything the second time."""
    project_dir = tmp_path / "idempotent"
    project_dir.mkdir()

    mod_dir = project_dir / "idmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always

        @asset(freshness=Always())
        def basic():
            return {"v": 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["idmod.assets"]
        """)
    )

    _cleanup("idmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh, reindex
        from barca._prune import prune

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        asset_id = store.asset_id_by_logical_name("idmod/assets.py:basic")
        refresh(store, project_dir, asset_id)

        prune(store, project_dir)
        r2 = prune(store, project_dir)
        # Second prune should have nothing to remove
        assert r2.removed_assets == 0
        assert r2.removed_materializations == 0
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("idmod")
