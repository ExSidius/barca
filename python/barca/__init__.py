"""Barca — invisible asset orchestrator.

This module provides decorator stubs and marker classes. The decorators are
pure no-ops (identity functions) — all logic lives in the Rust binary which
parses these statically from source without importing.
"""

from __future__ import annotations

__version__ = "0.1.5"

__all__ = [
    "asset",
    "sensor",
    "task",
    "sink",
    "unsafe",
    "Always",
    "Manual",
    "Schedule",
    "partitions",
    "partitions_from",
    "collect",
    "asset_ref",
    "get",
    "run",
    "plan",
    "history",
    "stats",
    "BarcaError",
]


# ─── Freshness markers ───────────────────────────────────────────────────────


class Always:
    """Auto-materializes whenever stale and all upstreams are fresh."""


class Manual:
    """Only runs on explicit refresh."""


class Schedule:
    """Runs on a cron schedule."""

    def __init__(self, cron: str) -> None:
        self.cron = cron


# ─── Decorators ───────────────────────────────────────────────────────────────


def asset(fn=None, *, serializer=None, retries=1, retry_backoff=0.0, **kwargs):
    """Declare a cached asset node.

    `retries` is the total number of attempts on failure (1 = no retry).
    `retry_backoff` is the base delay in seconds between attempts (delay grows
    linearly: `retry_backoff * attempt`). Both are read statically by the Rust
    binary, which owns the retry loop; this stub stays a no-op.
    """
    if fn is not None:
        return fn

    def decorator(f):
        return f

    return decorator


def sensor(fn=None, *, retries=1, retry_backoff=0.0, **kwargs):
    """Declare a sensor node (observes external state).

    See `asset` for `retries` / `retry_backoff` semantics.
    """
    if fn is not None:
        return fn

    def decorator(f):
        return f

    return decorator


def task(fn=None, *, inputs=None, retries=1, retry_backoff=0.0, **kwargs):
    """Declare a task node (always re-runs; never cached).

    Tasks model workflow-management steps — deploys, notifications, migrations,
    cache warming — that *do* something rather than produce cacheable data. They
    may appear anywhere in the graph and may depend on assets, sensors, or other
    tasks, but must not be an input to an asset or sensor.

    See `asset` for `retries` / `retry_backoff` semantics.
    """
    if fn is not None:
        return fn

    def decorator(f):
        return f

    return decorator


def sink(path: str, **kwargs):
    """Declare a sink output (stacked on @asset)."""

    def decorator(f):
        return f

    return decorator


def unsafe(fn):
    """Mark a function as unsafe (untraceable)."""
    return fn


# ─── Marker functions ─────────────────────────────────────────────────────────


def partitions(values):
    """Declare static partition values."""
    return values


def partitions_from(source):
    """Derive partitions from an upstream asset."""
    return source


def collect(asset_fn):
    """Aggregate all partitions of an upstream asset."""
    return asset_fn


def asset_ref(ref_string: str) -> str:
    """Canonical asset reference."""
    return ref_string


# ─── Python API ──────────────────────────────────────────────────────────────

from barca.api import BarcaError, get, history, plan, run, stats  # noqa: E402
