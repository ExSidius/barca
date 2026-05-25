"""Structured logging configuration for barca server."""

from __future__ import annotations

import json
import logging
import sys
from datetime import UTC, datetime

from barca._telemetry import current_trace_context, init_telemetry


class JSONFormatter(logging.Formatter):
    """Emit log records as single-line JSON.

    When Datadog APM is active (``BARCA_DATADOG_ENABLED=1`` + the
    ``datadog`` extra installed), each record gets ``dd.trace_id`` /
    ``dd.span_id`` so the Datadog UI can correlate logs to traces.
    Both fields are omitted when no span is active.
    """

    def format(self, record: logging.LogRecord) -> str:
        entry = {
            "ts": datetime.fromtimestamp(record.created, tz=UTC).isoformat(),
            "level": record.levelname.lower(),
            "logger": record.name,
            "msg": record.getMessage(),
        }
        if record.exc_info and record.exc_info[1]:
            entry["error"] = str(record.exc_info[1])
        trace_id, span_id = current_trace_context()
        if trace_id is not None:
            entry["dd.trace_id"] = trace_id
            entry["dd.span_id"] = span_id
        return json.dumps(entry)


def configure_logging(level: str = "info") -> None:
    """Set up structured JSON logging on stderr.

    Also calls ``init_telemetry()`` so any spans opened by request
    handlers or the background scheduler get attached to the configured
    Datadog tracer (no-op when the extra isn't installed).
    """
    init_telemetry()
    handler = logging.StreamHandler(sys.stderr)
    handler.setFormatter(JSONFormatter())
    root = logging.getLogger("barca")
    root.handlers.clear()
    root.addHandler(handler)
    root.setLevel(getattr(logging, level.upper(), logging.INFO))
