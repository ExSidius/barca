"""Schedule primitives — cron, always, manual."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Literal, Union, cast

from croniter import croniter


@dataclass(frozen=True)
class CronSchedule:
    """A cron-expression schedule."""

    expr: str

    def __post_init__(self):
        # Validate the expression eagerly
        croniter(self.expr)


Schedule = Union[Literal["manual", "always"], CronSchedule]


def cron(expr: str) -> CronSchedule:
    """Create a cron schedule from an expression like '0 5 * * *'."""
    return CronSchedule(expr)


def is_schedule_eligible(schedule: Schedule, last_run_ts: float | None, now_ts: float) -> bool:
    """Should this node run right now given its schedule?

    - manual: never eligible (only via explicit refresh)
    - always: eligible whenever called
    - cron: eligible if a tick has occurred between last_run_ts and now_ts
    """
    if schedule == "manual":
        return False
    if schedule == "always":
        return True
    if isinstance(schedule, CronSchedule):
        base = last_run_ts if last_run_ts is not None else 0.0
        it = croniter(schedule.expr, base)
        next_tick = it.get_next(float)
        return next_tick <= now_ts
    return False


def serialize_schedule(schedule: Schedule) -> str:
    """Serialize a Schedule to a string for DB storage."""
    if isinstance(schedule, CronSchedule):
        return f"cron:{schedule.expr}"
    return schedule  # "manual" or "always"


def deserialize_schedule(value: str) -> Schedule:
    """Deserialize a Schedule from DB storage."""
    if value.startswith("cron:"):
        return CronSchedule(value[5:])
    if value in ("manual", "always"):
        return cast("Schedule", value)
    raise ValueError(f"invalid schedule value: {value!r}")
