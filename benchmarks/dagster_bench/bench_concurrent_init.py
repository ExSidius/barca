"""Dagster benchmark: concurrent asset initialization.

Registers N independent assets and materializes them all.
Dagster uses sequential in-process execution by default.
"""

import math
import sys
import time

from dagster import asset, materialize


def make_assets(n):
    assets = []
    for i in range(n):
        @asset(name=f"cinit_{i:04d}")
        def _asset(*, _i=i):
            return {"i": _i}
        assets.append(_asset)
    return assets


if __name__ == "__main__":
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    n_assets = int(sys.argv[2]) if len(sys.argv) > 2 else 200

    assets = make_assets(n_assets)

    # Warmup
    materialize(assets)

    print(f"[dagster] {n_assets} independent assets ({runs} runs):")
    times = []
    for i in range(runs):
        t0 = time.perf_counter()
        result = materialize(assets)
        elapsed = time.perf_counter() - t0
        n_success = len([e for e in result.all_events if e.is_step_success])
        times.append(elapsed)
        print(f"  Run {i+1}: {elapsed:.3f}s ({n_success} materialized)")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"  Total avg:  {avg:.3f}s +/- {std:.3f}s")
