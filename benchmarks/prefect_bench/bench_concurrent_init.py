"""Prefect benchmark: concurrent asset initialization.

Registers N independent tasks and runs them all via ThreadPoolTaskRunner.
"""

import math
import sys
import time

from prefect import flow, task
from prefect.task_runners import ThreadPoolTaskRunner


@task
def init_task(i: int) -> dict:
    return {"i": i}


def make_flow(n_assets, max_workers=64):
    @flow(task_runner=ThreadPoolTaskRunner(max_workers=max_workers))
    def init_flow():
        futures = [init_task.submit(i) for i in range(n_assets)]
        return [f.result() for f in futures]
    return init_flow


if __name__ == "__main__":
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    n_assets = int(sys.argv[2]) if len(sys.argv) > 2 else 200

    benchmark_flow = make_flow(n_assets)

    # Warmup
    benchmark_flow()

    print(f"[prefect] {n_assets} independent tasks ({runs} runs):")
    times = []
    for i in range(runs):
        t0 = time.perf_counter()
        results = benchmark_flow()
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i+1}: {elapsed:.3f}s ({len(results)} results)")

    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"  Total avg:  {avg:.3f}s +/- {std:.3f}s")
