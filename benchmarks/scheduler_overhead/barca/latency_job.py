"""Barca scheduler benchmark — trigger-latency probe.

A single `@task` on a **1-minute** cron. One minute is the finest cadence that
Dagster and Prefect also support, so pinning all three frameworks to `* * * * *`
keeps the trigger-latency comparison apples-to-apples (see this benchmark's
README for the fairness rationale; barca's sub-minute capability is measured
separately by `cadence_job.py`).

Each time the scheduler fires this task, its body appends the wall-clock time it
actually began executing to the file named by `$SCHED_RESULTS`. `bench.sh`
derives per-fire latency as (execution time) − (the minute boundary the tick was
due at).
"""

import os
import time

from barca import Schedule, task


@task(freshness=Schedule("* * * * *"))
def probe() -> None:
    path = os.environ.get("SCHED_RESULTS")
    if path:
        with open(path, "a") as f:
            f.write(f"{time.time()}\n")
