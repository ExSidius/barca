"""Tests for reindex three-way diff: added / removed / renamed.

Covers spec rule ReindexShowsDiff and design decision D6 (in-place rename).

Rename detection:
- Primary signal: AST match (same function body, different location)
- Secondary signal: same explicit name= at a different location
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


def _make_project(tmp_path, mod_name, src):
    project_dir = tmp_path / mod_name
    project_dir.mkdir()
    pkg_dir = project_dir / mod_name
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")
    (pkg_dir / "assets.py").write_text(src)
    (project_dir / "barca.toml").write_text(
        textwrap.dedent(f"""\
        [project]
        modules = ["{mod_name}.assets"]
        """)
    )
    return project_dir, pkg_dir


# ---------------------------------------------------------------------------
# Added
# ---------------------------------------------------------------------------


def test_added_asset_appears_in_diff(tmp_path):
    """Fresh project: diff.added contains all assets, removed/renamed empty."""
    project_dir, _ = _make_project(
        tmp_path,
        "addmod",
        textwrap.dedent("""\
        from barca import asset

        @asset()
        def fresh_asset():
            return {"n": 1}
        """),
    )
    _cleanup("addmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        diff = reindex(store, project_dir)

        assert len(diff.added) == 1
        assert "fresh_asset" in diff.added[0]
        assert diff.removed == []
        assert diff.renamed == []
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("addmod")


# ---------------------------------------------------------------------------
# Removed
# ---------------------------------------------------------------------------


def test_removed_asset_appears_in_diff(tmp_path):
    """After removal, reindex shows it in diff.removed. Asset is deactivated, not deleted."""
    project_dir, pkg_dir = _make_project(
        tmp_path,
        "remmod",
        textwrap.dedent("""\
        from barca import asset

        @asset()
        def to_delete():
            return {"n": 1}
        """),
    )
    _cleanup("remmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)

        # Remove the asset from source
        (pkg_dir / "assets.py").write_text("")
        _cleanup("remmod")
        sys.path.insert(0, str(project_dir))

        diff = reindex(store, project_dir)
        assert diff.added == []
        assert len(diff.removed) == 1
        assert "to_delete" in diff.removed[0]
        assert diff.renamed == []
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("remmod")


# ---------------------------------------------------------------------------
# Renamed by AST match
# ---------------------------------------------------------------------------


def test_rename_detected_by_ast_match(tmp_path):
    """Moving a function body to a new file is detected as a rename."""
    project_dir = tmp_path / "astrenproj"
    project_dir.mkdir()
    pkg_dir = project_dir / "astmod"
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")
    (pkg_dir / "original.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset()
        def moving_target():
            return {"value": 42}
        """)
    )
    (pkg_dir / "new_location.py").write_text("")
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["astmod.original", "astmod.new_location"]
        """)
    )

    _cleanup("astmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        old_id = store.asset_id_by_logical_name("astmod/original.py:moving_target")
        assert old_id is not None

        # Move the function body to the new file
        (pkg_dir / "original.py").write_text("")
        (pkg_dir / "new_location.py").write_text(
            textwrap.dedent("""\
            from barca import asset

            @asset()
            def moving_target():
                return {"value": 42}
            """)
        )
        _cleanup("astmod")
        sys.path.insert(0, str(project_dir))

        diff = reindex(store, project_dir)
        assert diff.added == []
        assert diff.removed == []
        assert len(diff.renamed) == 1
        old_name, new_name = diff.renamed[0]
        assert "original.py" in old_name
        assert "new_location.py" in new_name

        # Asset id preserved
        new_id = store.asset_id_by_logical_name("astmod/new_location.py:moving_target")
        assert new_id == old_id
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("astmod")


# ---------------------------------------------------------------------------
# Renamed by name= kwarg
# ---------------------------------------------------------------------------


def test_name_kwarg_preserves_identity_across_edits(tmp_path):
    """Explicit name= makes the continuity_key stable, so editing the function
    body is an in-place definition update (not a rename at the diff level).
    The asset_id is preserved without needing the rename detection path."""
    project_dir = tmp_path / "namerenproj"
    project_dir.mkdir()
    pkg_dir = project_dir / "namemod"
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")
    (pkg_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset(name="stable_id")
        def old_name():
            return {"v": 1}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["namemod.assets"]
        """)
    )

    _cleanup("namemod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        old_id = store.asset_id_by_logical_name("stable_id")
        assert old_id is not None

        # Change function name and body; keep name=
        (pkg_dir / "assets.py").write_text(
            textwrap.dedent("""\
            from barca import asset

            @asset(name="stable_id")
            def new_function_name():
                return {"v": 999}  # body changed too
            """)
        )
        _cleanup("namemod")
        sys.path.insert(0, str(project_dir))

        diff = reindex(store, project_dir)
        # Id preserved via name= binding — continuity_key stable, no rename in diff
        new_id = store.asset_id_by_logical_name("stable_id")
        assert new_id == old_id
        # No added/removed/renamed — it's an in-place update
        assert diff.added == []
        assert diff.removed == []
        assert diff.renamed == []
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("namemod")


# ---------------------------------------------------------------------------
# History preservation across rename
# ---------------------------------------------------------------------------


def test_rename_preserves_materialization_history(tmp_path):
    """After a rename, the old materialisation is still reachable via the new id."""
    project_dir = tmp_path / "histproj"
    project_dir.mkdir()
    pkg_dir = project_dir / "histmod"
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")
    (pkg_dir / "a.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset()
        def tracked():
            return {"v": 1}
        """)
    )
    (pkg_dir / "b.py").write_text("")
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["histmod.a", "histmod.b"]
        """)
    )

    _cleanup("histmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import refresh, reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)
        old_id = store.asset_id_by_logical_name("histmod/a.py:tracked")
        refresh(store, project_dir, old_id)
        assert store.latest_successful_materialization(old_id) is not None

        # Rename: move to b.py
        (pkg_dir / "a.py").write_text("")
        (pkg_dir / "b.py").write_text(
            textwrap.dedent("""\
            from barca import asset

            @asset()
            def tracked():
                return {"v": 1}
            """)
        )
        _cleanup("histmod")
        sys.path.insert(0, str(project_dir))
        reindex(store, project_dir)

        new_id = store.asset_id_by_logical_name("histmod/b.py:tracked")
        assert new_id == old_id
        # History survives
        assert store.latest_successful_materialization(new_id) is not None
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("histmod")


def test_non_rename_appears_as_remove_plus_add(tmp_path):
    """Unrelated changes: remove + add, not a rename."""
    project_dir = tmp_path / "unrelated"
    project_dir.mkdir()
    pkg_dir = project_dir / "urmod"
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")
    (pkg_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset()
        def apple():
            return {"fruit": "apple"}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["urmod.assets"]
        """)
    )

    _cleanup("urmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)

        (pkg_dir / "assets.py").write_text(
            textwrap.dedent("""\
            from barca import asset

            @asset()
            def completely_different_thing():
                return {"other": "business", "unrelated": True}
            """)
        )
        _cleanup("urmod")
        sys.path.insert(0, str(project_dir))

        diff = reindex(store, project_dir)
        assert len(diff.removed) == 1
        assert len(diff.added) == 1
        assert diff.renamed == []
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("urmod")


def test_reindex_diff_on_no_changes_is_empty(tmp_path):
    """Second reindex on unchanged project: all three lists empty."""
    project_dir, _ = _make_project(
        tmp_path,
        "nochangemod",
        textwrap.dedent("""\
        from barca import asset

        @asset()
        def stable():
            return {"v": 1}
        """),
    )
    _cleanup("nochangemod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)  # first
        diff = reindex(store, project_dir)  # second
        assert diff.added == []
        assert diff.removed == []
        assert diff.renamed == []
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("nochangemod")
