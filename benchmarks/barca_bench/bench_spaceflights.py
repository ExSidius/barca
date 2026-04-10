"""Barca benchmark: spaceflights pipeline (6-level diamond DAG).

Topology:
    raw_shuttles ──→ prep_shuttles ──┐
    raw_companies ─→ prep_companies ─├→ master_table → split → train → evaluate
    raw_reviews ───→ prep_reviews ──┘

Measures: fan-out/fan-in merge, deep sequential chain, sklearn compute.
"""

import math
import os
import subprocess
import sys
import time

BENCH_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(os.path.dirname(BENCH_DIR))
CLI = "barca"


def run(args, **kwargs):
    return subprocess.run(args, cwd=BENCH_DIR, check=True, **kwargs)


def find_asset_id(name):
    result = run([CLI, "assets", "list"], capture_output=True, text=True)
    for line in result.stdout.splitlines():
        if name in line:
            for part in line.split():
                if part.isdigit():
                    return int(part)
    raise RuntimeError(f"Asset '{name}' not found")


def bench(runs, concurrency=None):
    label = f"-j {concurrency}" if concurrency else "default"
    times = []
    for i in range(runs):
        run([CLI, "reset", "--db", "--artifacts"], capture_output=True)
        run([CLI, "reindex"], capture_output=True)
        asset_id = find_asset_id("evaluate")

        cmd = [CLI, "assets", "refresh", str(asset_id)]
        if concurrency is not None:
            cmd += ["-j", str(concurrency)]

        t0 = time.perf_counter()
        subprocess.run(cmd, cwd=BENCH_DIR, check=True, capture_output=True)
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i + 1}: {elapsed:.2f}s")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"\n[barca {label}] spaceflights (10 assets, 6-level DAG): {avg:.2f}s +/- {std:.2f}s")


if __name__ == "__main__":
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    concurrency = int(sys.argv[2]) if len(sys.argv) > 2 else None
    bench(runs, concurrency)
