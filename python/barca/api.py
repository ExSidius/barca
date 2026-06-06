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
        # Strip any leading "Error: " prefix to avoid doubling.
        if stderr.startswith("Error: "):
            stderr = stderr[len("Error: ") :]
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


def get(target_or_file: str, *extra_files: str, no_cache: bool = False) -> Any:
    """Get asset value(s).

    If target_or_file ends in .py, gets all assets in the file.
    Otherwise, treats it as a target asset name and remaining args as files.

    Returns the deserialized value of the target asset directly.
    """
    args: list[str] = ["get", target_or_file, *extra_files]
    if no_cache:
        args.append("--no-cache")
    result = _exec(args)
    output = result.get("final_output")
    if output is not None:
        return _read_output(output)
    return output


def run(
    target: str,
    *files: str,
    burst: list[str] | None = None,
) -> Any:
    """Run a task (and its cone), bursting upstream asset caches.

    Tasks always re-run. By default every upstream asset is force-rerun; pass
    ``burst=["asset_name", ...]`` to re-run only those assets while the rest
    stay cached.

    Returns the deserialized value of the target task directly (or ``None``).
    """
    args: list[str] = ["run", target, *files]
    if burst:
        args += ["--burst", ",".join(burst)]
    result = _exec(args)
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


def history(limit: int = 10) -> list[dict]:
    """Return recent run history.

    Returns a list of dicts, each with:
        - run_id: str
        - command: str
        - files: str
        - target: str | None
        - status: str
        - steps_total: int | None
        - steps_executed: int
        - steps_cached: int
        - started_at: str
        - finished_at: str | None
        - elapsed_seconds: float | None
    """
    binary = _find_binary()
    result = subprocess.run(
        [binary, "history", "--limit", str(limit)],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        stderr = result.stderr.strip()
        if stderr.startswith("Error: "):
            stderr = stderr[len("Error: ") :]
        raise BarcaError(stderr)

    # Parse the table output into dicts.
    stdout = result.stdout.strip()
    if not stdout or stdout == "No run history found.":
        return []

    lines = stdout.splitlines()
    if len(lines) < 3:  # header + separator + at least one row
        return []

    records = []
    for line in lines[2:]:  # skip header and separator
        parts = line.split()
        if len(parts) < 7:
            continue
        records.append(
            {
                "run_id": parts[0],
                "command": parts[1],
                "status": parts[2],
                "steps_executed": int(parts[3]),
                "steps_cached": int(parts[4]),
                "elapsed_seconds": float(parts[5].rstrip("s")) if parts[5] != "-" else None,
                "started_at": " ".join(parts[6:]),
            }
        )
    return records


def stats(target: str, file: str, *extra_files: str) -> dict:
    """Return execution statistics for an asset.

    Returns a dict with:
        - node_id: str
        - total_runs: int
        - avg_elapsed_seconds: float | None
        - cache_hit_rate: float
        - recent_runs: list of dicts
    """
    files = [file, *extra_files]
    binary = _find_binary()
    result = subprocess.run(
        [binary, "stats", target, *files],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        stderr = result.stderr.strip()
        if stderr.startswith("Error: "):
            stderr = stderr[len("Error: ") :]
        raise BarcaError(stderr)

    stdout = result.stdout.strip()
    lines = stdout.splitlines()

    stats_dict: dict[str, Any] = {
        "node_id": "",
        "total_runs": 0,
        "avg_elapsed_seconds": None,
        "median_elapsed_seconds": None,
        "max_elapsed_seconds": None,
        "p95_elapsed_seconds": None,
        "cache_hit_rate": 0.0,
        "recent_runs": [],
    }

    def _parse_time(s: str) -> float | None:
        s = s.strip().rstrip("s")
        if s == "-":
            return None
        return float(s)

    in_recent = False
    for line in lines:
        if line.startswith("Asset: "):
            stats_dict["node_id"] = line[len("Asset: ") :]
        elif line.startswith("Total materializations: "):
            stats_dict["total_runs"] = int(line.split(": ")[1])
        elif line.startswith("Timing:"):
            # "Timing:  avg 0.105s  median 0.105s  p95 0.105s  max 0.105s"
            parts = line.split()
            for i, part in enumerate(parts):
                if part == "avg" and i + 1 < len(parts):
                    stats_dict["avg_elapsed_seconds"] = _parse_time(parts[i + 1])
                elif part == "median" and i + 1 < len(parts):
                    stats_dict["median_elapsed_seconds"] = _parse_time(parts[i + 1])
                elif part == "p95" and i + 1 < len(parts):
                    stats_dict["p95_elapsed_seconds"] = _parse_time(parts[i + 1])
                elif part == "max" and i + 1 < len(parts):
                    stats_dict["max_elapsed_seconds"] = _parse_time(parts[i + 1])
        elif line.startswith("Cache hit rate: "):
            val = line.split(": ")[1].rstrip("%")
            stats_dict["cache_hit_rate"] = float(val) / 100.0
        elif line.strip().startswith("ELAPSED"):
            in_recent = True
        elif in_recent and line.strip():
            parts = line.split()
            if len(parts) >= 3:
                stats_dict["recent_runs"].append(
                    {
                        "elapsed_seconds": _parse_time(parts[0]),
                        "status": parts[1],
                        "created_at": " ".join(parts[2:]),
                    }
                )

    return stats_dict
