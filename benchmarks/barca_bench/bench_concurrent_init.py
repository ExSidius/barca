"""Barca benchmark: concurrent asset initialization.

Registers N independent assets and materializes them all in parallel.
Each asset does minimal work — this measures framework + DB overhead
for concurrent initialization. Turso's MVCC means parallel threads
don't block each other on DB writes.
"""

import math
import os
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed

BENCH_DIR = os.path.dirname(os.path.abspath(__file__))

if BENCH_DIR not in sys.path:
    sys.path.insert(0, BENCH_DIR)


def _gen_module(root, n_assets):
    """Generate a module with N independent @asset functions."""
    mod_path = root / "bench_project" / "_gen_concurrent.py"
    lines = ["from barca import asset\n\n"]
    for i in range(n_assets):
        lines.append(f"@asset()\ndef cinit_{i:04d}() -> dict:\n    return {{'i': {i}}}\n\n")
    mod_path.write_text("".join(lines))
    return mod_path


def bench(runs, n_assets, max_workers):
    from pathlib import Path

    from barca._engine import reindex, refresh, reset
    from barca._store import MetadataStore

    root = Path(BENCH_DIR)
    label = f"-j {max_workers}"

    def _store():
        return MetadataStore(os.path.join(BENCH_DIR, ".barca", "metadata.db"))

    def _reset():
        reset(root, db=True, artifacts=True)

    def _find_asset_ids(store, substring, count):
        ids = []
        for a in store.list_assets():
            if substring in a.logical_name:
                ids.append(a.asset_id)
        if len(ids) < count:
            raise RuntimeError(f"Expected {count} assets matching '{substring}', found {len(ids)}")
        return ids

    def _refresh_one(asset_id):
        """Each thread gets its own store (own connection) for true concurrency."""
        thread_store = _store()
        refresh(thread_store, root, asset_id)

    mod_path = _gen_module(root, n_assets)

    # Warmup
    _reset()
    store = _store()
    reindex(store, root)

    print(f"[barca in-process {label}] {n_assets} independent assets ({runs} runs):")
    times_reindex = []
    times_materialize = []
    times_total = []

    for i in range(runs):
        _reset()
        # Clear cached module so reindex re-imports it
        for key in list(sys.modules):
            if key.startswith("bench_project"):
                del sys.modules[key]

        t0 = time.perf_counter()
        store = _store()
        reindex(store, root)
        t_reindex = time.perf_counter() - t0

        asset_ids = _find_asset_ids(store, "cinit_", n_assets)

        t1 = time.perf_counter()
        with ThreadPoolExecutor(max_workers=max_workers) as pool:
            futures = [pool.submit(_refresh_one, aid) for aid in asset_ids]
            for f in as_completed(futures):
                f.result()
        t_materialize = time.perf_counter() - t1

        total = time.perf_counter() - t0
        times_reindex.append(t_reindex)
        times_materialize.append(t_materialize)
        times_total.append(total)
        print(f"  Run {i+1}: reindex={t_reindex:.3f}s  materialize={t_materialize:.3f}s  total={total:.3f}s")

    def stats(ts):
        avg = sum(ts) / len(ts)
        std = math.sqrt(sum((t - avg) ** 2 for t in ts) / len(ts))
        return avg, std

    ri_avg, ri_std = stats(times_reindex)
    mat_avg, mat_std = stats(times_materialize)
    tot_avg, tot_std = stats(times_total)
    print(f"  Reindex avg:      {ri_avg:.3f}s +/- {ri_std:.3f}s")
    print(f"  Materialize avg:  {mat_avg:.3f}s +/- {mat_std:.3f}s")
    print(f"  Total avg:        {tot_avg:.3f}s +/- {tot_std:.3f}s")

    # Cleanup generated module
    mod_path.unlink(missing_ok=True)
    _reset()


if __name__ == "__main__":
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    n_assets = int(sys.argv[2]) if len(sys.argv) > 2 else 200
    max_workers = int(sys.argv[3]) if len(sys.argv) > 3 else os.cpu_count()
    bench(runs, n_assets, max_workers)
