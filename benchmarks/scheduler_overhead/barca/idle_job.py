"""Barca scheduler benchmark — idle-footprint fixture.

Ten scheduled tasks pinned to a cron far in the future (midnight on Jan 1), so
they are parsed, registered, and tracked by the scheduler but never fire during
the measurement window. This isolates the daemon's *idle* resource footprint
(the scheduler holding N jobs and ticking once a second) from any execution cost.

NOTE: barca discovers schedules by static analysis, so the cron must be an inline
`Schedule("...")` string literal on each task — a shared variable would not be
recognized and the task would silently fall back to `Always`.
"""

from barca import Schedule, task


@task(freshness=Schedule("0 0 1 1 *"))
def idle_00() -> None: ...


@task(freshness=Schedule("0 0 1 1 *"))
def idle_01() -> None: ...


@task(freshness=Schedule("0 0 1 1 *"))
def idle_02() -> None: ...


@task(freshness=Schedule("0 0 1 1 *"))
def idle_03() -> None: ...


@task(freshness=Schedule("0 0 1 1 *"))
def idle_04() -> None: ...


@task(freshness=Schedule("0 0 1 1 *"))
def idle_05() -> None: ...


@task(freshness=Schedule("0 0 1 1 *"))
def idle_06() -> None: ...


@task(freshness=Schedule("0 0 1 1 *"))
def idle_07() -> None: ...


@task(freshness=Schedule("0 0 1 1 *"))
def idle_08() -> None: ...


@task(freshness=Schedule("0 0 1 1 *"))
def idle_09() -> None: ...
