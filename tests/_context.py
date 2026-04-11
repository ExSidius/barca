"""BarcaTestContext — isolated test environment for Barca integration tests.

Inspired by uv's `TestContext` pattern. Every test gets:

* A fresh tempdir project with ``barca.toml`` and a single assets module
* A fresh ``MetadataStore`` scoped to that project
* Frozen time via ``BARCA_TEST_NOW`` so materialization timestamps and cron
  tick eligibility are deterministic
* Automatic cleanup of ``sys.modules`` and AST-trace caches between tests
* Ergonomic helpers for the common patterns in Barca tests:

    - ``ctx.write_module(rel, source)`` — write a Python module
    - ``ctx.edit_module(rel, transform)`` — edit-in-place via a string transform
    - ``ctx.reindex()`` — call the engine reindex and return the result
    - ``ctx.run_pass()`` — call ``run_pass`` and return ``RunPassResult``
    - ``ctx.refresh(asset_id, stale_policy=...)`` — explicit refresh
    - ``ctx.trigger_sensor(asset_id)`` — one-shot sensor trigger
    - ``ctx.cli(args)`` — subprocess-invoke the ``barca`` CLI
    - ``ctx.asset_id(logical_name)`` — quick lookup
    - ``ctx.assets_snapshot()`` — filtered, sorted text rep of the DAG
    - ``ctx.diff_assets(action)`` — unified diff of the DAG snapshot around an action
    - ``ctx.advance_time(seconds)`` — bump frozen clock

The context is not a library-grade API — it exists to make tests short and
readable. The goal is to reduce a typical 30-line fixture setup down to 3-5
lines, while keeping every test independent and deterministic.
"""

from __future__ import annotations

import difflib
import os
import subprocess
import sys
import textwrap
from collections.abc import Callable, Iterable, Sequence
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import pytest

from barca._hashing import _reset_auto_tick

# ---------------------------------------------------------------------------
# Frozen clock — "auto" mode is the default for tests so sequential inserts
# get distinct, monotonically-increasing timestamps without depending on the
# wall clock.
# ---------------------------------------------------------------------------

# A picked-out-of-a-hat epoch: 2024-03-25T00:00:00 UTC. Matches uv's
# EXCLUDE_NEWER for continuity of vibes.
FROZEN_BASE_TS: int = 1_711_324_800


def _install_frozen_time(base: int = FROZEN_BASE_TS) -> None:
    """Install auto-ticking frozen time for the current process."""
    os.environ["BARCA_TEST_NOW"] = f"auto:{base}"
    _reset_auto_tick()


def _uninstall_frozen_time() -> None:
    os.environ.pop("BARCA_TEST_NOW", None)
    _reset_auto_tick()


# ---------------------------------------------------------------------------
# Module cleanup
# ---------------------------------------------------------------------------


def _cleanup_modules(prefix: str) -> None:
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]


def _clear_trace_caches() -> None:
    from barca._trace import clear_caches

    clear_caches()


# ---------------------------------------------------------------------------
# CLI result helper
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class CliResult:
    """Captured output from a subprocess invocation of the barca CLI."""

    exit_code: int
    stdout: str
    stderr: str

    def success(self) -> bool:
        return self.exit_code == 0


# ---------------------------------------------------------------------------
# BarcaTestContext
# ---------------------------------------------------------------------------


@dataclass
class BarcaTestContext:
    """Isolated per-test project + store + helpers.

    Created by the ``barca_ctx`` pytest fixture (see conftest.py).

    The metadata store is created lazily on first access so that tests
    which only use ``ctx.cli()`` (subprocess invocation) never hold a
    SQLite write lock in the parent process.
    """

    root: Path
    """Absolute path to the tempdir project root (contains barca.toml)."""

    pkg_name: str
    """Name of the Python package directory inside the project (e.g. 'mymod')."""

    _store_impl: Any = None  # MetadataStore, lazily created

    _registered_modules: list[str] = field(default_factory=list)

    @property
    def store(self) -> Any:
        """Lazy accessor for the MetadataStore — creates on first access."""
        if self._store_impl is None:
            from barca._store import MetadataStore

            self._store_impl = MetadataStore(str(self.root / ".barca" / "metadata.db"))
        return self._store_impl

    @store.setter
    def store(self, value: Any) -> None:
        self._store_impl = value

    def _close_store(self) -> None:
        """Close the store connection if one exists."""
        if self._store_impl is None:
            return
        try:
            self._store_impl.conn.close()
        except Exception:
            pass
        self._store_impl = None

    # ------------------------------------------------------------------
    # Path helpers
    # ------------------------------------------------------------------

    @property
    def pkg_dir(self) -> Path:
        return self.root / self.pkg_name

    @property
    def barca_toml(self) -> Path:
        return self.root / "barca.toml"

    def artifact_path(self, rel: str) -> Path:
        """Resolve a path inside .barcafiles/ relative to the project root."""
        return self.root / rel

    # ------------------------------------------------------------------
    # Module writing
    # ------------------------------------------------------------------

    def write_module(self, rel: str, source: str) -> Path:
        """Write a Python module into the test package.

        ``rel`` is a path relative to the package directory (e.g.
        ``"assets.py"`` or ``"pipelines/etl.py"``). The source is dedented
        so callers can use triple-quoted strings with leading whitespace.

        Automatically clears module caches so a subsequent reindex picks
        up the new source.

        Returns the absolute path of the written file.
        """
        dest = self.pkg_dir / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(textwrap.dedent(source))
        _bump_mtime(dest)
        self.reload()
        return dest

    def edit_module(self, rel: str, transform: Callable[[str], str]) -> Path:
        """Read a module, apply a transform, write it back. Clears module caches."""
        dest = self.pkg_dir / rel
        old = dest.read_text()
        new = transform(old)
        dest.write_text(new)
        _bump_mtime(dest)
        self.reload()
        return dest

    def read_module(self, rel: str) -> str:
        return (self.pkg_dir / rel).read_text()

    def register_module(self, dotted: str) -> None:
        """Add a dotted module path to barca.toml's modules list.

        This is idempotent. The conftest fixture registers the default
        ``{pkg}.assets`` module automatically; call this only to add more.
        """
        if dotted in self._registered_modules:
            return
        self._registered_modules.append(dotted)
        modules_str = ", ".join(f'"{m}"' for m in self._registered_modules)
        self.barca_toml.write_text(
            textwrap.dedent(f"""\
            [project]
            modules = [{modules_str}]
            """)
        )
        # Force re-import of anything that might cache module state
        _cleanup_modules(self.pkg_name)
        _clear_trace_caches()

    def reload(self) -> None:
        """Force re-import of the test package so source edits take effect.

        Clears Python module cache, trace caches, AND the ``__pycache__``
        directories for this package. The pycache cleanup is critical: Python
        caches compiled bytecode in ``__pycache__/*.pyc`` and uses mtime
        comparison to decide when to recompile. In rapid test edits the
        mtime may appear unchanged, so we just nuke the cache directories.
        """
        _cleanup_modules(self.pkg_name)
        _clear_trace_caches()
        import shutil as _shutil

        for pycache in self.pkg_dir.rglob("__pycache__"):
            try:
                _shutil.rmtree(pycache)
            except OSError:
                pass

    # ------------------------------------------------------------------
    # Engine actions
    # ------------------------------------------------------------------

    def reindex(self):
        """Call engine.reindex and return whatever it returns.

        In Phase 2 this returns a list[AssetSummary]; after Phase 3 it will
        return a ReindexDiff. The context doesn't care — tests assert on
        the returned object directly.
        """
        from barca._engine import reindex as _reindex

        self.reload()
        return _reindex(self.store, self.root)

    def run_pass(self):
        """Call _run.run_pass. Will raise NotImplementedError during Phase 2."""
        from barca._run import run_pass as _run_pass

        self.reload()
        return _run_pass(self.store, self.root)

    def refresh(self, asset_id: int, *, stale_policy: str = "error"):
        """Explicitly refresh one asset."""
        from barca._engine import refresh as _refresh

        # Not every existing refresh signature accepts stale_policy yet.
        # Try the new signature first; fall back to the old one.
        try:
            return _refresh(self.store, self.root, asset_id, stale_policy=stale_policy)
        except TypeError:
            return _refresh(self.store, self.root, asset_id)

    def trigger_sensor(self, asset_id: int):
        from barca._engine import trigger_sensor as _trigger

        return _trigger(self.store, self.root, asset_id)

    def prune(self):
        """Call _prune.prune. Will raise NotImplementedError during Phase 2."""
        from barca._prune import prune as _prune

        return _prune(self.store, self.root)

    # ------------------------------------------------------------------
    # Store lookups
    # ------------------------------------------------------------------

    def asset_id(self, logical_name: str) -> int | None:
        return self.store.asset_id_by_logical_name(logical_name)

    def asset_id_by_function(self, function_name: str) -> int | None:
        """Look up an asset by function name. Returns None if not found.

        Raises if multiple assets share the same function_name (which
        usually means you forgot a ``name=`` kwarg).
        """
        assets = [a for a in self.store.list_assets() if a.function_name == function_name]
        if not assets:
            return None
        if len(assets) > 1:
            raise LookupError(f"multiple assets with function_name={function_name!r}")
        return assets[0].asset_id

    # ------------------------------------------------------------------
    # Snapshots
    # ------------------------------------------------------------------

    def assets_snapshot(
        self,
        *,
        show: Sequence[str] = ("kind", "freshness", "status"),
    ) -> str:
        """Return a stable text representation of every asset in the DAG.

        Columns are configurable so tests can focus on the axis they care
        about (e.g. only ``status`` for a reconcile-state test).

        Normalization:
            - Sorted by logical_name
            - Volatile fields (hashes, timestamps, asset_id) omitted
            - Absolute paths normalized to '<root>/...'
            - Output ends in a newline
        """
        rows = sorted(self.store.list_assets(), key=lambda a: a.logical_name)
        lines: list[str] = []
        for row in rows:
            parts = [row.logical_name]
            for field_name in show:
                value = _asset_field(row, field_name)
                parts.append(f"{field_name}={value}")
            lines.append(" | ".join(parts))
        text = "\n".join(lines)
        if text:
            text += "\n"
        return _normalize_paths(text, self.root)

    def diff_assets(
        self,
        action: Callable[[BarcaTestContext], Any],
        *,
        show: Sequence[str] = ("kind", "freshness", "status"),
    ) -> str:
        """Snapshot the unified diff of assets_snapshot() around an action.

        Usage::

            diff = ctx.diff_assets(lambda c: c.run_pass())
            assert diff == snapshot('''...''')

        This is analogous to uv's ``diff_lock`` pattern — it surfaces only
        what changed, which is far more readable than snapshotting the full
        post-state.
        """
        before = self.assets_snapshot(show=show)
        action(self)
        after = self.assets_snapshot(show=show)
        diff_lines = list(
            difflib.unified_diff(
                before.splitlines(keepends=True),
                after.splitlines(keepends=True),
                fromfile="before",
                tofile="after",
                n=3,
            )
        )
        return "".join(diff_lines) or "(no changes)\n"

    # ------------------------------------------------------------------
    # Time
    # ------------------------------------------------------------------

    def advance_time(self, seconds: int) -> None:
        """Bump the frozen clock base by ``seconds``.

        The auto-tick counter is NOT reset; subsequent timestamps continue
        monotonically from (base + seconds + auto_tick).
        """
        current = os.environ.get("BARCA_TEST_NOW", "")
        if not current.startswith("auto:"):
            raise RuntimeError("advance_time requires BARCA_TEST_NOW=auto:<base>")
        base = int(current[len("auto:") :])
        os.environ["BARCA_TEST_NOW"] = f"auto:{base + seconds}"

    def freeze_time(self, value: int) -> None:
        """Switch to fully-frozen mode at ``value``.

        Every subsequent ``now_ts()`` call returns exactly ``value``. Useful
        for tests that need absolute equality of timestamps across inserts.
        """
        os.environ["BARCA_TEST_NOW"] = str(value)
        _reset_auto_tick()

    # ------------------------------------------------------------------
    # CLI (subprocess)
    # ------------------------------------------------------------------

    def cli(self, args: Sequence[str], *, check: bool = False, timeout: float = 30) -> CliResult:
        """Invoke ``barca <args>`` as a subprocess in the project root.

        Closes the parent's store connection before spawning so the
        subprocess can acquire the SQLite write lock, then reopens it
        afterwards so the test can keep inspecting state.
        """
        env = os.environ.copy()
        if "BARCA_TEST_NOW" in os.environ:
            env["BARCA_TEST_NOW"] = os.environ["BARCA_TEST_NOW"]

        # Release the parent's DB lock (if a store was ever created)
        self._close_store()

        # Pin --project to the workspace root so uv uses the dev barca,
        # not whatever it finds via $PATH or the test's tempdir.
        import barca as _barca_module

        barca_src = Path(_barca_module.__file__).resolve()
        # Walk up to the workspace root (contains pyproject.toml)
        workspace_root = barca_src
        for parent in barca_src.parents:
            if (parent / "pyproject.toml").exists() and (parent / ".venv").exists():
                workspace_root = parent
                break
        try:
            result = subprocess.run(
                ["uv", "run", "--project", str(workspace_root), "barca", *args],
                cwd=self.root,
                env=env,
                capture_output=True,
                text=True,
                timeout=timeout,
                check=check,
            )
        finally:
            # The store will be lazily re-created on next access
            pass

        return CliResult(
            exit_code=result.returncode,
            stdout=result.stdout,
            stderr=result.stderr,
        )


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _bump_mtime(path: Path) -> None:
    """Force the file's mtime forward so Python re-compiles its bytecode.

    Python caches compiled bytecode in __pycache__ and uses mtime + size to
    decide when to re-compile. Tests that edit a file and re-import in the
    same second can hit stale bytecode. Bumping the mtime by 2 seconds
    ensures Python sees the file as newer than any cache.
    """
    import time

    future = time.time() + 2
    try:
        # Also invalidate any __pycache__ for this file
        import importlib

        importlib.invalidate_caches()
    except Exception:
        pass
    try:
        path.touch()
        # os.utime with future mtime
        import os as _os

        _os.utime(path, (future, future))
    except OSError:
        pass


def _asset_field(row: Any, field_name: str) -> str:
    """Extract a field from an AssetSummary, tolerating schema drift."""
    if field_name == "status":
        # Derived from materialization_status — fresh/stale/failed/None.
        status = getattr(row, "materialization_status", None)
        if status is None:
            return "unmaterialized"
        return str(status)
    if field_name == "freshness":
        # After Phase 3 this will be a dedicated field. Fall back to
        # the existing schedule field for now so tests written before
        # Phase 3 still snapshot something meaningful.
        return str(getattr(row, "freshness", None) or getattr(row, "schedule", "?"))
    value = getattr(row, field_name, None)
    return str(value) if value is not None else "?"


def _normalize_paths(text: str, root: Path) -> str:
    return text.replace(str(root), "<root>")


# ---------------------------------------------------------------------------
# pytest fixture
# ---------------------------------------------------------------------------


def make_ctx(tmp_path: Path, pkg_name: str = "tproj") -> BarcaTestContext:
    """Build a BarcaTestContext rooted at tmp_path.

    Creates:
        <tmp_path>/project/
            barca.toml
            <pkg_name>/
                __init__.py

    The store is created lazily on first access via the ``store`` property,
    so tests that only use ``ctx.cli()`` never hold a DB lock.
    """
    project_root = tmp_path / "project"
    project_root.mkdir()
    pkg_dir = project_root / pkg_name
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")
    (project_root / "barca.toml").write_text(
        textwrap.dedent(f"""\
        [project]
        modules = ["{pkg_name}.assets"]
        """)
    )

    sys.path.insert(0, str(project_root))

    ctx = BarcaTestContext(
        root=project_root,
        pkg_name=pkg_name,
        _registered_modules=[f"{pkg_name}.assets"],
    )
    return ctx


@pytest.fixture
def barca_ctx(tmp_path: Path) -> Iterable[BarcaTestContext]:
    """The primary test fixture. Yields a fresh BarcaTestContext per test.

    Automatically installs frozen time (``BARCA_TEST_NOW=auto:...``) for the
    duration of the test, and cleans up modules + caches afterwards.
    """
    _install_frozen_time()
    ctx = make_ctx(tmp_path)
    try:
        yield ctx
    finally:
        if str(ctx.root) in sys.path:
            sys.path.remove(str(ctx.root))
        _cleanup_modules(ctx.pkg_name)
        _clear_trace_caches()
        _uninstall_frozen_time()
