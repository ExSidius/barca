"""@sensor decorator — registers external-state observers."""

from __future__ import annotations

import inspect
from pathlib import Path
from typing import Any

from barca._schedule import Schedule, serialize_schedule


class SensorWrapper:
    """Wraps a @sensor-decorated function with metadata."""

    __barca_kind__ = "sensor"

    def __init__(
        self,
        original,
        *,
        name: str | None = None,
        schedule: Schedule = "always",
        description: str | None = None,
        tags: list[str] | None = None,
    ):
        self._original = original
        self._name = name
        self._schedule = schedule
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
            "schedule": serialize_schedule(self._schedule),
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


def sensor(func=None, *, name=None, schedule: Schedule = "always", description=None, tags=None):
    """Decorator that registers a Python function as a barca sensor.

    A sensor observes external state and returns (update_detected: bool, output).
    Sensors are source nodes — they have no inputs.
    """
    if func is not None:
        return SensorWrapper(func, name=name, schedule=schedule, description=description, tags=tags)

    def decorator(f):
        return SensorWrapper(f, name=name, schedule=schedule, description=description, tags=tags)

    return decorator
