"""Prefect cold start benchmark: time to execute a single trivial task.

Note: Run 1 includes Prefect's internal server startup (~6s).
Subsequent runs reuse the running server.
"""

import time
import sys
import math

from prefect import flow, task
from prefect.task_runners import ThreadPoolTaskRunner


@task
def trivial_task() -> dict:
    return {"status": "ok"}


@flow(task_runner=ThreadPoolTaskRunner(max_workers=1))
def single_task_flow():
    future = trivial_task.submit()
    return future.result()


def main():
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 5

    # Warm up (Prefect starts an internal API server on first call)
    single_task_flow()

    print(f"[prefect] Cold start benchmark ({runs} runs, server warm):")
    times = []
    for i in range(runs):
        t0 = time.perf_counter()
        single_task_flow()
        elapsed = time.perf_counter() - t0
        times.append(elapsed)
        print(f"  Run {i+1}: {elapsed:.3f}s")
    avg = sum(times) / len(times)
    std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
    print(f"[prefect] Cold start avg: {avg:.3f}s +/- {std:.3f}s (warm server)")


if __name__ == "__main__":
    main()
