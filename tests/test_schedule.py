"""Tests for schedule primitives."""

from barca._schedule import (
    CronSchedule,
    cron,
    deserialize_schedule,
    is_schedule_eligible,
    serialize_schedule,
)


def test_cron_creates_cron_schedule():
    s = cron("0 5 * * *")
    assert isinstance(s, CronSchedule)
    assert s.expr == "0 5 * * *"


def test_manual_never_eligible():
    assert not is_schedule_eligible("manual", None, 9999999999.0)
    assert not is_schedule_eligible("manual", 100.0, 9999999999.0)


def test_always_always_eligible():
    assert is_schedule_eligible("always", None, 0.0)
    assert is_schedule_eligible("always", 100.0, 200.0)


def test_cron_eligible_when_tick_passed():
    # Cron: every minute
    s = cron("* * * * *")
    # last_run at t=0, now at t=61 → tick at t=60 is between
    assert is_schedule_eligible(s, 0.0, 61.0)


def test_cron_not_eligible_before_tick():
    s = cron("0 5 * * *")  # daily at 5am
    # last_run at t=1000000, now at t=1000001 → no tick in 1 second
    assert not is_schedule_eligible(s, 1000000.0, 1000001.0)


def test_serialize_deserialize_roundtrip():
    for schedule in ["manual", "always", cron("0 5 * * *")]:
        serialized = serialize_schedule(schedule)
        deserialized = deserialize_schedule(serialized)
        if isinstance(schedule, CronSchedule):
            assert isinstance(deserialized, CronSchedule)
            assert deserialized.expr == schedule.expr
        else:
            assert deserialized == schedule
