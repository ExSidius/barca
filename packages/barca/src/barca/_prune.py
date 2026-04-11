"""barca prune — remove unreachable history and artifacts.

``prune(store, repo_root)`` removes:

- DB rows for assets whose ``active = 0`` (removed from the DAG)
- Their materializations, sensor observations, effect executions, sink executions
- Filesystem ``.barcafiles/{slug}/`` directories for removed assets
- Stale ``definition_hash`` subdirs for still-active assets (old versions)

``barca prune`` is the only destructive operation that permanently deletes
history. All other operations (reindex, deactivate, rename) preserve it.
"""

from __future__ import annotations

import logging
import shutil
from pathlib import Path
from typing import TYPE_CHECKING

from barca._models import PruneResult

if TYPE_CHECKING:
    from barca._store import MetadataStore

logger = logging.getLogger(__name__)


def prune(store: MetadataStore, repo_root: Path) -> PruneResult:
    """Remove unreachable history from the store and the filesystem."""
    active_ids = store.list_active_asset_ids()

    # Snapshot slugs of inactive assets BEFORE we delete their rows so we
    # know which artifact directories to remove from disk.
    inactive_slugs: list[str] = []
    rows = store.conn.execute("SELECT asset_slug FROM assets WHERE active = 0").fetchall()
    for row in rows:
        slug = row[0]
        if slug:
            inactive_slugs.append(slug)

    # Delete DB rows for unreachable assets
    result = store.prune_unreachable(active_ids)

    # Delete filesystem artifacts for removed assets
    artifacts_root = repo_root / ".barcafiles"
    if artifacts_root.exists():
        for slug in inactive_slugs:
            slug_dir = artifacts_root / slug
            if slug_dir.exists():
                try:
                    shutil.rmtree(slug_dir)
                    result.removed_artifact_files += 1
                except OSError as exc:  # pragma: no cover
                    logger.warning(f"failed to remove {slug_dir}: {exc}")

    return result
