"""Artifact serialization for Barca — format detection, read/write, path management.

Supports three formats:
  - json:    dicts, lists, primitives (stdlib json)
  - pickle:  arbitrary Python objects (stdlib pickle, protocol 5)
  - parquet: pandas/polars DataFrames (requires pyarrow)
"""

import json
import pickle
import re
from pathlib import Path
from typing import Any

_FORMAT_EXTENSIONS = {
    "json": ".json",
    "pickle": ".pkl",
    "parquet": ".parquet",
}


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


def _is_json_serializable(value: Any) -> bool:
    """Check if a value can be losslessly serialized as JSON."""
    try:
        json.dumps(value)
        return True
    except (TypeError, ValueError, OverflowError):
        return False


def serialize(value: Any, path: Path | str, fmt: str) -> int:
    """Write value to path in the given format. Returns size in bytes."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)

    if fmt == "json":
        data = json.dumps(value).encode()
        path.write_bytes(data)
        return len(data)

    if fmt == "pickle":
        with open(path, "wb") as f:
            pickle.dump(value, f, protocol=5)
        return path.stat().st_size

    if fmt == "parquet":
        return _serialize_parquet(value, path)

    raise ValueError(f"Unknown format: {fmt}")


def _serialize_parquet(value: Any, path: Path) -> int:
    """Write a DataFrame to parquet. Handles pandas, polars, and polars LazyFrame."""
    type_name = type(value).__name__
    module = type(value).__module__ or ""

    if module.startswith("polars"):
        if type_name == "LazyFrame":
            value = value.collect()
        value.write_parquet(str(path))
        return path.stat().st_size

    if module.startswith("pandas"):
        value.to_parquet(str(path))
        return path.stat().st_size

    # Fallback: try pandas-style API, then pickle with warning.
    if hasattr(value, "to_parquet"):
        value.to_parquet(str(path))
        return path.stat().st_size

    import sys

    print(
        f"[barca] Warning: parquet format requested but value is {type_name}, falling back to pickle",
        file=sys.stderr,
    )
    with open(path.with_suffix(".pkl"), "wb") as f:
        pickle.dump(value, f, protocol=5)
    return path.with_suffix(".pkl").stat().st_size


def deserialize(path: Path | str, fmt: str) -> Any:
    """Read an artifact from path using the given format."""
    path = Path(path)

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
        import pandas as pd  # ty: ignore[unresolved-import]

        return pd.read_parquet(str(path))
    except ImportError:
        pass

    try:
        import polars as pl  # ty: ignore[unresolved-import]

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


def artifact_path(artifact_dir: Path | str, node_id: str, fmt: str) -> Path:
    """Compute the deterministic artifact path for a node + format."""
    ext = _FORMAT_EXTENSIONS.get(fmt, f".{fmt}")
    return Path(artifact_dir) / f"{safe_node_id(node_id)}{ext}"
