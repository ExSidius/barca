"""Tests for assets that use multiple CPU cores internally (e.g. LightGBM, sklearn n_jobs=-1).

The core scenario: barca's ThreadPoolExecutor runs N partitions simultaneously; each
partition function itself spawns threads (via OpenMP, joblib threading backend, etc.).
This creates nested concurrency that must not deadlock, corrupt results, or lose data.
"""

from __future__ import annotations

import json
import sys
import textwrap
from pathlib import Path

import pytest

from barca._engine import refresh, reindex
from barca._store import MetadataStore


def _cleanup_modules(prefix: str) -> None:
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]


@pytest.fixture
def multicore_project(tmp_path: Path):
    """Project with assets that use multiple CPU cores internally.

    Uses sklearn with the threading joblib backend, which directly mirrors
    how LightGBM (and other OpenMP-based libraries) spawn threads within the
    same process rather than subprocesses.
    """
    project_dir = tmp_path / "mc_proj"
    project_dir.mkdir()

    mod_dir = project_dir / "mcmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, partitions


        @asset()
        def train_single() -> dict:
            '''Single asset that trains with in-process thread parallelism.'''
            from joblib import parallel_backend
            from sklearn.datasets import make_classification
            from sklearn.ensemble import RandomForestClassifier

            X, y = make_classification(n_samples=300, n_features=10, random_state=42)
            # Force threading backend — same as LightGBM/OpenMP: threads inside the process
            with parallel_backend("threading", n_jobs=4):
                clf = RandomForestClassifier(n_estimators=20, random_state=42)
                clf.fit(X, y)
            return {"accuracy": round(float(clf.score(X, y)), 4), "n_estimators": 20}


        @asset(partitions={"fold": partitions(["fold_0", "fold_1", "fold_2"])})
        def train_fold(fold: str) -> dict:
            '''Each partition trains with in-process thread parallelism.

            When barca runs these in parallel (max_workers=3), we get 3 barca
            threads each spawning 4 sklearn/joblib threads — 12 threads total
            competing for the GIL-less interpreter. This must not deadlock or
            corrupt results.
            '''
            from joblib import parallel_backend
            from sklearn.datasets import make_classification
            from sklearn.ensemble import RandomForestClassifier

            seed = int(fold.split("_")[1])
            X, y = make_classification(n_samples=300, n_features=10, random_state=seed)
            with parallel_backend("threading", n_jobs=4):
                clf = RandomForestClassifier(n_estimators=20, random_state=seed)
                clf.fit(X, y)
            return {"fold": fold, "accuracy": round(float(clf.score(X, y)), 4)}
    """)
    )

    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["mcmod.assets"]
    """)
    )

    _cleanup_modules("mcmod")
    sys.path.insert(0, str(project_dir))
    yield project_dir
    sys.path.remove(str(project_dir))
    _cleanup_modules("mcmod")
    from barca._trace import clear_caches

    clear_caches()


# ---------------------------------------------------------------------------
# Single (non-partitioned) multicore asset
# ---------------------------------------------------------------------------


def test_single_asset_multicore_succeeds(multicore_project: Path) -> None:
    """A non-partitioned asset using in-process thread parallelism materializes correctly."""
    store = MetadataStore(str(multicore_project / ".barca" / "metadata.db"))
    assets = reindex(store, multicore_project)
    asset_id = next(a.asset_id for a in assets if "train_single" in a.logical_name)

    refresh(store, multicore_project, asset_id)

    pairs = store.list_recent_materializations(10)
    success = [mat for mat, _ in pairs if mat.status == "success" and mat.asset_id == asset_id]
    assert len(success) == 1

    result = json.loads((multicore_project / success[0].artifact_path).read_text())
    assert result["accuracy"] > 0.5
    assert result["n_estimators"] == 20


# ---------------------------------------------------------------------------
# Partitioned multicore assets
# ---------------------------------------------------------------------------


def test_partitioned_multicore_sequential(multicore_project: Path) -> None:
    """Partitioned multicore asset runs correctly with max_workers=1."""
    store = MetadataStore(str(multicore_project / ".barca" / "metadata.db"))
    assets = reindex(store, multicore_project)
    asset_id = next(a.asset_id for a in assets if "train_fold" in a.logical_name)

    refresh(store, multicore_project, asset_id, max_workers=1)

    pairs = store.list_recent_materializations(20)
    success = [mat for mat, _ in pairs if mat.status == "success" and mat.asset_id == asset_id]
    assert len(success) == 3

    folds = {json.loads((multicore_project / m.artifact_path).read_text())["fold"] for m in success}
    assert folds == {"fold_0", "fold_1", "fold_2"}


def test_partitioned_multicore_parallel(multicore_project: Path) -> None:
    """Partitioned multicore asset runs correctly with max_workers=3.

    This is the primary regression test: 3 barca partition threads each spawn
    4 sklearn/joblib threads — simulating the LightGBM n_jobs scenario on a
    free-threaded Python interpreter (GIL disabled). All partitions must
    complete without deadlock and produce correct, distinct results.
    """
    store = MetadataStore(str(multicore_project / ".barca" / "metadata.db"))
    assets = reindex(store, multicore_project)
    asset_id = next(a.asset_id for a in assets if "train_fold" in a.logical_name)

    # Run all 3 partitions in parallel — each partition spawns 4 internal threads
    refresh(store, multicore_project, asset_id, max_workers=3)

    pairs = store.list_recent_materializations(20)
    success = [mat for mat, _ in pairs if mat.status == "success" and mat.asset_id == asset_id]
    assert len(success) == 3, f"Expected 3 successful materializations, got {len(success)}"

    folds = {json.loads((multicore_project / m.artifact_path).read_text())["fold"] for m in success}
    assert folds == {"fold_0", "fold_1", "fold_2"}

    for mat in success:
        result = json.loads((multicore_project / mat.artifact_path).read_text())
        assert result["accuracy"] > 0.5, f"Partition {result['fold']} produced low accuracy ({result['accuracy']})"


def test_partitioned_multicore_results_are_reproducible(multicore_project: Path) -> None:
    """Running the same partitioned multicore asset twice returns cached results.

    Verifies that parallel multicore execution produces stable, cacheable outputs —
    not non-deterministic garbage from thread-unsafe operations.
    """
    store = MetadataStore(str(multicore_project / ".barca" / "metadata.db"))
    assets = reindex(store, multicore_project)
    asset_id = next(a.asset_id for a in assets if "train_fold" in a.logical_name)

    refresh(store, multicore_project, asset_id, max_workers=3)
    first_run = {
        json.loads((multicore_project / m.artifact_path).read_text())["fold"]: m.artifact_checksum
        for m, _ in store.list_recent_materializations(20)
        if m.status == "success" and m.asset_id == asset_id
    }

    # Second run must hit cache (no new materializations)
    refresh(store, multicore_project, asset_id, max_workers=3)
    all_mats = [mat for mat, _ in store.list_recent_materializations(20) if mat.asset_id == asset_id]
    assert len(all_mats) == 3, "Cache should have been hit — no new materializations"

    second_run = {json.loads((multicore_project / m.artifact_path).read_text())["fold"]: m.artifact_checksum for m in all_mats if m.status == "success"}
    assert first_run == second_run, "Checksums changed between runs — non-deterministic output"
