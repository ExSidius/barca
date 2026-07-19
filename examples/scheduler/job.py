"""Barca as a plain task scheduler.

Decorate a function with a cron `Schedule` and leave `barca serve` running —
each job fires on its own cron tick. No external cron, no DSL, no daemon service
to install. `@task` is the right decorator when the point is the *side effect*
(hit an API, rebuild a file, send a report): a task always re-runs when its tick
fires.

    barca serve job.py

Barca uses standard 5-field cron (`minute hour day-of-month month day-of-week`)
and also accepts a 6-field form with a leading *seconds* field for sub-minute
schedules — the scheduler evaluates at 1-second resolution.
"""

from datetime import datetime, timezone

from barca import task, Schedule


@task(freshness=Schedule("*/10 * * * *"))
def refresh() -> None:
    """Every 10 minutes — the classic 'keep something fresh' job."""
    print("refresh ran at", datetime.now(timezone.utc).isoformat())


@task(freshness=Schedule("*/15 * * * * *"))
def heartbeat() -> None:
    """Every 15 seconds — a sub-minute job (6-field cron, leading seconds)."""
    print("heartbeat at", datetime.now(timezone.utc).isoformat())
