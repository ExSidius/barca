"""Barca scheduler benchmark — sub-minute cadence probe (barca-only capability).

Dagster and Prefect floor cron at **1 minute**. Barca accepts a **6-field** cron
with a leading seconds field and evaluates the schedule at 1-second resolution —
so this task fires every **2 seconds**, a cadence the other two frameworks cannot
express at all. `bench.sh` counts fires in a fixed window to report the achievable
cadence; this file is also the fast CI smoke served by `barca/run.sh`.

Each fire appends the wall-clock time to `$SCHED_RESULTS`.
"""

import os
import time

from barca import Schedule, task


@task(freshness=Schedule("*/2 * * * * *"))
def probe() -> None:
    path = os.environ.get("SCHED_RESULTS")
    if path:
        with open(path, "a") as f:
            f.write(f"{time.time()}\n")
