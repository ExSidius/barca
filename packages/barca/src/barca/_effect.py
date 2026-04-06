"""@effect decorator — registers external-state side-effect nodes."""

from __future__ import annotations

import inspect
from pathlib import Path
from typing import Any

from barca._schedule import Schedule, serialize_schedule


def _resolve_input_ref(value: Any) -> str:
    """Resolve an input value to a canonical ref: '{abs_path}:{func_name}'."""
    kind = getattr(value, "__barca_kind__", None)
    if kind in ("asset", "sensor"):
        original = getattr(value, "__barca_original__", value)
        source_file = inspect.getsourcefile(original)
        file_path = str(Path(source_file).resolve()) if source_file else ""
        func_name = original.__name__
        return f"{file_path}:{func_name}"
    if isinstance(value, str):
        return value
    raise TypeError("effect inputs must be @asset or @sensor decorated functions, or ref strings")


class EffectWrapper:
    """Wraps an @effect-decorated function with metadata."""

    __barca_kind__ = "effect"

    def __init__(
        self,
        original,
        *,
        name: str | None = None,
        inputs: dict[str, str] | None = None,
        schedule: Schedule = "always",
        description: str | None = None,
        tags: list[str] | None = None,
    ):
        self._original = original
        self._name = name
        self._inputs = inputs
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
            "kind": "effect",
            "name": self._name,
            "inputs": self._inputs,
            "schedule": serialize_schedule(self._schedule),
            "description": self._description,
            "tags": self._tags,
            "partitions": None,
            "serializer": None,
        }

    @property
    def __barca_original__(self):
        return self._original

    def __get__(self, obj, objtype=None):
        return self


def effect(func=None, *, name=None, inputs=None, schedule: Schedule = "always", description=None, tags=None):
    """Decorator that registers a Python function as a barca effect.

    An effect pushes data to external systems. Effects are leaf nodes —
    nothing can depend on them. They are never cached.
    """
    resolved_inputs: dict[str, str] | None = None
    if inputs is not None:
        resolved_inputs = {}
        for param_name, ref_value in inputs.items():
            resolved_inputs[param_name] = _resolve_input_ref(ref_value)

    if func is not None:
        return EffectWrapper(func, name=name, inputs=resolved_inputs, schedule=schedule, description=description, tags=tags)

    def decorator(f):
        return EffectWrapper(f, name=name, inputs=resolved_inputs, schedule=schedule, description=description, tags=tags)

    return decorator
