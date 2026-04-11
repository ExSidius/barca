"""@sensor decorator — registers external-state observers.

Sensors must declare their ``freshness`` explicitly (no default). They
accept only ``Manual`` or ``Schedule(...)`` — ``Always`` is forbidden per
spec invariant ``SensorFreshnessIsNotAlways``.

A sensor is a source node (no inputs). It must return a 2-tuple
``(update_detected: bool, output: Any)``. The full tuple is passed as
input to downstream assets, so downstream code can inspect
``update_detected`` before doing expensive work.
"""

from __future__ import annotations

from typing import Any

from barca._freshness import Always, Freshness
from barca._freshness import serialize as serialize_freshness


class SensorWrapper:
    """Wraps a ``@sensor``-decorated function with metadata."""

    __barca_kind__ = "sensor"

    def __init__(
        self,
        original,
        *,
        name: str | None = None,
        freshness: Freshness,
        description: str | None = None,
        tags: list[str] | None = None,
    ):
        self._original = original
        self._name = name
        self._freshness = freshness
        self._description = description
        self._tags = tags or []
        self.__name__ = original.__name__
        self.__doc__ = original.__doc__
        self.__module__ = original.__module__
        self.__qualname__ = original.__qualname__
        self.__wrapped__ = original

    def __call__(self, *args, **kwargs):
        return self._original(*args, **kwargs)

    @property
    def __barca_metadata__(self) -> dict[str, Any]:
        return {
            "kind": "sensor",
            "name": self._name,
            "freshness": serialize_freshness(self._freshness),
            "description": self._description,
            "tags": self._tags,
            "inputs": None,
            "partitions": None,
            "serializer": None,
        }

    @property
    def __barca_original__(self):
        return self._original

    def __get__(self, obj, objtype=None):
        return self


def _validate_sensor_freshness(freshness: Freshness | None) -> Freshness:
    """Enforce spec invariant SensorFreshnessIsNotAlways at decoration time."""
    if freshness is None:
        raise TypeError("@sensor requires an explicit freshness= argument. Sensors poll external state — the polling frequency must be declared via Manual() or Schedule(cron_expression).")
    if isinstance(freshness, Always):
        raise TypeError("@sensor does not accept freshness=Always(). Sensors must use Manual() or Schedule(cron_expression).")
    return freshness


def sensor(
    func=None,
    *,
    name: str | None = None,
    freshness: Freshness | None = None,
    description: str | None = None,
    tags: list[str] | None = None,
    inputs: Any = None,  # caught and rejected — sensors are source nodes
):
    """Decorator that registers a Python function as a Barca sensor.

    A sensor observes external state and returns
    ``(update_detected: bool, output)``. Sensors are source nodes — they
    have no inputs — and must declare an explicit ``freshness=`` argument.

    Example::

        @sensor(freshness=Schedule("*/5 * * * *"))
        def file_watcher():
            return (True, {"path": "/tmp/data.csv"})
    """
    if inputs is not None:
        raise TypeError("@sensor does not accept an inputs= argument. Sensors are source nodes — they observe external state directly. (Invariant: SensorsAreSourceNodes)")

    # Reject bare @sensor (no parens) because we require explicit freshness
    if callable(func) and freshness is None:
        raise TypeError('@sensor requires an explicit freshness= argument. Use @sensor(freshness=Schedule("*/5 * * * *")) or @sensor(freshness=Manual()).')

    def _make_wrapper(fn):
        validated = _validate_sensor_freshness(freshness)
        return SensorWrapper(
            fn,
            name=name,
            freshness=validated,
            description=description,
            tags=tags,
        )

    if func is not None:
        return _make_wrapper(func)
    return _make_wrapper
