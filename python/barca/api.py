"""Barca Python API — programmatic access to the asset orchestrator.

Calls the barca binary under the hood, parses results, and returns
Python objects. Artifact files are deserialized automatically.
"""

import json
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

from barca._artifacts import deserialize


class BarcaError(Exception):
    """Raised when a barca command fails."""


_cached_binary: str | None = None


def _find_binary() -> str:
    """Locate the barca binary, with version validation."""
    global _cached_binary
    if _cached_binary is not None:
        return _cached_binary

    # Check sibling of the Python interpreter (same venv bin/).
    bin_dir = Path(sys.executable).parent
    candidate = bin_dir / "barca"
    if candidate.is_file():
        _cached_binary = str(candidate)
        return _cached_binary

    # Fall back to PATH.
    found = shutil.which("barca")
    if found:
        _cached_binary = found
        return _cached_binary

    raise BarcaError("barca binary not found. Install with: uv add barca")


def _exec(args: list[str]) -> dict:
    """Run barca with args, return parsed JSON from the last stdout line."""
    binary = _find_binary()
    result = subprocess.run(
        [binary, *args],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        stderr = result.stderr.strip()
        raise BarcaError(stderr)

    stdout = result.stdout.strip()
    if not stdout:
        raise BarcaError("No output from barca")
    # Try parsing the full output as JSON first (handles pretty-printed plan output).
    # Fall back to last line (for run/get where user prints precede the JSON).
    try:
        return json.loads(stdout)
    except json.JSONDecodeError:
        last_line = stdout.splitlines()[-1]
        return json.loads(last_line)


def _read_output(output_ref: Any) -> Any:
    """Deserialize a final_output value.

    If it's already a plain value (dict, list, int, etc.), return as-is.
    If it's artifact metadata (has artifact_path), read from disk.
    """
    if (
        isinstance(output_ref, dict)
        and set(output_ref.keys()) == {"artifact_path", "artifact_format", "artifact_size_bytes"}
        and output_ref.get("artifact_format") in ("json", "pickle", "parquet")
    ):
        return deserialize(output_ref["artifact_path"], output_ref["artifact_format"])
    return output_ref


def run(file: str, *extra_files: str) -> dict:
    """Execute all assets in a pipeline.

    Returns a dict with:
        - elapsed_seconds: float
        - steps_executed: int
        - phases: int
        - final_output: the deserialized value of the last asset
    """
    files = [file, *extra_files]
    result = _exec(["run", *files])
    if result.get("final_output") is not None:
        result["final_output"] = _read_output(result["final_output"])
    return result


def get(target: str, file: str, *extra_files: str) -> Any:
    """Get a fresh asset value (cache-aware).

    Returns the deserialized value of the target asset directly.
    """
    files = [file, *extra_files]
    result = _exec(["get", target, *files])
    output = result.get("final_output")
    if output is not None:
        return _read_output(output)
    return output


def plan(file: str, *extra_files: str) -> dict:
    """Return the execution plan as a dict.

    Returns a dict with:
        - total_steps: int
        - phases: list of phase dicts
    """
    files = [file, *extra_files]
    return _exec(["plan", *files])
