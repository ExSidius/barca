"""Barca — invisible asset orchestrator.

This module provides decorator stubs and marker classes. The decorators are
pure no-ops (identity functions) — all logic lives in the Rust binary which
parses these statically from source without importing.
"""

from __future__ import annotations

import json
import os
import sys

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
    "parallel",
    "parallel_map",
    "ParallelError",
    "get",
    "run",
    "plan",
    "history",
    "stats",
    "BarcaError",
]

# True when running inside a barca worker process (set by Rust via env var).
_BARCA_WORKER = os.environ.get("BARCA_WORKER") == "1"


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


# ─── Parallel primitives ─────────────────────────────────────────────────────


class ParallelError:
    """Represents a failed branch in a parallel() call."""

    def __init__(self, error: str) -> None:
        self.error = error

    def __repr__(self) -> str:
        return f"ParallelError({self.error!r})"

    def __str__(self) -> str:
        return self.error


def parallel(*callables):
    """Run callables in parallel across worker processes.

    Each argument should be a `functools.partial` wrapping a @task-decorated
    function. Returns a list of results (or ParallelError objects) in argument
    order.

    When running inside a barca worker (BARCA_WORKER=1), uses the stderr/stdin
    protocol to request Rust to dispatch branches as separate workers. When
    running standalone, executes sequentially.
    """
    if not _BARCA_WORKER:
        # Standalone: execute sequentially.
        return [c() for c in callables]

    # Build work items from the callables (must be functools.partial objects).
    items = []
    for c in callables:
        if hasattr(c, "func") and hasattr(c, "args") and hasattr(c, "keywords"):
            # It's a functools.partial.
            fn = c.func
            fn_name = fn.__name__
            # Get the source file from the function's code object.
            source_file = getattr(fn, "__code__", None)
            source_file = getattr(source_file, "co_filename", "") if source_file else ""
            fn_ref = f"{source_file}:{fn_name}" if source_file else fn_name
            items.append(
                {
                    "fn_ref": fn_ref,
                    "args": list(c.args),
                    "kwargs": dict(c.keywords),
                }
            )
        else:
            raise TypeError(f"parallel() expects functools.partial objects, got {type(c).__name__}")

    # Send request to Rust via stderr protocol.
    request = json.dumps({"parallel_request": items})
    print(f"BARCA:2:{request}", file=sys.stderr, flush=True)

    # Block and read response from stdin.
    response_line = sys.stdin.readline()
    if not response_line:
        raise RuntimeError("parallel(): no response from orchestrator")

    response = json.loads(response_line)
    results = []
    for item in response:
        if item.get("status") == "ok":
            results.append(item.get("result"))
        else:
            results.append(ParallelError(item.get("error", "unknown error")))
    return results


def parallel_map(fn, items, **kwargs):
    """Map a @task function over items in parallel.

    Sugar for `parallel(*(partial(fn, item, **kwargs) for item in items))`.
    """
    from functools import partial

    return parallel(*(partial(fn, item, **kwargs) for item in items))


# ─── Python API ──────────────────────────────────────────────────────────────

from barca.api import BarcaError, get, history, plan, run, stats  # noqa: E402
