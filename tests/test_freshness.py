"""Tests for the freshness primitive (replaces test_schedule.py).

These tests cover the `_freshness` module:
- Always / Manual / Schedule class construction and equality
- is_eligible behaviour for each kind
- serialize / deserialize round-trip
- Invalid cron expressions rejected eagerly
- cron() helper factory

They do NOT touch assets, the store, or the engine — those are integration
tested elsewhere.
"""

from __future__ import annotations

import pytest

from barca._freshness import (
    Always,
    Freshness,
    Manual,
    Schedule,
    cron,
    deserialize,
    is_eligible,
    serialize,
)

# ---------------------------------------------------------------------------
# Construction & equality
# ---------------------------------------------------------------------------


def test_always_is_freshness_instance():
    assert isinstance(Always(), Freshness)


def test_manual_is_freshness_instance():
    assert isinstance(Manual(), Freshness)


def test_schedule_is_freshness_instance():
    assert isinstance(Schedule("0 5 * * *"), Freshness)


def test_always_equality():
    assert Always() == Always()


def test_manual_equality():
    assert Manual() == Manual()


def test_always_not_equal_manual():
    assert Always() != Manual()


def test_schedule_equality_by_expression():
    assert Schedule("0 5 * * *") == Schedule("0 5 * * *")
    assert Schedule("0 5 * * *") != Schedule("0 6 * * *")


def test_schedule_not_equal_always():
    assert Schedule("* * * * *") != Always()


def test_schedule_is_hashable():
    # Must be usable as a dict key / in sets
    s1 = Schedule("0 5 * * *")
    s2 = Schedule("0 5 * * *")
    assert hash(s1) == hash(s2)
    assert {s1, s2} == {s1}


# ---------------------------------------------------------------------------
# Schedule validation
# ---------------------------------------------------------------------------


def test_schedule_requires_non_empty_expression():
    with pytest.raises((ValueError, TypeError)):
        Schedule("")


def test_schedule_rejects_invalid_cron_expression():
    # croniter raises its own exception type (CroniterBadCronError), but we
    # catch ValueError since that's the parent class it inherits from
    with pytest.raises(Exception):  # noqa: B017 - croniter raises a specific subclass
        Schedule("not a cron expression")


def test_schedule_accepts_standard_cron():
    Schedule("0 5 * * *")  # daily at 5am
    Schedule("* * * * *")  # every minute
    Schedule("*/15 * * * *")  # every 15 minutes


# ---------------------------------------------------------------------------
# is_eligible
# ---------------------------------------------------------------------------


def test_always_always_eligible():
    assert is_eligible(Always(), None, 0.0)
    assert is_eligible(Always(), 100.0, 200.0)
    assert is_eligible(Always(), 1e18, 1e18)


def test_manual_never_eligible():
    assert not is_eligible(Manual(), None, 0.0)
    assert not is_eligible(Manual(), 100.0, 9_999_999_999.0)


def test_schedule_eligible_after_tick_elapsed():
    # every-minute schedule: last=0, now=61 → a tick at 60 is between
    assert is_eligible(Schedule("* * * * *"), 0.0, 61.0)


def test_schedule_not_eligible_before_tick():
    # daily at 5am: last=1000000, now=1000001 (1s later) → no tick in between
    assert not is_eligible(Schedule("0 5 * * *"), 1_000_000.0, 1_000_001.0)


def test_schedule_eligible_with_none_last_run():
    # First-ever run: last_run_ts is None → base=0, any future tick counts
    assert is_eligible(Schedule("* * * * *"), None, 120.0)


# ---------------------------------------------------------------------------
# serialize / deserialize
# ---------------------------------------------------------------------------


def test_serialize_always():
    assert serialize(Always()) == "always"


def test_serialize_manual():
    assert serialize(Manual()) == "manual"


def test_serialize_schedule():
    assert serialize(Schedule("0 5 * * *")) == "schedule:0 5 * * *"


def test_deserialize_always():
    assert deserialize("always") == Always()


def test_deserialize_manual():
    assert deserialize("manual") == Manual()


def test_deserialize_schedule():
    deserialized = deserialize("schedule:0 5 * * *")
    assert isinstance(deserialized, Schedule)
    assert deserialized.cron_expression == "0 5 * * *"


def test_deserialize_rejects_invalid_value():
    with pytest.raises(ValueError):
        deserialize("bogus")


def test_round_trip_always():
    f = Always()
    assert deserialize(serialize(f)) == f


def test_round_trip_manual():
    f = Manual()
    assert deserialize(serialize(f)) == f


def test_round_trip_schedule():
    f = Schedule("*/15 * * * *")
    assert deserialize(serialize(f)) == f


# ---------------------------------------------------------------------------
# cron() helper
# ---------------------------------------------------------------------------


def test_cron_helper_returns_schedule():
    s = cron("0 5 * * *")
    assert isinstance(s, Schedule)
    assert s.cron_expression == "0 5 * * *"


def test_cron_helper_equivalent_to_schedule_constructor():
    assert cron("0 5 * * *") == Schedule("0 5 * * *")
