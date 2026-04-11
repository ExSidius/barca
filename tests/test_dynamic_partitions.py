"""Tests for dynamic partitions resolved from an upstream asset's output.

Covers spec rule PartitionSetResolvedLazily and design decision D8.

Dynamic partitions work like this::

    @asset(freshness=Always())
    def date_range():
        return ["2024-01", "2024-02", "2024-03"]

    @asset(partitions={"date": date_range}, freshness=Always())
    def report(date: str):
        return {"date": date, "rows": 100}

The partition-defining asset (date_range) becomes an implicit input to the
partitioned asset. Partition values are NOT known at index time — they are
resolved at run time, after date_range has materialised. Until then, the
partitioned asset is in a "partitions_state=pending" state.
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


def _write_dynamic_project(tmp_path, mod_name):
    project_dir = tmp_path / mod_name
    project_dir.mkdir()
    pkg_dir = project_dir / mod_name
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")
    (pkg_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always

        @asset(freshness=Always())
        def date_range():
            return ["2024-01", "2024-02", "2024-03"]

        @asset(partitions={"date": date_range}, freshness=Always())
        def report(date: str):
            return {"date": date, "rows": 100}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent(f"""\
        [project]
        modules = ["{mod_name}.assets"]
        """)
    )
    return project_dir, pkg_dir


def test_partition_set_pending_before_upstream_materializes(tmp_path):
    """Before date_range has run, report shows partitions_state='pending'."""
    project_dir, _ = _write_dynamic_project(tmp_path, "pendmod")
    _cleanup("pendmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)

        report_summary = next(a for a in store.list_assets() if "report" in a.logical_name)
        assert report_summary.partitions_state == "pending"
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("pendmod")


def test_partition_set_resolved_at_run_time(tmp_path):
    """After date_range runs, run_pass materialises one report per partition."""
    project_dir, _ = _write_dynamic_project(tmp_path, "runmod")
    _cleanup("runmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)

        # date_range + 3 report partitions = 4 executed assets
        assert result.executed_assets >= 4
        assert result.failed == 0

        report_id = store.asset_id_by_logical_name("runmod/assets.py:report")
        mats = store.list_materializations(report_id, limit=100)
        # One successful materialisation per partition value
        success_mats = [m for m in mats if m.status == "success"]
        assert len(success_mats) >= 3
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("runmod")


def test_partition_upstream_is_implicit_input(tmp_path):
    """The partition-defining asset appears in asset_inputs with is_partition_source=True."""
    project_dir, _ = _write_dynamic_project(tmp_path, "implmod")
    _cleanup("implmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._engine import reindex

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        reindex(store, project_dir)

        report_id = store.asset_id_by_logical_name("implmod/assets.py:report")
        detail = store.asset_detail(report_id)
        inputs = store.get_asset_inputs(detail.asset.definition_id)

        # At least one input with is_partition_source=True
        partition_sources = [i for i in inputs if getattr(i, "is_partition_source", False)]
        assert len(partition_sources) == 1
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("implmod")


def test_partition_upstream_change_invalidates_downstream(tmp_path):
    """When date_range re-materialises with new values, partitioned asset sees new set."""
    project_dir = tmp_path / "changeproj"
    project_dir.mkdir()
    pkg_dir = project_dir / "chmod"
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")
    (pkg_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, Always

        @asset(freshness=Always())
        def date_range():
            return ["2024-01", "2024-02"]

        @asset(partitions={"date": date_range}, freshness=Always())
        def report(date: str):
            return {"date": date}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["chmod.assets"]
        """)
    )

    _cleanup("chmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        run_pass(store, project_dir)

        # Change date_range output
        (pkg_dir / "assets.py").write_text(
            textwrap.dedent("""\
            from barca import asset, Always

            @asset(freshness=Always())
            def date_range():
                return ["2024-01", "2024-02", "2024-03", "2024-04"]

            @asset(partitions={"date": date_range}, freshness=Always())
            def report(date: str):
                return {"date": date}
            """)
        )
        _cleanup("chmod")
        sys.path.insert(0, str(project_dir))

        run_pass(store, project_dir)
        # At least the two new partitions should have materialised
        report_id = store.asset_id_by_logical_name("chmod/assets.py:report")
        mats = store.list_materializations(report_id, limit=100)
        success_mats = [m for m in mats if m.status == "success"]
        assert len(success_mats) >= 4
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("chmod")
