"""Dagster scheduler benchmark — trigger-latency probe (1-minute cron).

A `ScheduleDefinition` on `* * * * *` (Dagster's finest cron granularity)
launches a one-op job each tick; the op appends the wall-clock time it actually
executed to `$SCHED_RESULTS`. Served by `dagster dev`, which bundles the
schedule-ticking daemon. `default_status=RUNNING` so the schedule is live without
a UI toggle.

Fairness note: Dagster's daemon evaluates schedules on a ~30s interval and cannot
express sub-minute cron — see this benchmark's README. Pinning to 1 minute is the
finest cadence barca, Dagster, and Prefect all share.
"""

import os
import time

from dagster import (
    DefaultScheduleStatus,
    Definitions,
    ScheduleDefinition,
    job,
    op,
)


@op
def record_fire():
    path = os.environ.get("SCHED_RESULTS")
    if path:
        with open(path, "a") as f:
            f.write(f"{time.time()}\n")


@job
def probe_job():
    record_fire()


probe_schedule = ScheduleDefinition(
    name="probe_schedule",
    job=probe_job,
    cron_schedule="* * * * *",
    default_status=DefaultScheduleStatus.RUNNING,
)

defs = Definitions(jobs=[probe_job], schedules=[probe_schedule])
