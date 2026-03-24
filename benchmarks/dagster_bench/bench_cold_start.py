"""Dagster cold start benchmark: time to materialize a single trivial asset.

Note: Run 1 includes module loading overhead.
"""

import time
import sys
import math

from dagster import asset, materialize


@asset
def trivial_single() -> dict:
    return {"status": "ok"}


def main():
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 5

    # Warm up (first materialize loads Dagster internals)
    materialize([trivial_single])

    print(f"[dagster] Cold start benchmark ({runs} runs, modules warm):")
    times = []
    for i in range(runs):
        t0 = time.perf_counter()
        materialize([trivial_single])
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i+1}: {elapsed:.3f}s")
    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"[dagster] Cold start avg: {avg:.3f}s +/- {std:.3f}s (warm modules)")


if __name__ == "__main__":
    main()
