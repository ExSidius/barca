"""Prefect server-mode: 500 tasks x 50ms with ConcurrentTaskRunner."""

import time

from prefect import flow, task
from prefect.task_runners import ConcurrentTaskRunner


@task
def work_item(i: int) -> dict:
    time.sleep(0.05)
    return {"i": i, "status": "ok"}


@flow(task_runner=ConcurrentTaskRunner(max_workers=16))
def fan_out_50ms_flow():
    futures = [work_item.submit(i) for i in range(500)]
    return {"count": len(futures)}
