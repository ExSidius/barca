"""DB change notifier — decoupled from write source.

Both CLI and HTTP API write to Turso via the same _engine.py functions.
This module watches the WAL file for any write (from any source) and
notifies all active SSE watch streams via asyncio.Event.

Latency: ~20-60ms (kernel FSEvents on macOS, inotify on Linux).
"""

from __future__ import annotations

import asyncio
import logging
from pathlib import Path

from watchfiles import awatch

logger = logging.getLogger("barca.server.notifier")


class ChangeNotifier:
    """
    Singleton notification hub. SSE watch streams subscribe to receive
    one-shot asyncio.Events that fire whenever any process writes to the DB.
    """

    def __init__(self) -> None:
        self._subscribers: set[asyncio.Event] = set()

    def subscribe(self) -> asyncio.Event:
        """Register a one-shot Event. Fires on the next DB write."""
        ev = asyncio.Event()
        self._subscribers.add(ev)
        return ev

    def unsubscribe(self, ev: asyncio.Event) -> None:
        """Explicitly unsubscribe (called on SSE stream disconnect)."""
        self._subscribers.discard(ev)

    def notify(self) -> None:
        """Wake all waiting coroutines. Called by watch_wal on every DB write."""
        for ev in self._subscribers:
            ev.set()
        self._subscribers.clear()  # subscribers re-register after each wake

    async def watch_wal(self, db_path: Path) -> None:
        """
        Long-running background task: watches the .barca/ directory for WAL writes.

        The WAL file (.barca/metadata.db-wal) is modified on every libSQL commit.
        watchfiles uses FSEvents (macOS) or inotify (Linux) — kernel-level, not polling.
        debounce=50ms, step=10ms → notification within ~20-60ms of any DB write.

        This fires for ALL writes: HTTP API, CLI, scheduler, anything that touches
        the DB. SSE watch streams simply await the Event and query DB to see what changed.
        """
        watch_dir = str(db_path.parent)
        logger.info("WAL watcher started on %s", watch_dir)
        async for _changes in awatch(watch_dir, debounce=50, step=10):
            self.notify()


# Module-level singleton — shared across all request handlers via closure in create_app
notifier = ChangeNotifier()
