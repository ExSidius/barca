"""Freshness primitives — Always, Manual, Schedule.

Freshness is the core primitive that declares how eagerly Barca keeps an
asset's output up to date during ``barca run``. It replaces the older
``schedule`` string/CronSchedule model.

The three variants are:

- ``Always()``   — Barca keeps the asset fresh automatically; any upstream
                   change cascades through and re-materialises it. Default
                   for ``@asset`` and ``@effect``.
- ``Manual()``   — Barca never auto-updates the asset, even when stale.
                   Downstream ``Always`` assets are blocked while the manual
                   upstream is stale. Only refreshed via explicit
                   ``barca assets refresh``.
- ``Schedule(cron_expression)`` — Barca refreshes on a cron cadence.
                   Acceptable staleness between ticks.

``cron(expr)`` is a friendly factory that returns a ``Schedule`` instance.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from croniter import croniter


class Freshness:
    """Base class for freshness declarations."""

    def __eq__(self, other: object) -> bool:
        return type(self) is type(other)

    def __hash__(self) -> int:
        return hash(type(self).__name__)


@dataclass(frozen=True, eq=False)
class Always(Freshness):
    """Auto-propagate upstream changes. The default for ``@asset``/``@effect``."""

    def __repr__(self) -> str:
        return "Always()"


@dataclass(frozen=True, eq=False)
class Manual(Freshness):
    """Never auto-update. Downstream Always assets are blocked while stale."""

    def __repr__(self) -> str:
        return "Manual()"


@dataclass(frozen=True, eq=False)
class Schedule(Freshness):
    """Refresh on a cron cadence."""

    cron_expression: str = field()

    def __post_init__(self) -> None:
        if not isinstance(self.cron_expression, str) or not self.cron_expression:
            raise ValueError("Schedule requires a non-empty cron expression")
        # Eager validation — mirror the old CronSchedule contract.
        croniter(self.cron_expression)

    def __eq__(self, other: object) -> bool:
        return isinstance(other, Schedule) and other.cron_expression == self.cron_expression

    def __hash__(self) -> int:
        return hash(("Schedule", self.cron_expression))

    def __repr__(self) -> str:
        return f"Schedule({self.cron_expression!r})"


def cron(expr: str) -> Schedule:
    """Create a ``Schedule`` from a cron expression like ``'0 5 * * *'``."""
    return Schedule(expr)


def is_eligible(freshness: Freshness, last_run_ts: float | None, now_ts: float) -> bool:
    """Should this node run right now given its freshness declaration?

    - ``Always``    — always eligible.
    - ``Manual``    — never eligible (only via explicit refresh).
    - ``Schedule``  — eligible if a cron tick has elapsed between
                      ``last_run_ts`` and ``now_ts``.
    """
    if isinstance(freshness, Always):
        return True
    if isinstance(freshness, Manual):
        return False
    if isinstance(freshness, Schedule):
        base = last_run_ts if last_run_ts is not None else 0.0
        it = croniter(freshness.cron_expression, base)
        next_tick = it.get_next(float)
        return next_tick <= now_ts
    raise TypeError(f"unknown freshness type: {type(freshness).__name__}")


def serialize(freshness: Freshness) -> str:
    """Serialize a Freshness to a string for DB storage."""
    if isinstance(freshness, Always):
        return "always"
    if isinstance(freshness, Manual):
        return "manual"
    if isinstance(freshness, Schedule):
        return f"schedule:{freshness.cron_expression}"
    raise TypeError(f"unknown freshness type: {type(freshness).__name__}")


def deserialize(value: str) -> Freshness:
    """Deserialize a Freshness from DB storage."""
    if value == "always":
        return Always()
    if value == "manual":
        return Manual()
    if value.startswith("schedule:"):
        return Schedule(value[len("schedule:") :])
    raise ValueError(f"invalid freshness value: {value!r}")
