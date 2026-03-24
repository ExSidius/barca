"""Prefect benchmark: 500 trivial parallel tasks (no work)."""

import time
import math
import sys

from prefect import flow, task
from prefect.task_runners import ThreadPoolTaskRunner


@task
def trivial_task(i: int) -> dict:
    return {"i": i, "status": "ok"}


@flow(task_runner=ThreadPoolTaskRunner(max_workers=64))
def benchmark_flow():
    futures = [trivial_task.submit(i) for i in range(500)]
    return [f.result() for f in futures]


if __name__ == "__main__":
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 3

    # Warm up
    benchmark_flow()

    times = []
    for i in range(runs):
        t0 = time.perf_counter()
        results = benchmark_flow()
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i+1}: {elapsed:.2f}s ({len(results)} results)")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"\n[prefect] 500 trivial tasks: {avg:.2f}s +/- {std:.2f}s")
