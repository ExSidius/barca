"""@asset decorator — registers Python functions as Barca assets."""

from __future__ import annotations

import inspect
import json
from pathlib import Path
from typing import Any

from barca._collect import CollectInput
from barca._freshness import Always, Freshness
from barca._freshness import serialize as serialize_freshness
from barca._sink import SinkSpec


class Partitions:
    """Marker type for static partition declarations."""

    def __init__(self, values: list):
        self.values = values

    def __repr__(self) -> str:
        return f"Partitions({len(self.values)} values)"


def partitions(values: list) -> Partitions:
    """Declare a static partition universe for use with ``@asset(partitions=...)``."""
    return Partitions(values)


def _resolve_input_ref(value: Any) -> str | dict:
    """Resolve an input value to a canonical asset ref or structured spec.

    Returns either:
      - A string ref ``"{abs_path}:{func_name}"`` for a plain upstream
      - A dict ``{"kind": "collect", "ref": "..."}`` for a ``collect()`` marker

    Raises ``TypeError`` for effects/sinks (leaf nodes) or any other value.
    """
    if isinstance(value, CollectInput):
        inner_ref = _resolve_input_ref(value.upstream)
        return {"kind": "collect", "ref": inner_ref}

    kind = getattr(value, "__barca_kind__", None)
    if kind == "effect":
        raise TypeError("effects cannot be used as inputs — they are leaf nodes. Pass an @asset or @sensor function instead.")
    if kind == "sink":
        raise TypeError("sinks cannot be used as inputs — they are leaf nodes.")
    if kind in ("asset", "sensor"):
        original = getattr(value, "__barca_original__", value)
        source_file = inspect.getsourcefile(original)
        file_path = str(Path(source_file).resolve()) if source_file else ""
        func_name = original.__name__
        return f"{file_path}:{func_name}"

    if isinstance(value, str):
        return value

    raise TypeError("inputs values must be @asset or @sensor decorated functions, collect(asset), or ref strings")


def _resolve_partition_spec(dim_name: str, spec: Any) -> dict:
    """Resolve a partition spec to a serializable dict.

    Static: ``partitions([...])`` → ``{"kind": "inline", "values_json": ...}``
    Dynamic: an ``@asset`` function → ``{"kind": "dynamic", "upstream_ref": ...}``
    """
    if isinstance(spec, Partitions):
        return {
            "kind": "inline",
            "values_json": json.dumps(spec.values),
        }
    # Dynamic: the partition values come from an upstream asset's output
    kind = getattr(spec, "__barca_kind__", None)
    if kind == "asset":
        original = getattr(spec, "__barca_original__", spec)
        source_file = inspect.getsourcefile(original)
        file_path = str(Path(source_file).resolve()) if source_file else ""
        func_name = original.__name__
        return {
            "kind": "dynamic",
            "upstream_ref": f"{file_path}:{func_name}",
        }
    raise TypeError(f"partition dimension {dim_name!r}: values must be partitions([...]) or a decorated @asset function")


class AssetWrapper:
    """Wraps an ``@asset``-decorated function with metadata."""

    __barca_kind__ = "asset"

    def __init__(
        self,
        original,
        *,
        name: str | None = None,
        inputs: dict[str, Any] | None = None,
        partitions: dict[str, dict] | None = None,
        serializer: str | None = None,
        freshness: Freshness | None = None,
        sinks: list[SinkSpec] | None = None,
    ):
        self._original = original
        self._name = name
        self._inputs = inputs
        self._partitions = partitions
        self._serializer = serializer
        self._freshness = freshness if freshness is not None else Always()
        self._sinks = sinks or []
        # Forward dunder attributes
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
            "freshness": serialize_freshness(self._freshness),
            "sinks": [{"path": s.path, "serializer": s.serializer} for s in self._sinks],
        }
        return meta

    @property
    def __barca_original__(self):
        return self._original

    @property
    def __barca_sinks__(self) -> list[SinkSpec]:
        return self._sinks

    def __get__(self, obj, objtype=None):
        return self


def asset(
    func=None,
    *,
    name: str | None = None,
    inputs: dict[str, Any] | None = None,
    partitions: dict[str, Any] | None = None,
    serializer: str | None = None,
    freshness: Freshness | None = None,
):
    """Decorator that registers a Python function as a Barca asset.

    Example::

        @asset(freshness=Always())
        def my_asset():
            return {"v": 1}

        @asset(inputs={"u": upstream}, freshness=Manual())
        def downstream(u):
            return u["v"] + 1
    """
    # Resolve inputs eagerly at decoration time
    resolved_inputs: dict[str, Any] | None = None
    if inputs is not None:
        resolved_inputs = {}
        for param_name, ref_value in inputs.items():
            resolved_inputs[param_name] = _resolve_input_ref(ref_value)

    # Resolve partitions eagerly at decoration time
    resolved_partitions: dict[str, dict] | None = None
    if partitions is not None:
        resolved_partitions = {}
        for dim_name, spec in partitions.items():
            resolved_partitions[dim_name] = _resolve_partition_spec(dim_name, spec)

    def _make_wrapper(fn):
        # Harvest any @sink specs that were stacked below this @asset.
        # @sink attaches `__barca_sinks__` as a list; we pull it off and
        # store it on the AssetWrapper.
        harvested_sinks: list[SinkSpec] = []
        raw_sinks = getattr(fn, "__barca_sinks__", None)
        if raw_sinks:
            harvested_sinks = list(raw_sinks)
            # Remove the marker from the raw function so it doesn't leak.
            try:
                delattr(fn, "__barca_sinks__")
            except AttributeError:
                pass

        return AssetWrapper(
            fn,
            name=name,
            inputs=resolved_inputs,
            partitions=resolved_partitions,
            serializer=serializer,
            freshness=freshness,
            sinks=harvested_sinks,
        )

    # @asset (no parentheses): func is the decorated function directly
    if func is not None:
        return _make_wrapper(func)

    # @asset() or @asset(name=...) etc.: return a decorator
    return _make_wrapper
