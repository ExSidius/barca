"""Thread-safety tests for MetadataStore with Turso/libSQL.

CLAUDE.md states: "MetadataStore should still be created per-thread for server
routes (to_thread pattern)." These tests verify that pattern is safe under
concurrent load — multiple threads each owning their own store instance against
the same DB file must not produce data races, lost writes, or corruption.
"""

from __future__ import annotations

import json
import threading
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

from barca._models import IndexedAsset
from barca._store import MetadataStore

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_asset(n: int) -> IndexedAsset:
    """Return a minimal IndexedAsset with a unique continuity_key."""
    return IndexedAsset(
        logical_name=f"mod.py:asset_{n}",
        continuity_key=f"mod.py:asset_{n}",
        module_path="mod",
        file_path="mod.py",
        function_name=f"asset_{n}",
        asset_slug=f"asset_{n}",
        kind="asset",
        definition_hash=f"def_hash_{n}",
        run_hash=f"run_hash_{n}",
        source_text="def f(): pass",
        module_source_text="",
        decorator_metadata_json=json.dumps({"freshness": "manual"}),
        return_type=None,
        serializer_kind="json",
        python_version="3.14",
        codebase_hash="",
        dependency_cone_hash="",
    )


def _store(db_path: str) -> MetadataStore:
    """Create a per-thread store instance (the recommended pattern)."""
    return MetadataStore(db_path)


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------


def test_concurrent_upsert_unique_assets(tmp_path: Path) -> None:
    """N threads each upsert a distinct asset — all rows must be written."""
    db_path = str(tmp_path / ".barca" / "metadata.db")
    n_threads = 20

    errors: list[Exception] = []

    def worker(n: int) -> None:
        try:
            store = _store(db_path)
            store.upsert_indexed_asset(_make_asset(n))
        except Exception as exc:
            errors.append(exc)

    with ThreadPoolExecutor(max_workers=n_threads) as pool:
        futures = [pool.submit(worker, i) for i in range(n_threads)]
        for f in as_completed(futures):
            f.result()  # re-raise any exception

    assert not errors, errors

    # Verify all rows landed
    store = _store(db_path)
    assets = store.list_assets()
    assert len(assets) == n_threads


def test_concurrent_upsert_same_asset_is_idempotent(tmp_path: Path) -> None:
    """N threads upsert the exact same asset concurrently — result is one row."""
    db_path = str(tmp_path / ".barca" / "metadata.db")
    n_threads = 20
    asset = _make_asset(0)

    errors: list[Exception] = []

    def worker(_: int) -> None:
        try:
            store = _store(db_path)
            store.upsert_indexed_asset(asset)
        except Exception as exc:
            errors.append(exc)

    with ThreadPoolExecutor(max_workers=n_threads) as pool:
        futures = [pool.submit(worker, i) for i in range(n_threads)]
        for f in as_completed(futures):
            f.result()

    assert not errors, errors

    store = _store(db_path)
    assets = store.list_assets()
    assert len(assets) == 1


def test_concurrent_insert_materializations(tmp_path: Path) -> None:
    """N threads insert materialization records for the same asset."""
    db_path = str(tmp_path / ".barca" / "metadata.db")

    # Set up the asset first (single-threaded)
    setup_store = _store(db_path)
    setup_store.upsert_indexed_asset(_make_asset(0))
    assets = setup_store.list_assets()
    asset = assets[0]
    n_threads = 20

    errors: list[Exception] = []
    mat_ids: list[int] = []
    lock = threading.Lock()

    def worker(n: int) -> None:
        try:
            store = _store(db_path)
            mat_id = store.insert_queued_materialization(
                asset_id=asset.asset_id,
                definition_id=asset.asset_id,  # reuse for simplicity
                run_hash=f"run_{n}",
            )
            with lock:
                mat_ids.append(mat_id)
        except Exception as exc:
            errors.append(exc)

    with ThreadPoolExecutor(max_workers=n_threads) as pool:
        futures = [pool.submit(worker, i) for i in range(n_threads)]
        for f in as_completed(futures):
            f.result()

    assert not errors, errors
    # All IDs must be unique (no duplicates from concurrent last_insert_rowid)
    assert len(set(mat_ids)) == n_threads, f"Duplicate mat IDs: {mat_ids}"


def test_concurrent_reads_do_not_block(tmp_path: Path) -> None:
    """N threads reading list_assets concurrently must all succeed."""
    db_path = str(tmp_path / ".barca" / "metadata.db")

    setup_store = _store(db_path)
    for i in range(5):
        setup_store.upsert_indexed_asset(_make_asset(i))

    errors: list[Exception] = []
    results: list[int] = []
    lock = threading.Lock()

    def reader(_: int) -> None:
        try:
            store = _store(db_path)
            assets = store.list_assets()
            with lock:
                results.append(len(assets))
        except Exception as exc:
            errors.append(exc)

    with ThreadPoolExecutor(max_workers=30) as pool:
        futures = [pool.submit(reader, i) for i in range(30)]
        for f in as_completed(futures):
            f.result()

    assert not errors, errors
    assert all(r == 5 for r in results), f"Inconsistent read results: {results}"
