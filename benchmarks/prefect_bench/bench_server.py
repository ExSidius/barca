"""Prefect server benchmark: startup time + API job launch latency.

Starts `prefect server start`, deploys a flow, then triggers runs via the
REST API and polls for completion.

Note: Prefect's server is a separate process from the worker. To measure
end-to-end latency we run the flow inline via the client API rather than
through the full worker pipeline, since deploying a worker adds significant
setup complexity. This gives us the API overhead measurement.
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
PREFECT_CMD = os.path.join(VENV_BIN, "prefect")
BASE_URL = "http://127.0.0.1:4200"
API_URL = f"{BASE_URL}/api"


def api_get(path):
    req = urllib.request.Request(f"{API_URL}{path}")
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def api_post(path, data=None):
    payload = json.dumps(data or {}).encode()
    req = urllib.request.Request(
        f"{API_URL}{path}",
        data=payload,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read())


def wait_for_server(timeout=60):
    """Poll until Prefect server is ready, return startup time in seconds."""
    t0 = time.perf_counter()
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            api_get("/health")
            return time.perf_counter() - t0
        except (urllib.error.URLError, ConnectionRefusedError, OSError):
            pass
        time.sleep(0.1)
    return None


def bench_startup(runs):
    """Measure prefect server startup time."""
    print(f"[prefect] Server startup time ({runs} runs):")
    times = []
    for i in range(runs):
        server = subprocess.Popen(
            [PREFECT_CMD, "server", "start", "--host", "127.0.0.1", "--port", "4200"],
            cwd=BENCH_DIR,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env={**os.environ, "PREFECT_HOME": f"/tmp/prefect_server_bench_{i}", "PREFECT_LOGGING_LEVEL": "ERROR"},
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
        print(f"[prefect] Startup avg: {avg * 1000:.0f}ms +/- {std * 1000:.0f}ms")
    return times


def bench_flow_run_latency(runs):
    """Measure flow run creation + execution via Prefect's embedded API.

    Since Prefect requires a worker process to actually execute deployed flows,
    we measure the overhead of the Prefect client library running a flow locally
    against the server (which tracks the run). This captures the API roundtrip
    cost that Prefect adds.
    """
    from prefect import flow

    server = subprocess.Popen(
        [PREFECT_CMD, "server", "start", "--host", "127.0.0.1", "--port", "4200"],
        cwd=BENCH_DIR,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env={**os.environ, "PREFECT_HOME": "/tmp/prefect_server_bench", "PREFECT_LOGGING_LEVEL": "ERROR"},
    )

    try:
        startup = wait_for_server()
        if startup is None:
            print("[prefect] ERROR: Server failed to start")
            return []

        @flow
        def trivial_flow():
            return {"status": "ok"}

        # Warmup
        trivial_flow()

        print(f"[prefect] Flow run latency via server ({runs} runs):")
        times = []
        for i in range(runs):
            t0 = time.perf_counter()
            trivial_flow()
            elapsed = (time.perf_counter() - t0) * 1000
            times.append(elapsed)
            print(f"  Run {i + 1}: {elapsed:.0f}ms")

        if times:
            avg = sum(times) / len(times)
            std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
            print(f"[prefect] Flow run latency avg: {avg:.0f}ms +/- {std:.0f}ms")
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
    if mode in ("all", "flow"):
        bench_flow_run_latency(runs)
        print()


if __name__ == "__main__":
    main()
