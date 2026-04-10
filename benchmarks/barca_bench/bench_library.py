"""Barca library-mode benchmarks — in-process, no CLI subprocess overhead.

Apples-to-apples comparison with Dagster/Prefect which also run in-process.
"""

import math
import os
import sys
import time

BENCH_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(os.path.dirname(BENCH_DIR))

# Add bench dir to path so bench_project modules are importable
if BENCH_DIR not in sys.path:
    sys.path.insert(0, BENCH_DIR)

from barca._engine import refresh, reindex, reset
from barca._store import MetadataStore


def _store():
    db_path = os.path.join(BENCH_DIR, ".barca", "metadata.db")
    return MetadataStore(db_path)


def _reset():
    from pathlib import Path

    reset(Path(BENCH_DIR), db=True, artifacts=True)


def _find_asset_id(store, name):
    for a in store.list_assets():
        if name in a.logical_name:
            return a.asset_id
    raise RuntimeError(f"Asset '{name}' not found")


# ── Cold start ──────────────────────────────────────────────────────────────


def bench_cold_start(runs):
    from pathlib import Path

    root = Path(BENCH_DIR)

    print(f"[barca in-process] Cold start: single asset ({runs} runs):")
    times = []
    for i in range(runs):
        _reset()
        t0 = time.perf_counter()
        store = _store()
        reindex(store, root)
        asset_id = _find_asset_id(store, "single_asset")
        refresh(store, root, asset_id)
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i + 1}: {elapsed:.3f}s")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"[barca in-process] Cold start avg: {avg:.3f}s +/- {std:.3f}s")


# ── Spaceflights ────────────────────────────────────────────────────────────


def bench_spaceflights(runs, max_workers=None):
    from pathlib import Path

    root = Path(BENCH_DIR)

    label = f"-j {max_workers}" if max_workers else "default"

    # Warmup: import modules and run once to warm sklearn etc.
    _reset()
    store = _store()
    reindex(store, root)
    asset_id = _find_asset_id(store, "evaluate")
    refresh(store, root, asset_id, max_workers=max_workers)

    print(f"[barca in-process {label}] Spaceflights ({runs} runs):")
    times = []
    for i in range(runs):
        _reset()
        store = _store()
        reindex(store, root)
        asset_id = _find_asset_id(store, "evaluate")

        t0 = time.perf_counter()
        refresh(store, root, asset_id, max_workers=max_workers)
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i + 1}: {elapsed:.3f}s")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"[barca in-process {label}] Spaceflights: {avg:.3f}s +/- {std:.3f}s")


# ── 500 partitions x 50ms ──────────────────────────────────────────────────


def bench_parallel(runs, max_workers=None):
    from pathlib import Path

    root = Path(BENCH_DIR)

    label = f"-j {max_workers}" if max_workers else "default"

    # Warmup
    _reset()
    store = _store()
    reindex(store, root)
    asset_id = _find_asset_id(store, "parallel_500")
    refresh(store, root, asset_id, max_workers=max_workers)

    print(f"[barca in-process {label}] 500 partitions x 50ms ({runs} runs):")
    times = []
    for i in range(runs):
        _reset()
        store = _store()
        reindex(store, root)
        asset_id = _find_asset_id(store, "parallel_500")

        t0 = time.perf_counter()
        refresh(store, root, asset_id, max_workers=max_workers)
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i + 1}: {elapsed:.2f}s")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"[barca in-process {label}] 500 x 50ms: {avg:.2f}s +/- {std:.2f}s")


# ── 500 trivial ────────────────────────────────────────────────────────────


def bench_trivial(runs, max_workers=None):
    from pathlib import Path

    root = Path(BENCH_DIR)

    label = f"-j {max_workers}" if max_workers else "default"

    # Warmup
    _reset()
    store = _store()
    reindex(store, root)
    asset_id = _find_asset_id(store, "trivial_500")
    refresh(store, root, asset_id, max_workers=max_workers)

    print(f"[barca in-process {label}] 500 trivial partitions ({runs} runs):")
    times = []
    for i in range(runs):
        _reset()
        store = _store()
        reindex(store, root)
        asset_id = _find_asset_id(store, "trivial_500")

        t0 = time.perf_counter()
        refresh(store, root, asset_id, max_workers=max_workers)
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i + 1}: {elapsed:.2f}s")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"[barca in-process {label}] 500 trivial: {avg:.2f}s +/- {std:.2f}s")


if __name__ == "__main__":
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    max_workers = int(sys.argv[2]) if len(sys.argv) > 2 else None
    mode = sys.argv[3] if len(sys.argv) > 3 else "all"

    if mode in ("all", "cold"):
        bench_cold_start(runs)
        print()
    if mode in ("all", "spaceflights"):
        bench_spaceflights(runs, max_workers)
        print()
    if mode in ("all", "parallel"):
        bench_parallel(runs, max_workers)
        print()
    if mode in ("all", "trivial"):
        bench_trivial(runs, max_workers)
        print()
