"""Artifact serialization for Barca — format detection, read/write, path management.

Supports three formats:
  - json:    dicts, lists, primitives (stdlib json)
  - pickle:  arbitrary Python objects (stdlib pickle, protocol 5)
  - parquet: pandas/polars DataFrames (requires pyarrow)

Destinations may be local paths or remote URIs (abfss://, s3://, gs://, ...
— see barca._storage). Every write is staged through a local temp file and
then finalized with an atomic os.replace (local) or a chunked upload
(remote), so serialized payloads are never buffered fully in memory and a
crash mid-write never leaves a partial artifact at the destination.
"""

import json
import os
import pickle
import re
import tempfile
from contextlib import contextmanager
from pathlib import Path
from typing import Any

from barca import _storage

_FORMAT_EXTENSIONS = {
    "json": ".json",
    "pickle": ".pkl",
    "parquet": ".parquet",
}

# Staging area for remote uploads/downloads. Deliberately on project disk
# rather than the system tempdir: /tmp is commonly tmpfs on Linux, which
# would put multi-hundred-MB payloads back in RAM.
_STAGING_DIR = ".barca/staging"


def detect_format(value: Any, explicit: str | None = None) -> str:
    """Auto-detect the best serialization format for a value.

    Priority:
      1. Explicit override (if provided)
      2. pandas/polars DataFrame → parquet
      3. JSON-serializable → json
      4. Fallback → pickle
    """
    if explicit is not None:
        return explicit

    # Check for DataFrame types without requiring the imports at module level.
    type_name = type(value).__name__
    module = type(value).__module__ or ""

    if type_name in ("DataFrame", "LazyFrame") and (
        module.startswith("pandas") or module.startswith("polars")
    ):
        return "parquet"

    # Try JSON — must succeed without default=str to be considered safe.
    if _is_json_serializable(value):
        return "json"

    return "pickle"


def resolve_format(value: Any, fmt: str) -> str:
    """Downgrade parquet to pickle when the value has no parquet writer.

    Must be called before computing the artifact path so the extension,
    the receipt, and the bytes on disk all agree.
    """
    if fmt != "parquet":
        return fmt
    module = type(value).__module__ or ""
    if module.startswith("polars") or hasattr(value, "to_parquet"):
        return fmt

    import sys

    print(
        f"[barca] Warning: parquet format requested but value is "
        f"{type(value).__name__}, falling back to pickle",
        file=sys.stderr,
    )
    return "pickle"


def _is_json_serializable(value: Any) -> bool:
    """Check if a value can be losslessly serialized as JSON."""
    try:
        json.dumps(value)
        return True
    except (TypeError, ValueError, OverflowError):
        return False


def _staging_dir() -> Path:
    d = Path(_STAGING_DIR)
    d.mkdir(parents=True, exist_ok=True)
    return d


def clean_staging() -> None:
    """Best-effort removal of stale temp files left by crashed workers."""
    d = Path(_STAGING_DIR)
    if not d.is_dir():
        return
    for tmp in d.glob("*.tmp"):
        try:
            tmp.unlink()
        except OSError:
            pass


def _make_temp(directory: Path, prefix: str = "stage-") -> Path:
    fd, name = tempfile.mkstemp(dir=directory, prefix=prefix, suffix=".tmp")
    os.close(fd)
    return Path(name)


@contextmanager
def _staged_write(dest: "Path | str"):
    """Yield a local temp path to write into; finalize to dest on success.

    Local dest: temp file in the destination directory (guarantees same
    filesystem), atomic os.replace on success. Remote dest: temp file in
    .barca/staging/, chunked upload on success. Either way the temp file is
    removed on failure and the destination is never left partially written.
    """
    if _storage.is_remote(dest):
        tmp = _make_temp(_staging_dir())
        try:
            yield tmp
            _storage.put_file(tmp, str(dest))
        finally:
            tmp.unlink(missing_ok=True)
    else:
        dest = Path(dest)
        dest.parent.mkdir(parents=True, exist_ok=True)
        tmp = _make_temp(dest.parent, prefix=f".{dest.name}.")
        try:
            yield tmp
            os.replace(tmp, dest)
        except BaseException:
            tmp.unlink(missing_ok=True)
            raise


def serialize(value: Any, path: "Path | str", fmt: str) -> int:
    """Write value to path (local or remote URI) in the given format.

    Returns size in bytes. The caller is responsible for having resolved
    the format first (see resolve_format) so path and fmt agree.
    """
    if fmt not in ("json", "pickle", "parquet"):
        raise ValueError(f"Unknown format: {fmt}")

    size = 0
    with _staged_write(path) as tmp:
        if fmt == "json":
            with open(tmp, "w") as f:
                json.dump(value, f)
        elif fmt == "pickle":
            with open(tmp, "wb") as f:
                pickle.dump(value, f, protocol=5)
        else:
            _write_parquet(value, tmp)
        size = tmp.stat().st_size
    return size


def _write_parquet(value: Any, path: Path) -> None:
    """Write a DataFrame to parquet. Handles pandas, polars, and polars LazyFrame."""
    type_name = type(value).__name__
    module = type(value).__module__ or ""

    if module.startswith("polars"):
        if type_name == "LazyFrame":
            value = value.collect()
        value.write_parquet(str(path))
        return

    if hasattr(value, "to_parquet"):
        value.to_parquet(str(path))
        return

    raise TypeError(
        f"parquet format requires a DataFrame, got {type_name} "
        "(use resolve_format() to downgrade to pickle first)"
    )


def deserialize(path: "Path | str", fmt: str) -> Any:
    """Read an artifact from a local path or remote URI using the given format."""
    if _storage.is_remote(path):
        tmp = _make_temp(_staging_dir(), prefix="fetch-")
        try:
            _storage.get_file(str(path), tmp)
            return _deserialize_local(tmp, fmt)
        finally:
            tmp.unlink(missing_ok=True)
    return _deserialize_local(Path(path), fmt)


def _deserialize_local(path: Path, fmt: str) -> Any:
    if fmt == "json":
        with open(path) as f:
            return json.load(f)

    if fmt == "pickle":
        with open(path, "rb") as f:
            return pickle.load(f)

    if fmt == "parquet":
        return _deserialize_parquet(path)

    raise ValueError(f"Unknown format: {fmt}")


def _deserialize_parquet(path: Path) -> Any:
    """Read a parquet file. Prefers pandas if available, then polars."""
    try:
        import pandas as pd

        return pd.read_parquet(str(path))
    except ImportError:
        pass

    try:
        import polars as pl

        return pl.read_parquet(str(path))
    except ImportError:
        pass

    raise ImportError(
        "Reading parquet requires pandas or polars. Install one: pip install pandas pyarrow"
    )


def safe_node_id(node_id: str) -> str:
    """Sanitize a node_id for use as a filename (no special chars)."""
    # Replace known special characters with safe alternatives.
    s = node_id
    s = s.replace("/", "__")
    s = s.replace(":", "--")
    s = s.replace("[", "_")
    s = s.replace("]", "")
    s = s.replace("=", "_")
    s = s.replace(",", "_")
    s = s.replace(" ", "_")
    # Remove any remaining problematic characters.
    s = re.sub(r"[^\w.\-]", "_", s)
    return s


def artifact_path(artifact_dir: "Path | str", node_id: str, fmt: str) -> "Path | str":
    """Compute the deterministic artifact path for a node + format.

    Returns a Path for a local artifact_dir, or a URI string when
    artifact_dir is a remote prefix (e.g. BARCA_ARTIFACT_URI).
    """
    ext = _FORMAT_EXTENSIONS.get(fmt, f".{fmt}")
    return _storage.join(artifact_dir, f"{safe_node_id(node_id)}{ext}")
