"""Dagster benchmark: 500 assets, each doing 50ms of simulated work.

Uses in-process executor (Dagster's default). Runs sequentially.
"""

import math
import sys
import time

from dagster import asset, materialize


def make_asset(idx: int):
    @asset(name=f"work_{idx:04d}")
    def _asset():
        time.sleep(0.05)
        return {"i": idx, "status": "ok"}

    return _asset


assets = [make_asset(i) for i in range(500)]


if __name__ == "__main__":
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 3

    # Warm up
    materialize(assets)

    times = []
    for i in range(runs):
        t0 = time.perf_counter()
        result = materialize(assets)
        elapsed = time.perf_counter() - t0
        n_success = len([e for e in result.all_events if e.is_step_success])
        times.append(elapsed)
        print(f"  Run {i + 1}: {elapsed:.2f}s ({n_success} assets)")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"\n[dagster] 500 assets x 50ms work: {avg:.2f}s +/- {std:.2f}s")
    print("[dagster] (sequential in-process execution)")
