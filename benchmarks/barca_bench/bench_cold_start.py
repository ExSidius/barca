"""Barca cold start benchmark: time to materialize a single trivial asset from scratch."""

import subprocess
import time
import os
import sys
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


def cold_start_single():
    run([CLI, "reset", "--db", "--artifacts"], capture_output=True)
    t0 = time.perf_counter()
    run([CLI, "reindex"], capture_output=True)
    asset_id = find_asset_id("single_asset")
    run([CLI, "assets", "refresh", str(asset_id)], capture_output=True)
    return time.perf_counter() - t0


def main():
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 5
    print(f"[barca] Cold start: single asset ({runs} runs):")
    times = []
    for i in range(runs):
        t = cold_start_single()
        times.append(t)
        print(f"  Run {i+1}: {t:.3f}s")
    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"[barca] Cold start avg: {avg:.3f}s +/- {std:.3f}s")


if __name__ == "__main__":
    main()
