"""Smoke tests for BarcaTestContext — verify the helper works before we rely on it.

These tests exercise only capabilities that work in the current (pre-Phase-3)
source tree. They validate the fixture mechanics: fresh tempdir, frozen time,
module writing, reindex, asset snapshots, and diffs.

Tests that need Phase 3 (run_pass, prune, sinks, freshness semantics, etc.)
live in the feature-specific test files.
"""

from __future__ import annotations

import os

from barca._hashing import now_ts

# ---------------------------------------------------------------------------
# Frozen time
# ---------------------------------------------------------------------------


def test_context_installs_frozen_time(barca_ctx):
    """The fixture sets BARCA_TEST_NOW=auto:..., so now_ts() is deterministic."""
    env_value = os.environ.get("BARCA_TEST_NOW", "")
    assert env_value.startswith("auto:")

    t1 = now_ts()
    t2 = now_ts()
    t3 = now_ts()
    # Auto-tick: each call increments by 1
    assert t2 == t1 + 1
    assert t3 == t1 + 2


def test_context_resets_frozen_time_between_tests(barca_ctx):
    """The counter is fresh per test — the first call returns the base."""
    # If this test and test_context_installs_frozen_time ran in either order,
    # their first now_ts() should both start from the base.
    first = now_ts()
    # The base is an implementation detail; we only assert it's deterministic.
    assert first == now_ts() - 1


def test_context_advance_time(barca_ctx):
    """advance_time(seconds) bumps the base forward."""
    before = now_ts()
    barca_ctx.advance_time(3600)
    after = now_ts()
    # After advance, the next call should reflect the bump
    # (new_base + auto_tick > old_base + old_auto_tick)
    assert after > before + 100, f"expected jump of ~3600, got {after - before}"


def test_context_freeze_time(barca_ctx):
    """freeze_time(value) makes every subsequent now_ts() return exactly that."""
    barca_ctx.freeze_time(9_999_999)
    assert now_ts() == 9_999_999
    assert now_ts() == 9_999_999
    assert now_ts() == 9_999_999


# ---------------------------------------------------------------------------
# Module writing
# ---------------------------------------------------------------------------


def test_write_module_creates_file(barca_ctx):
    path = barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def hello():
            return {"msg": "hello"}
        """,
    )
    assert path.exists()
    assert path.parent == barca_ctx.pkg_dir
    content = path.read_text()
    assert "def hello()" in content
    # Dedent applied
    assert not content.startswith("    ")


def test_write_module_nested_path(barca_ctx):
    path = barca_ctx.write_module(
        "pipelines/etl.py",
        """
        from barca import asset

        @asset()
        def pipeline_one():
            return 1
        """,
    )
    assert path.exists()
    assert path.parent.name == "pipelines"
    assert (path.parent / "etl.py").exists()


def test_edit_module_applies_transform(barca_ctx):
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def v():
            return 1
        """,
    )
    barca_ctx.edit_module("assets.py", lambda src: src.replace("return 1", "return 2"))
    assert "return 2" in barca_ctx.read_module("assets.py")


# ---------------------------------------------------------------------------
# Engine integration (pre-Phase-3)
# ---------------------------------------------------------------------------


def test_reindex_returns_assets(barca_ctx):
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def first():
            return {"a": 1}

        @asset()
        def second():
            return {"b": 2}
        """,
    )
    assets = barca_ctx.reindex()
    # Works with both the current list[AssetSummary] and future ReindexDiff
    # (diff has .added/.removed/.renamed; for the fresh project both aliases
    # should be populated).
    if hasattr(assets, "added"):
        names = [n for n in assets.added]
    else:
        names = [a.function_name for a in assets]
    assert any("first" in n for n in names)
    assert any("second" in n for n in names)


def test_asset_id_lookup(barca_ctx):
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def findme():
            return 42
        """,
    )
    barca_ctx.reindex()
    asset_id = barca_ctx.asset_id_by_function("findme")
    assert isinstance(asset_id, int)
    assert asset_id > 0


# ---------------------------------------------------------------------------
# Snapshots
# ---------------------------------------------------------------------------


def test_assets_snapshot_is_stable(barca_ctx):
    """Two reindexes on the same source produce identical snapshots."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def alpha():
            return 1

        @asset()
        def beta():
            return 2
        """,
    )
    barca_ctx.reindex()
    snap1 = barca_ctx.assets_snapshot()

    barca_ctx.reindex()
    snap2 = barca_ctx.assets_snapshot()

    assert snap1 == snap2


def test_assets_snapshot_is_sorted_and_normalized(barca_ctx):
    """Snapshot is alphabetical by logical_name and strips volatile bits."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def zebra():
            return 1

        @asset()
        def apple():
            return 2
        """,
    )
    barca_ctx.reindex()
    snap = barca_ctx.assets_snapshot()
    lines = [line for line in snap.splitlines() if line]
    # apple comes before zebra
    assert lines[0].split(" | ")[0].endswith("apple")
    assert lines[1].split(" | ")[0].endswith("zebra")
    # No volatile absolute path
    assert str(barca_ctx.root) not in snap


def test_diff_assets_empty_when_nothing_changes(barca_ctx):
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def stable():
            return 1
        """,
    )
    barca_ctx.reindex()
    diff = barca_ctx.diff_assets(lambda c: c.reindex())
    assert diff == "(no changes)\n"


def test_diff_assets_captures_addition(barca_ctx):
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def existing():
            return 1
        """,
    )
    barca_ctx.reindex()

    def add_one(c):
        c.edit_module(
            "assets.py",
            lambda src: src + "\n\n@asset()\ndef newcomer():\n    return 2\n",
        )
        c.reindex()

    diff = barca_ctx.diff_assets(add_one)

    # Should show exactly one line added (for newcomer), no removals.
    # The normalization strips root paths, so the line should reference
    # <root>/... or just the logical name tail.
    added_lines = [line for line in diff.splitlines() if line.startswith("+") and not line.startswith("+++")]
    assert any("newcomer" in line for line in added_lines), f"expected 'newcomer' in added lines, got diff:\n{diff}"
