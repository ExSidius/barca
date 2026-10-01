"""Dagster scheduler benchmark — idle-footprint fixture (10 far-future schedules).

Ten `ScheduleDefinition`s pinned to `0 0 1 1 *` (00:00 Jan 1): registered and
tracked by the daemon but never firing during the measurement window, so the
reading reflects the daemon's *idle* footprint holding N jobs, not execution.
"""

from dagster import (
    DefaultScheduleStatus,
    Definitions,
    ScheduleDefinition,
    job,
    op,
)


@op
def noop():
    pass


@job
def idle_job():
    noop()


idle_schedules = [
    ScheduleDefinition(
        name=f"idle_{i:02d}",
        job=idle_job,
        cron_schedule="0 0 1 1 *",
        default_status=DefaultScheduleStatus.RUNNING,
    )
    for i in range(10)
]

defs = Definitions(jobs=[idle_job], schedules=idle_schedules)
