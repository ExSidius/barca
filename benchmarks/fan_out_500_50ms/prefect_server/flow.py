"""Prefect server-mode: 500 tasks x 50ms with ConcurrentTaskRunner."""

import os
import time

from prefect import flow, task
from prefect.task_runners import ConcurrentTaskRunner

# Matches barca's pool_size and dagster's max_concurrent for this benchmark run
# (see benchmarks/lib/env.sh) so no framework gets more/fewer workers than another.
BENCH_WORKERS = int(os.environ.get("BARCA_BENCH_WORKERS", "16"))


@task
def work_item(i: int) -> dict:
    time.sleep(0.05)
    return {"i": i, "status": "ok"}


@flow(task_runner=ConcurrentTaskRunner(max_workers=BENCH_WORKERS))
def fan_out_50ms_flow():
    futures = [work_item.submit(i) for i in range(500)]
    return {"count": len(futures)}
