"""collect() — aggregate all partitions of a partitioned asset into a dict.

``collect(asset_fn)`` returns a ``CollectInput`` marker that declares a
downstream asset wants the full partition set of ``asset_fn`` as a single
``dict[tuple[str, ...], OutputType]`` input.

Partition keys are always tuples — single-dimension partitions yield
``(value,)``; multi-dimension partitions yield ``(v1, v2, ...)``. Developers
always unpack, regardless of dimensionality.

If any partition of the upstream has failed, ``collect`` blocks the
downstream entirely: the downstream asset does not run until every partition
succeeds. This matches the ``CollectPartitions`` rule in ``barca.allium``.

Semantically, ``collect`` is parallel to ``partitions()`` — it's a marker
class consumed by the ``@asset`` input resolver, not a runtime helper.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class CollectInput:
    """Marker that a downstream input should receive all upstream partitions."""

    upstream: Any  # An AssetWrapper; typed loosely to avoid circular imports.

    def __repr__(self) -> str:
        upstream_name = getattr(self.upstream, "__name__", repr(self.upstream))
        return f"CollectInput({upstream_name})"


def collect(asset_fn: Any) -> CollectInput:
    """Declare that a downstream asset consumes all partitions of ``asset_fn``.

    Example::

        @asset(partitions={"date": partitions(["2024-01", "2024-02"])})
        def report(date: str) -> dict:
            ...

        @asset(inputs={"reports": collect(report)})
        def summary(reports: dict[tuple[str, ...], dict]) -> dict:
            ...
    """
    return CollectInput(upstream=asset_fn)
