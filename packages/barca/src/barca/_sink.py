"""@sink — declarative output destinations stacked on ``@asset``.

A ``@sink(path, serializer=...)`` decorator declares that when its parent
``@asset`` materialises, its output should also be written to ``path``.
Multiple sinks may be stacked on a single asset.

Paths are fsspec-compatible: local paths, ``s3://bucket/key``,
``gs://bucket/key``, etc. Serializers determine how the output is encoded:
``"json"`` (default), ``"parquet"``, ``"pickle"``, ``"text"``.

Sinks are leaf nodes — no other asset may depend on a sink. Sink failure is
non-blocking: the parent asset remains fresh even if the sink write fails.
Failed sinks are retried on the next ``barca run`` pass while the parent is
fresh.

This module is a **stub** during Phase 1 of the refactor — it defines the
public surface (``sink``, ``SinkSpec``) but the decorator does not yet attach
specs to asset wrappers or write files. Phase 3 wires everything together.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class SinkSpec:
    """Metadata for a single output sink attached to an asset."""

    path: str
    serializer: str = "json"


def sink(path: str, *, serializer: str = "json") -> Callable[[Any], Any]:
    """Stack on ``@asset`` to declare an output sink at ``path``.

    Example::

        @asset(freshness=Always())
        @sink("./tmp/greeting.txt", serializer="text")
        @sink("s3://my-bucket/greeting.txt", serializer="text")
        def banana():
            return {"a": 1}

    The wrapped function still returns its value normally; Barca handles
    the sink writes as part of materialisation.
    """

    def decorator(target: Any) -> Any:
        # Phase 1 stub: attach the sink spec to the target so that decorators
        # applied above (e.g. @asset) can harvest it. The real implementation
        # in Phase 3 will integrate with AssetWrapper to emit per-sink
        # InspectedAssets.
        spec = SinkSpec(path=path, serializer=serializer)
        existing = getattr(target, "__barca_sinks__", None)
        if existing is None:
            existing = []
            try:
                target.__barca_sinks__ = existing
            except AttributeError as e:
                raise TypeError("@sink must be stacked on a callable that supports attribute assignment") from e
        existing.append(spec)
        return target

    return decorator
