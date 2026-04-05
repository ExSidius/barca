"""Background reconcile loop — runs as an asyncio task."""

from __future__ import annotations

import asyncio
import logging
from pathlib import Path

from barca._store import MetadataStore
from barca.server.service import run_reconcile

logger = logging.getLogger("barca.server.scheduler")


def _tick(db_path: str, repo_root: Path) -> None:
    """Run a single reconcile pass. Called inside executor thread."""
    store = MetadataStore(db_path)
    run_reconcile(store, repo_root)


async def scheduler_loop(
    repo_root: Path,
    db_path: str,
    interval: int,
    lock: asyncio.Lock,
) -> None:
    """Run reconcile() every `interval` seconds, holding `lock` during execution."""
    logger.info("scheduler started (interval=%ds)", interval)
    while True:
        try:
            async with lock:
                await asyncio.to_thread(_tick, db_path, repo_root)
        except asyncio.CancelledError:
            logger.info("scheduler stopped")
            raise
        except Exception:
            logger.exception("scheduler tick failed")
        await asyncio.sleep(interval)
