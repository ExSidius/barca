"""Barca benchmark: 500 trivial partitions (zero work)."""

import subprocess
import time
import os
import math

BENCH_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(os.path.dirname(BENCH_DIR))
CLI = os.path.join(REPO_ROOT, "target", "release", "barca")
ENV = {**os.environ, "RUST_LOG": "error"}


def run(args, **kwargs):
    return subprocess.run(args, cwd=BENCH_DIR, check=True, env=ENV, **kwargs)


def find_asset_id(name):
    result = run([CLI, "assets", "list"], capture_output=True, text=True)
    for line in result.stdout.splitlines():
        if name in line:
            for part in line.split():
                if part.isdigit():
                    return int(part)
    raise RuntimeError(f"Asset '{name}' not found")


if __name__ == "__main__":
    runs = 3
    times = []
    for i in range(runs):
        run([CLI, "reset", "--db", "--artifacts"], capture_output=True)
        run([CLI, "reindex"], capture_output=True)
        asset_id = find_asset_id("trivial_500")

        t0 = time.perf_counter()
        subprocess.run([CLI, "assets", "refresh", str(asset_id)], cwd=BENCH_DIR,
                       check=True, capture_output=True, env={**os.environ, "RUST_LOG": "warn"})
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i+1}: {elapsed:.2f}s")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"\n[barca] 500 trivial partitions: {avg:.2f}s +/- {std:.2f}s")
