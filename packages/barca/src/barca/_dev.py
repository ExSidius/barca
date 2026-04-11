"""barca dev — file watcher that tracks staleness without materialising.

Two entry points:

- ``handle_file_change(store, repo_root)`` — pure function. Calls
  ``reindex()``, which updates the store with any new/changed definitions.
  Any asset whose source changed will have a new ``definition_hash``, and
  ``store.list_assets()`` will show it as stale. Unit-testable.

- ``dev_watch(store, repo_root, stop_event)`` — long-running file watcher
  that calls ``handle_file_change`` on every ``*.py`` modification.

Dev mode never materialises anything — the whole point is to show the
live state of the DAG as the developer types, without triggering any
computation.
"""

from __future__ import annotations

import logging
from pathlib import Path
from threading import Event
from typing import TYPE_CHECKING

from barca._engine import reindex

if TYPE_CHECKING:
    from barca._models import ReindexDiff
    from barca._store import MetadataStore

logger = logging.getLogger(__name__)


def handle_file_change(store: MetadataStore, repo_root: Path) -> ReindexDiff:
    """Called whenever a watched file changes. Pure reindex dispatch."""
    return reindex(store, repo_root)


def dev_watch(
    store: MetadataStore,
    repo_root: Path,
    stop_event: Event | None = None,
) -> None:
    """Watch the project for file changes and update staleness live.

    Uses ``watchfiles`` if available, falls back to a simple polling
    loop otherwise. The watcher intentionally does NOT materialise
    anything — it only calls ``handle_file_change``.
    """
    if stop_event is None:
        stop_event = Event()

    try:
        from watchfiles import watch

        for _changes in watch(
            str(repo_root),
            stop_event=stop_event,
            recursive=True,
        ):
            try:
                handle_file_change(store, repo_root)
            except Exception as exc:  # pragma: no cover
                logger.warning(f"reindex error during dev watch: {exc}")
            if stop_event.is_set():
                break
    except ImportError:
        # Fallback: poll for mtime changes on .py files every 0.5s
        _poll_loop(store, repo_root, stop_event)


def _poll_loop(
    store: MetadataStore,
    repo_root: Path,
    stop_event: Event,
    interval: float = 0.5,
) -> None:
    """Simple polling fallback when watchfiles isn't installed."""
    seen_mtimes: dict[Path, float] = {}
    while not stop_event.is_set():
        changed = False
        for py_file in repo_root.rglob("*.py"):
            try:
                mtime = py_file.stat().st_mtime
            except OSError:
                continue
            prev = seen_mtimes.get(py_file)
            if prev is None or prev != mtime:
                seen_mtimes[py_file] = mtime
                changed = True
        if changed:
            try:
                handle_file_change(store, repo_root)
            except Exception as exc:  # pragma: no cover
                logger.warning(f"reindex error during dev poll: {exc}")
        if stop_event.wait(timeout=interval):
            break
