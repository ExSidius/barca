"""Prefect scheduler benchmark — trigger-latency probe (1-minute cron).

`probe_flow.serve(cron="* * * * *")` runs a long-lived single process that both
schedules and executes; each tick runs the flow, which appends the wall-clock
time it actually executed to `$SCHED_RESULTS`.

Fairness note: 1 minute is Prefect's finest cron granularity, and its runner
polls the schedule on a ~10-15s interval — see this benchmark's README. This is
the single-process `.serve()` model (its own ephemeral SQLite-backed API), the
closest analog to `barca serve`.
"""

import os
import time

from prefect import flow


@flow(log_prints=False)
def probe_flow():
    path = os.environ.get("SCHED_RESULTS")
    if path:
        with open(path, "a") as f:
            f.write(f"{time.time()}\n")


if __name__ == "__main__":
    probe_flow.serve(name="probe", cron="* * * * *")
