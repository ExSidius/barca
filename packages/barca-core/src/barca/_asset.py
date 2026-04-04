"""@asset decorator — registers Python functions as barca assets."""

from __future__ import annotations

import inspect
import json
from pathlib import Path
from typing import Any


class Partitions:
    """Marker type for partition declarations."""

    def __init__(self, values: list):
        self.values = values

    def __repr__(self) -> str:
        return f"Partitions({len(self.values)} values)"


def partitions(values: list) -> Partitions:
    """Declare a static partition universe for use with @asset(partitions=...)."""
    return Partitions(values)


def _resolve_input_ref(value: Any) -> str:
    """Resolve an input value to a canonical asset ref: '{abs_path}:{func_name}'."""
    kind = getattr(value, "__barca_kind__", None)
    if kind == "asset":
        original = getattr(value, "__barca_original__", value)
        source_file = inspect.getsourcefile(original)
        file_path = str(Path(source_file).resolve()) if source_file else ""
        func_name = original.__name__
        return f"{file_path}:{func_name}"

    if isinstance(value, str):
        return value

    raise TypeError("inputs values must be @asset-decorated functions or asset ref strings")


class AssetWrapper:
    """Wraps an @asset-decorated function with metadata."""

    __barca_kind__ = "asset"

    def __init__(
        self,
        original,
        *,
        name: str | None = None,
        inputs: dict[str, str] | None = None,
        partitions: dict[str, dict] | None = None,
        serializer: str | None = None,
    ):
        self._original = original
        self._name = name
        self._inputs = inputs
        self._partitions = partitions
        self._serializer = serializer
        # Forward dunder attributes (can't use properties for these)
        self.__name__ = original.__name__
        self.__doc__ = original.__doc__
        self.__module__ = original.__module__
        self.__qualname__ = original.__qualname__
        self.__wrapped__ = original

    def __call__(self, *args, **kwargs):
        return self._original(*args, **kwargs)

    @property
    def __barca_metadata__(self) -> dict:
        meta: dict[str, Any] = {
            "kind": "asset",
            "name": self._name,
            "serializer": self._serializer,
            "inputs": self._inputs,
            "partitions": self._partitions,
        }
        return meta

    @property
    def __barca_original__(self):
        return self._original

    def __get__(self, obj, objtype=None):
        return self


def asset(func=None, *, name=None, inputs=None, partitions=None, serializer=None):
    """Decorator that registers a Python function as a barca asset."""

    # Resolve inputs eagerly at decoration time
    resolved_inputs: dict[str, str] | None = None
    if inputs is not None:
        resolved_inputs = {}
        for param_name, ref_value in inputs.items():
            resolved_inputs[param_name] = _resolve_input_ref(ref_value)

    # Resolve partitions eagerly at decoration time
    resolved_partitions: dict[str, dict] | None = None
    if partitions is not None:
        resolved_partitions = {}
        for dim_name, spec in partitions.items():
            if isinstance(spec, Partitions):
                resolved_partitions[dim_name] = {
                    "kind": "inline",
                    "values_json": json.dumps(spec.values),
                }
            else:
                raise TypeError("partition values must be partitions([...]) instances")

    # @asset (no parentheses): func is the decorated function directly
    if func is not None:
        return AssetWrapper(
            func,
            name=name,
            inputs=resolved_inputs,
            partitions=resolved_partitions,
            serializer=serializer,
        )

    # @asset() or @asset(name=...) etc.: return a decorator
    def decorator(f):
        return AssetWrapper(
            f,
            name=name,
            inputs=resolved_inputs,
            partitions=resolved_partitions,
            serializer=serializer,
        )

    return decorator
