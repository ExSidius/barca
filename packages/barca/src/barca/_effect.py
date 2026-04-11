"""@effect decorator — standalone side-effect nodes.

Effects are leaf nodes: they take upstream inputs and produce no
meaningful output. Use them for sending emails, writing to databases,
calling external APIs — anything that has side effects but whose return
value is discarded.

``@effect`` is distinct from ``@sink``. ``@sink`` writes an asset's
output to a file (via fsspec). ``@effect`` runs arbitrary code with
arbitrary side effects.
"""

from __future__ import annotations

import inspect
from pathlib import Path
from typing import Any

from barca._collect import CollectInput
from barca._freshness import Always, Freshness
from barca._freshness import serialize as serialize_freshness


def _resolve_input_ref(value: Any) -> str | dict:
    """Resolve an input value to a canonical ref or structured spec."""
    if isinstance(value, CollectInput):
        inner = _resolve_input_ref(value.upstream)
        return {"kind": "collect", "ref": inner}

    kind = getattr(value, "__barca_kind__", None)
    if kind in ("effect", "sink"):
        raise TypeError(f"{kind}s cannot be used as inputs — they are leaf nodes.")
    if kind in ("asset", "sensor"):
        original = getattr(value, "__barca_original__", value)
        source_file = inspect.getsourcefile(original)
        file_path = str(Path(source_file).resolve()) if source_file else ""
        func_name = original.__name__
        return f"{file_path}:{func_name}"
    if isinstance(value, str):
        return value
    raise TypeError("effect inputs must be @asset or @sensor decorated functions, collect(asset), or ref strings")


class EffectWrapper:
    """Wraps an ``@effect``-decorated function with metadata."""

    __barca_kind__ = "effect"

    def __init__(
        self,
        original,
        *,
        name: str | None = None,
        inputs: dict[str, Any] | None = None,
        freshness: Freshness | None = None,
        description: str | None = None,
        tags: list[str] | None = None,
    ):
        self._original = original
        self._name = name
        self._inputs = inputs
        self._freshness = freshness if freshness is not None else Always()
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
            "freshness": serialize_freshness(self._freshness),
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


def effect(
    func=None,
    *,
    name: str | None = None,
    inputs: dict[str, Any] | None = None,
    freshness: Freshness | None = None,
    description: str | None = None,
    tags: list[str] | None = None,
    path: Any = None,  # caught and rejected — that's @sink's territory
):
    """Decorator that registers a Python function as a Barca effect.

    An effect runs side effects (email, DB write, API call) and produces
    no meaningful output. Effects are leaf nodes — nothing can depend on
    them. Default freshness is ``Always()``.

    For writing asset outputs to file paths, use ``@sink`` instead.
    """
    if path is not None:
        raise TypeError("@effect does not accept a path= argument. Use @sink(path=...) for writing asset outputs to files.")

    resolved_inputs: dict[str, Any] | None = None
    if inputs is not None:
        resolved_inputs = {}
        for param_name, ref_value in inputs.items():
            resolved_inputs[param_name] = _resolve_input_ref(ref_value)

    def _make_wrapper(fn):
        return EffectWrapper(
            fn,
            name=name,
            inputs=resolved_inputs,
            freshness=freshness,
            description=description,
            tags=tags,
        )

    if func is not None:
        return _make_wrapper(func)
    return _make_wrapper
