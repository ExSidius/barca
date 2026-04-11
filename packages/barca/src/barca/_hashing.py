"""Hashing utilities — pure functions for definition/run/codebase hashes."""

from __future__ import annotations

import hashlib
import json
import os
import time
from pathlib import Path

PROTOCOL_VERSION = "0.4.0"

# Environment variable for frozen time during tests. When set to an integer
# epoch-seconds value, every call to `now_ts()` returns exactly that value.
# This is how the test suite achieves deterministic materialization timestamps
# and cron-tick eligibility without filtering volatile strings from snapshots.
#
# Set to "auto" to make `now_ts()` return a monotonically-incrementing counter
# (starting at the frozen base) so sequential inserts get distinct, ordered
# timestamps without real wall-clock variation. Tests that need distinct times
# between events should use "auto" and call `advance_time()` explicitly.
_FROZEN_TIME_ENV = "BARCA_TEST_NOW"
_AUTO_TICK: int = 0

SKIP_DIRS = frozenset(
    {
        ".venv",
        "__pycache__",
        ".git",
        ".barca",
        ".barcafiles",
        "build",
        "dist",
        "node_modules",
        "target",
        "tmp",
    }
)


def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def compute_codebase_hash(root: Path) -> str:
    """Merkle-tree hash of all .py files + uv.lock under root."""
    leaf_hashes: list[tuple[str, str]] = []

    uv_lock = root / "uv.lock"
    if uv_lock.is_file():
        leaf_hashes.append(("uv.lock", sha256_hex(uv_lock.read_bytes())))

    _walk_py_for_hash(root, root, leaf_hashes)
    leaf_hashes.sort(key=lambda x: x[0])

    h = hashlib.sha256()
    for path, file_hash in leaf_hashes:
        h.update(path.encode())
        h.update(b"\0")
        h.update(file_hash.encode())
        h.update(b"\n")
    return h.hexdigest()


def _walk_py_for_hash(root: Path, directory: Path, out: list[tuple[str, str]]) -> None:
    try:
        entries = list(directory.iterdir())
    except OSError:
        return
    for entry in entries:
        name = entry.name
        if name.startswith(".") or name in SKIP_DIRS:
            continue
        if entry.is_dir():
            _walk_py_for_hash(root, entry, out)
        elif entry.suffix == ".py":
            try:
                data = entry.read_bytes()
            except OSError:
                continue
            rel = relative_path(root, entry)
            out.append((rel, sha256_hex(data)))


def compute_definition_hash(
    *,
    dependency_cone_hash: str,
    function_source: str,
    decorator_metadata: dict,
    serializer_kind: str,
    python_version: str,
) -> str:
    """SHA-256 of the canonical JSON payload.

    ``sort_keys=True`` ensures decorator metadata order does not affect the
    hash — so ``@asset(inputs=X, freshness=Y)`` and ``@asset(freshness=Y,
    inputs=X)`` produce the same definition_hash.
    """
    payload = {
        "dependency_cone_hash": dependency_cone_hash,
        "function_source": function_source,
        "decorator_metadata": decorator_metadata,
        "serializer_kind": serializer_kind,
        "python_version": python_version,
        "protocol_version": PROTOCOL_VERSION,
    }
    return sha256_hex(json.dumps(payload, separators=(",", ":"), sort_keys=True).encode())


def compute_run_hash(
    definition_hash: str,
    upstream_materialization_ids: list[int],
    partition_key_json: str | None = None,
) -> str:
    """SHA-256 of the canonical run payload.

    Upstream materialization ids are sorted before hashing so that callers
    don't have to worry about iteration order.
    """
    payload = {
        "definition_hash": definition_hash,
        "upstream_materialization_ids": sorted(upstream_materialization_ids),
        "partition_key_json": partition_key_json,
    }
    return sha256_hex(json.dumps(payload, separators=(",", ":"), sort_keys=True).encode())


def relative_path(root: Path, path: Path) -> str:
    try:
        return str(path.relative_to(root)).replace("\\", "/")
    except ValueError:
        return str(path).replace("\\", "/")


def slugify(parts: list[str]) -> str:
    joined = "-".join(parts)
    out: list[str] = []
    last_dash = False
    for ch in joined:
        if ch.isascii() and ch.isalnum():
            out.append(ch.lower())
            last_dash = False
        else:
            if not last_dash:
                out.append("-")
            last_dash = True
    return "".join(out).strip("-")


def now_ts() -> int:
    """Return the current epoch-seconds timestamp, honoring BARCA_TEST_NOW.

    If BARCA_TEST_NOW is unset or invalid, returns int(time.time()).
    If set to an integer, returns exactly that integer (fully frozen).
    If set to "auto:<base>", returns a monotonically-incrementing counter
    starting at <base>. Each call bumps the counter by 1.
    """
    frozen = os.environ.get(_FROZEN_TIME_ENV)
    if frozen is None:
        return int(time.time())
    if frozen.startswith("auto:"):
        global _AUTO_TICK
        try:
            base = int(frozen[len("auto:") :])
        except ValueError:
            return int(time.time())
        value = base + _AUTO_TICK
        _AUTO_TICK += 1
        return value
    try:
        return int(frozen)
    except ValueError:
        return int(time.time())


def _reset_auto_tick() -> None:
    """Reset the auto-tick counter. Called by test fixtures between tests."""
    global _AUTO_TICK
    _AUTO_TICK = 0
