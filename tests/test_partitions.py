"""W3: Partitioned assets."""

import json

from barca._engine import refresh, reindex
from barca._store import MetadataStore


def test_partition_creates_n_jobs(partition_project):
    store = MetadataStore(str(partition_project / ".barca" / "metadata.db"))
    assets = reindex(store, partition_project)
    prices_id = next(a.asset_id for a in assets if "fetch_prices" in a.logical_name)

    refresh(store, partition_project, prices_id)

    # Should have 3 successful materializations (one per partition)
    pairs = store.list_recent_materializations(50)
    success_count = sum(1 for mat, _ in pairs if mat.status == "success" and mat.asset_id == prices_id)
    assert success_count == 3


def test_partition_kwargs_correct(partition_project):
    store = MetadataStore(str(partition_project / ".barca" / "metadata.db"))
    assets = reindex(store, partition_project)
    prices_id = next(a.asset_id for a in assets if "fetch_prices" in a.logical_name)

    refresh(store, partition_project, prices_id)

    # Check each partition artifact has the correct ticker
    barcafiles = partition_project / ".barcafiles"
    tickers_found = set()
    for value_file in barcafiles.rglob("value.json"):
        value = json.loads(value_file.read_text())
        if "ticker" in value:
            tickers_found.add(value["ticker"])

    assert tickers_found == {"AAPL", "MSFT", "GOOG"}


def test_partition_run_hashes_distinct(partition_project):
    store = MetadataStore(str(partition_project / ".barca" / "metadata.db"))
    assets = reindex(store, partition_project)
    prices_id = next(a.asset_id for a in assets if "fetch_prices" in a.logical_name)

    refresh(store, partition_project, prices_id)

    pairs = store.list_recent_materializations(50)
    run_hashes = {mat.run_hash for mat, _ in pairs if mat.asset_id == prices_id}
    assert len(run_hashes) == 3, "each partition should have a distinct run_hash"


def test_partition_artifacts_in_subdirs(partition_project):
    store = MetadataStore(str(partition_project / ".barca" / "metadata.db"))
    assets = reindex(store, partition_project)
    prices_id = next(a.asset_id for a in assets if "fetch_prices" in a.logical_name)

    refresh(store, partition_project, prices_id)

    barcafiles = partition_project / ".barcafiles"
    partition_dirs = list(barcafiles.rglob("partitions"))
    assert len(partition_dirs) >= 1, "should have partitions subdirectory"

    value_files = list(barcafiles.rglob("value.json"))
    assert len(value_files) == 3, "should have 3 value.json files"


def test_partition_second_run_cached(partition_project):
    store = MetadataStore(str(partition_project / ".barca" / "metadata.db"))
    assets = reindex(store, partition_project)
    prices_id = next(a.asset_id for a in assets if "fetch_prices" in a.logical_name)

    refresh(store, partition_project, prices_id)
    pairs1 = store.list_recent_materializations(50)
    count1 = sum(1 for mat, _ in pairs1 if mat.status == "success" and mat.asset_id == prices_id)

    # Second run should hit cache
    refresh(store, partition_project, prices_id)
    pairs2 = store.list_recent_materializations(50)
    count2 = sum(1 for mat, _ in pairs2 if mat.status == "success" and mat.asset_id == prices_id)

    # Should still be 3 (no new jobs created)
    assert count2 == count1


def test_partition_parallel_execution(partition_project):
    """Partitions run in parallel with max_workers > 1 and produce correct results."""
    store = MetadataStore(str(partition_project / ".barca" / "metadata.db"))
    assets = reindex(store, partition_project)
    prices_id = next(a.asset_id for a in assets if "fetch_prices" in a.logical_name)

    refresh(store, partition_project, prices_id, max_workers=3)

    pairs = store.list_recent_materializations(50)
    success_count = sum(1 for mat, _ in pairs if mat.status == "success" and mat.asset_id == prices_id)
    assert success_count == 3

    # Verify all partition artifacts are correct
    barcafiles = partition_project / ".barcafiles"
    tickers_found = set()
    for value_file in barcafiles.rglob("value.json"):
        value = json.loads(value_file.read_text())
        if "ticker" in value:
            tickers_found.add(value["ticker"])
    assert tickers_found == {"AAPL", "MSFT", "GOOG"}
