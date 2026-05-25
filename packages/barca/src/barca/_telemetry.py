"""Telemetry shim — optional Datadog APM integration.

Barca emits a small number of named spans (``barca.materialize``,
``barca.run_pass``, ``barca.reindex``) and structured log fields for
trace correlation. When ``ddtrace`` is installed (``uv sync --extra
datadog``) and ``BARCA_DATADOG_ENABLED=1`` is set, these flow into a
Datadog Agent over the standard ``DD_AGENT_HOST``/``DD_TRACE_AGENT_URL``
contract. Otherwise everything degrades to no-ops with zero overhead.

Call sites stay clean:

    from barca._telemetry import span

    with span("barca.materialize", asset_id=detail.asset.asset_id):
        ...

The context manager always returns; whether it produces a real span is
an environment concern, not a code concern.
"""

from __future__ import annotations

import os
from collections.abc import Iterator
from contextlib import contextmanager
from typing import Any

_TRACER: Any | None = None
_ENABLED: bool = False


def init_telemetry() -> bool:
    """Configure ddtrace if available and ``BARCA_DATADOG_ENABLED=1``.

    Returns True when real tracing is active, False when the shim stays
    in no-op mode. Safe to call multiple times — subsequent calls return
    the same answer without reconfiguring.
    """
    global _TRACER, _ENABLED
    if _TRACER is not None:
        return _ENABLED

    if os.environ.get("BARCA_DATADOG_ENABLED") != "1":
        _ENABLED = False
        return False

    try:
        from ddtrace import tracer  # type: ignore[import-not-found]
    except ImportError:
        _ENABLED = False
        return False

    _TRACER = tracer
    _ENABLED = True
    return True


@contextmanager
def span(name: str, **tags: Any) -> Iterator[Any]:
    """Open a span named ``name`` with ``tags`` attached.

    When ddtrace isn't active this is a zero-allocation pass-through.
    Tag values are coerced to strings on the boundary so callers can
    pass ints / paths / pydantic models without thinking about it.
    """
    if not _ENABLED or _TRACER is None:
        yield None
        return

    with _TRACER.trace(name) as s:
        for key, value in tags.items():
            try:
                s.set_tag(key, str(value))
            except Exception:
                pass
        yield s


def current_trace_context() -> tuple[str | None, str | None]:
    """Return ``(trace_id, span_id)`` strings for the current context, or
    ``(None, None)`` when tracing is off or no span is active.

    Used by the JSON log formatter to emit ``dd.trace_id`` / ``dd.span_id``
    fields so logs correlate to traces in the Datadog UI.
    """
    if not _ENABLED or _TRACER is None:
        return (None, None)
    s = _TRACER.current_span()
    if s is None:
        return (None, None)
    try:
        return (str(s.trace_id), str(s.span_id))
    except Exception:
        return (None, None)
