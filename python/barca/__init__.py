"""Barca — invisible asset orchestrator.

This module provides decorator stubs and marker classes. The decorators are
pure no-ops (identity functions) — all logic lives in the Rust binary which
parses these statically from source without importing.
"""

from __future__ import annotations

__version__ = "0.1.4"

__all__ = [
    "asset",
    "sensor",
    "effect",
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


def asset(fn=None, *, serializer=None, **kwargs):
    """Declare a cached asset node."""
    if fn is not None:
        return fn

    def decorator(f):
        return f

    return decorator


def sensor(fn=None, **kwargs):
    """Declare a sensor node (observes external state)."""
    if fn is not None:
        return fn

    def decorator(f):
        return f

    return decorator


def effect(fn=None, **kwargs):
    """Declare an effect node (side-effect leaf)."""
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

from barca.api import BarcaError, get, history, plan, stats  # noqa: E402
