"""Dagster server benchmark: startup time + GraphQL job launch latency.

Starts `dagster dev`, waits for the GraphQL API, then triggers materializations
via GraphQL mutations and polls for completion.

Dagster requires a definitions module. This file includes both the asset
definitions and the benchmark logic.
"""

import json
import math
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

BENCH_DIR = os.path.dirname(os.path.abspath(__file__))
VENV_BIN = os.path.join(BENCH_DIR, ".venv", "bin")
BASE_URL = "http://127.0.0.1:3000"
GRAPHQL_URL = f"{BASE_URL}/graphql"
DAGSTER_HOME = os.environ.get("DAGSTER_HOME", "/tmp/dagster_server_bench")


# ── Dagster definitions (used by `dagster dev -f bench_server.py`) ───────────

from dagster import Definitions
from dagster import asset as dagster_asset


@dagster_asset
def single_asset() -> dict:
    return {"status": "ok"}


defs = Definitions(assets=[single_asset])


# ── Benchmark helpers ────────────────────────────────────────────────────────


def graphql_query(query, variables=None):
    """Execute a GraphQL query against the Dagster API."""
    payload = json.dumps({"query": query, "variables": variables or {}}).encode()
    req = urllib.request.Request(
        GRAPHQL_URL,
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read())


def wait_for_server(timeout=60):
    """Poll until Dagster webserver is ready, return startup time in seconds."""
    t0 = time.perf_counter()
    deadline = time.time() + timeout
    query = "{ repositoriesOrError { __typename } }"
    while time.time() < deadline:
        try:
            result = graphql_query(query)
            if "data" in result:
                return time.perf_counter() - t0
        except (urllib.error.URLError, ConnectionRefusedError, OSError):
            pass
        time.sleep(0.1)
    return None


def launch_materialization(asset_key):
    """Launch a materialization run via GraphQL, return run ID."""
    query = """
    mutation LaunchRun($executionParams: ExecutionParams!) {
        launchRun(executionParams: $executionParams) {
            __typename
            ... on LaunchRunSuccess {
                run {
                    runId
                }
            }
            ... on PythonError {
                message
            }
        }
    }
    """
    variables = {
        "executionParams": {
            "selector": {
                "repositoryLocationName": "bench_server.py",
                "repositoryName": "__repository__",
                "jobName": "__ASSET_JOB",
            },
            "stepKeys": [f"{asset_key}"],
            "mode": "default",
        }
    }
    result = graphql_query(query, variables)
    launch = result["data"]["launchRun"]
    if launch["__typename"] != "LaunchRunSuccess":
        raise RuntimeError(f"Failed to launch: {launch}")
    return launch["run"]["runId"]


def poll_run_completion(run_id, timeout=30):
    """Poll until run completes, return (status, elapsed_ms)."""
    query = """
    query RunStatus($runId: ID!) {
        runOrError(runId: $runId) {
            ... on Run {
                status
            }
        }
    }
    """
    t0 = time.perf_counter()
    deadline = time.time() + timeout
    while time.time() < deadline:
        result = graphql_query(query, {"runId": run_id})
        status = result["data"]["runOrError"]["status"]
        if status in ("SUCCESS", "FAILURE", "CANCELED"):
            elapsed = (time.perf_counter() - t0) * 1000
            return status, elapsed
        time.sleep(0.05)
    return "TIMEOUT", (time.perf_counter() - t0) * 1000


def _dagster_env():
    os.makedirs(DAGSTER_HOME, exist_ok=True)
    return {**os.environ, "DAGSTER_HOME": DAGSTER_HOME}


def _dagster_cmd():
    return [os.path.join(VENV_BIN, "dagster"), "dev", "-f", "bench_server.py", "-p", "3000"]


def bench_startup(runs):
    """Measure dagster dev startup time."""
    print(f"[dagster] Server startup time ({runs} runs):")
    times = []
    for i in range(runs):
        server = subprocess.Popen(
            _dagster_cmd(),
            cwd=BENCH_DIR,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=_dagster_env(),
        )
        try:
            startup = wait_for_server()
            if startup is None:
                print(f"  Run {i + 1}: FAILED (timeout)")
                continue
            times.append(startup)
            print(f"  Run {i + 1}: {startup * 1000:.0f}ms")
        finally:
            server.terminate()
            server.wait(timeout=10)
            time.sleep(1)  # Let port release

    if times:
        avg = sum(times) / len(times)
        std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
        print(f"[dagster] Startup avg: {avg * 1000:.0f}ms +/- {std * 1000:.0f}ms")
    return times


def bench_refresh_latency(runs):
    """Measure GraphQL materialization latency (launch → success)."""
    server = subprocess.Popen(
        _dagster_cmd(),
        cwd=BENCH_DIR,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env=_dagster_env(),
    )

    try:
        startup = wait_for_server()
        if startup is None:
            print("[dagster] ERROR: Server failed to start")
            return []

        # Warmup run
        run_id = launch_materialization("single_asset")
        poll_run_completion(run_id)

        print(f"[dagster] GraphQL materialization latency ({runs} runs):")
        times = []
        for i in range(runs):
            t0 = time.perf_counter()
            run_id = launch_materialization("single_asset")
            status, _poll_ms = poll_run_completion(run_id)
            total_ms = (time.perf_counter() - t0) * 1000
            times.append(total_ms)
            print(f"  Run {i + 1}: {total_ms:.0f}ms ({status})")

        if times:
            avg = sum(times) / len(times)
            std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
            print(f"[dagster] Materialization latency avg: {avg:.0f}ms +/- {std:.0f}ms")
        return times

    finally:
        server.terminate()
        server.wait(timeout=10)


def main():
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 5
    mode = sys.argv[2] if len(sys.argv) > 2 else "all"

    if mode in ("all", "startup"):
        bench_startup(runs)
        print()
    if mode in ("all", "refresh"):
        bench_refresh_latency(runs)
        print()


if __name__ == "__main__":
    main()
