"""Batch worker: reads a JSON queue of jobs and processes them sequentially.

Usage:
    python -m barca.batch_worker --queue-file /path/to/queue.json --results-file /path/to/results.json

Queue file format (JSON array):
    [
        {
            "job_id": 1,
            "asset_id": 10,
            "module_path": "my_module.assets",
            "function_name": "my_asset",
            "output_dir": "/tmp/staging/job-1",
            "input_kwargs_json": "{\"x\": 42}"  // or null
        },
        ...
    ]

Results file format (JSON array, written after all jobs complete):
    [
        {"job_id": 1, "ok": true, "output_dir": "/tmp/staging/job-1", "error": null},
        {"job_id": 2, "ok": false, "output_dir": "/tmp/staging/job-2", "error": "KeyError: 'x'"},
        ...
    ]

The worker imports each module once (cached by Python), then calls the
unwrapped original function for each job. This avoids per-job process
startup overhead.
"""

from __future__ import annotations

import argparse
import json
import sys
import traceback


def main() -> None:
    parser = argparse.ArgumentParser(description="Batch materialization worker")
    parser.add_argument("--queue-file", required=True, help="Path to JSON queue file")
    parser.add_argument("--results-file", required=True, help="Path to write JSON results")
    args = parser.parse_args()

    with open(args.queue_file) as f:
        jobs = json.load(f)

    results = []
    for job in jobs:
        result = execute_job(job)
        results.append(result)

    with open(args.results_file, "w") as f:
        json.dump(results, f)


def execute_job(job: dict) -> dict:
    """Execute a single materialization job.

    Calls barca._barca.materialize_asset for the given module/function,
    capturing the result or any error.
    """
    job_id = job["job_id"]
    module_path = job["module_path"]
    function_name = job["function_name"]
    output_dir = job["output_dir"]
    input_kwargs_json = job.get("input_kwargs_json")

    try:
        from barca._barca import materialize_asset

        materialize_asset(module_path, function_name, output_dir, input_kwargs_json)
        return {
            "job_id": job_id,
            "ok": True,
            "output_dir": output_dir,
            "error": None,
        }
    except Exception as exc:
        tb = traceback.format_exc()
        print(f"[batch_worker] job {job_id} failed: {exc}", file=sys.stderr)
        print(tb, file=sys.stderr)

        # Still try to write a result.json for the Rust side
        error_result = {
            "ok": False,
            "error": str(exc),
            "error_type": type(exc).__name__,
        }
        try:
            import os
            os.makedirs(output_dir, exist_ok=True)
            with open(os.path.join(output_dir, "result.json"), "w") as f:
                json.dump(error_result, f)
        except Exception:
            pass

        return {
            "job_id": job_id,
            "ok": False,
            "output_dir": output_dir,
            "error": str(exc),
        }


if __name__ == "__main__":
    main()
