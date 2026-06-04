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
    """Locate the barca binary and validate version on first call."""
    global _cached_binary
    if _cached_binary is not None:
        return _cached_binary

    binary = None

    # Check sibling of the Python interpreter (same venv bin/).
    bin_dir = Path(sys.executable).parent
    candidate = bin_dir / "barca"
    if candidate.is_file():
        binary = str(candidate)

    # Fall back to PATH.
    if binary is None:
        binary = shutil.which("barca")

    if binary is None:
        raise BarcaError("barca binary not found. Install with: uv add barca")

    # Validate version matches the Python package.
    try:
        import barca

        result = subprocess.run([binary, "--version"], capture_output=True, text=True, timeout=5)
        if result.returncode == 0:
            bin_version = result.stdout.strip().split()[-1]
            if bin_version != barca.__version__:
                import warnings

                warnings.warn(
                    f"barca binary version ({bin_version}) differs from Python package "
                    f"({barca.__version__}). This may cause protocol errors. "
                    f"Binary: {binary}",
                    stacklevel=2,
                )
    except Exception:
        pass  # Don't block on version check failures.

    _cached_binary = binary
    return _cached_binary


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
    If it's a sentinel-wrapped artifact reference, read from disk.
    """
    if isinstance(output_ref, dict) and "_barca_artifact" in output_ref:
        meta = output_ref["_barca_artifact"]
        return deserialize(meta["path"], meta["format"])
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
