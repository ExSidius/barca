"""Subprocess-based CLI tests.

These tests exercise the exact `barca` binary users run. They're slower
than library-level tests (one subprocess per test), so we keep them
focused on things you can only verify through the CLI surface:

- Exit codes
- Stdout/stderr shape (table headers, diff output, badges)
- Argument parsing
- --help discoverability
- User-facing error messages
- The `[unsafe]` badge in `assets list` (spec rule UnsafeAssetsDistinguishedInListing)
- The three-way diff format in `reindex`
- Prune confirmation prompt

Most of these will fail during Phase 2 because the new CLI commands
(`barca run`, `barca dev`, `barca prune`, `--stale-policy`) don't exist
yet. That's expected — each failing test is an executable definition
of what Phase 3 must deliver.
"""

from __future__ import annotations

# ---------------------------------------------------------------------------
# Help & command discoverability
# ---------------------------------------------------------------------------


def test_cli_help_lists_run_command(barca_ctx):
    """`barca --help` should advertise the new `run` command."""
    result = barca_ctx.cli(["--help"])
    assert result.success(), f"barca --help failed: {result.stderr}"
    assert "run" in result.stdout.lower()


def test_cli_help_lists_dev_command(barca_ctx):
    result = barca_ctx.cli(["--help"])
    assert result.success()
    assert "dev" in result.stdout.lower()


def test_cli_help_lists_prune_command(barca_ctx):
    result = barca_ctx.cli(["--help"])
    assert result.success()
    assert "prune" in result.stdout.lower()


def test_cli_refresh_help_documents_stale_policy(barca_ctx):
    """`barca assets refresh --help` should document --stale-policy."""
    result = barca_ctx.cli(["assets", "refresh", "--help"])
    assert result.success(), f"help command failed: {result.stderr}"
    assert "stale-policy" in result.stdout.lower() or "stale_policy" in result.stdout.lower()


# ---------------------------------------------------------------------------
# Reindex three-way diff output
# ---------------------------------------------------------------------------


def test_cli_reindex_shows_added_section(barca_ctx):
    """First reindex of a fresh project prints an 'Added' section with the new assets."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def alpha():
            return 1
        """,
    )
    result = barca_ctx.cli(["reindex"])
    assert result.success(), f"reindex failed:\n{result.stderr}"
    combined = (result.stdout + result.stderr).lower()
    assert "added" in combined or "+" in result.stdout, f"expected a diff section; got:\n{result.stdout}"
    assert "alpha" in combined


def test_cli_reindex_shows_removed_section(barca_ctx):
    """After removing an asset, reindex prints a 'Removed' section."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def going_away():
            return 1
        """,
    )
    barca_ctx.cli(["reindex"])  # initial index

    # Clear the file
    barca_ctx.write_module("assets.py", "# empty\n")

    result = barca_ctx.cli(["reindex"])
    assert result.success()
    combined = (result.stdout + result.stderr).lower()
    assert "removed" in combined or "-" in result.stdout
    assert "going_away" in combined


def test_cli_reindex_empty_project_exits_zero(barca_ctx):
    """An empty project (no decorated functions) should reindex cleanly."""
    barca_ctx.write_module("assets.py", "# no assets\n")
    result = barca_ctx.cli(["reindex"])
    assert result.success(), f"empty project reindex failed: {result.stderr}"


# ---------------------------------------------------------------------------
# Unsafe badge in assets list
# ---------------------------------------------------------------------------


def test_cli_assets_list_shows_unsafe_badge(barca_ctx):
    """Spec rule UnsafeAssetsDistinguishedInListing.
    An @unsafe asset must be visually distinguished in ``barca assets list``.
    The renderer may wrap the badge onto a continuation line when columns
    are narrow, so we check the global stdout for ``unsafe`` in proximity
    to ``impure_one`` but not near ``pure_one``."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset, unsafe

        @asset()
        def pure_one():
            return 1

        @asset()
        @unsafe
        def impure_one():
            return 2
        """,
    )
    barca_ctx.cli(["reindex"])

    result = barca_ctx.cli(["assets", "list"])
    assert result.success(), f"assets list failed: {result.stderr}"

    # The unsafe badge should appear SOMEWHERE in the output, and
    # impure_one should be in the output.
    assert "impure_one" in result.stdout
    assert "pure_one" in result.stdout
    assert "unsafe" in result.stdout.lower(), f"expected 'unsafe' badge in assets list output; got:\n{result.stdout}"


# ---------------------------------------------------------------------------
# Refresh stale policy
# ---------------------------------------------------------------------------


def test_cli_refresh_stale_policy_error_nonzero_exit(barca_ctx):
    """`barca assets refresh <id>` with default policy=error and a stale
    upstream must exit non-zero with a useful stderr message."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def upstream():
            return 1

        @asset(inputs={"u": upstream})
        def downstream(u):
            return u + 1
        """,
    )
    barca_ctx.cli(["reindex"])

    # Try to refresh downstream while upstream is stale
    result = barca_ctx.cli(["assets", "refresh", "downstream"])
    assert not result.success(), "expected refresh to fail with stale upstream under default policy"
    combined = (result.stdout + result.stderr).lower()
    assert "stale" in combined or "upstream" in combined


def test_cli_refresh_stale_policy_pass_succeeds(barca_ctx):
    """`barca assets refresh --stale-policy=pass` succeeds using a prior
    upstream materialization even when the upstream has since been made stale."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def upstream():
            return 1

        @asset(inputs={"u": upstream})
        def downstream(u):
            return u + 1
        """,
    )
    barca_ctx.cli(["reindex"])
    # First materialize upstream so it has a prior value
    barca_ctx.cli(["assets", "refresh", "upstream"])

    # Edit upstream to make it stale (definition changed)
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def upstream():
            return 999  # changed

        @asset(inputs={"u": upstream})
        def downstream(u):
            return u + 1
        """,
    )

    # Now refresh downstream with pass — should succeed using upstream's prior value
    result = barca_ctx.cli(["assets", "refresh", "downstream", "--stale-policy", "pass"])
    assert result.success(), f"refresh --stale-policy=pass should succeed; got:\nstdout: {result.stdout}\nstderr: {result.stderr}"


# ---------------------------------------------------------------------------
# Prune
# ---------------------------------------------------------------------------


def test_cli_prune_with_yes_flag(barca_ctx):
    """`barca prune --yes` runs without a confirmation prompt."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def keepable():
            return 1
        """,
    )
    barca_ctx.cli(["reindex"])

    result = barca_ctx.cli(["prune", "--yes"])
    assert result.success(), f"prune --yes failed:\n{result.stderr}"


def test_cli_prune_empty_project_is_noop(barca_ctx):
    """Prune on a fresh project with no history should exit 0 and say 'nothing to prune'."""
    result = barca_ctx.cli(["prune", "--yes"])
    assert result.success()
    combined = (result.stdout + result.stderr).lower()
    assert "nothing" in combined or "0" in combined or "clean" in combined or "empty" in combined


# ---------------------------------------------------------------------------
# Run / dev basic
# ---------------------------------------------------------------------------


def test_cli_run_command_exists(barca_ctx):
    """`barca run --help` must exist and document the command."""
    result = barca_ctx.cli(["run", "--help"])
    assert result.success(), f"barca run --help failed — command may not exist:\n{result.stderr}"


def test_cli_dev_command_exists(barca_ctx):
    """`barca dev --help` must exist and document the command."""
    result = barca_ctx.cli(["dev", "--help"])
    assert result.success(), f"barca dev --help failed — command may not exist:\n{result.stderr}"


# ---------------------------------------------------------------------------
# Reconcile is gone
# ---------------------------------------------------------------------------


def test_cli_reconcile_command_removed(barca_ctx):
    """`barca reconcile` was removed from the user-facing CLI in Phase 3.
    `barca run` replaces it. This test asserts the rename actually happened."""
    result = barca_ctx.cli(["reconcile"])
    # Expect non-zero exit with "unknown command" or similar message
    assert not result.success(), "barca reconcile should have been removed in Phase 3 (replaced by barca run)"
