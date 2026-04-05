"""Barca server benchmark: startup time + HTTP job pickup latency.

Starts the barca server, measures time-to-ready, then POSTs refresh requests
and measures end-to-end latency (POST → materialization complete).
"""

import math
import os
import subprocess
import sys
import time
import json
import urllib.request
import urllib.error

BENCH_DIR = os.path.dirname(os.path.abspath(__file__))
BASE_URL = "http://127.0.0.1:8400"
CLI = "barca"


def wait_for_server(timeout=30):
    """Poll until the server is ready, return startup time in seconds."""
    t0 = time.perf_counter()
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            urllib.request.urlopen(f"{BASE_URL}/health", timeout=1)
            return time.perf_counter() - t0
        except (urllib.error.URLError, ConnectionRefusedError, OSError):
            time.sleep(0.05)
    return None


def api_get(path):
    req = urllib.request.Request(f"{BASE_URL}{path}")
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def api_post(path):
    req = urllib.request.Request(f"{BASE_URL}{path}", method="POST", data=b"")
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read())


def find_asset_id(name):
    assets = api_get("/assets")
    for a in assets:
        if name in a.get("continuity_key", "") or name in a.get("function_name", ""):
            return a["asset_id"]
    raise RuntimeError(f"Asset '{name}' not found via API")


def measure_refresh(asset_id):
    """POST /assets/{id}/refresh, return total latency in ms."""
    t0 = time.perf_counter()
    detail = api_post(f"/assets/{asset_id}/refresh")
    elapsed = time.perf_counter() - t0

    mat = detail.get("latest_materialization")
    status = mat["status"] if mat else "unknown"
    return elapsed * 1000, status


def bench_startup(runs):
    """Measure server startup time (cold → /health 200)."""
    print(f"[barca] Server startup time ({runs} runs):")
    times = []
    for i in range(runs):
        # Clean slate
        subprocess.run([CLI, "reset", "--db", "--artifacts"], cwd=BENCH_DIR,
                       capture_output=True)

        server = subprocess.Popen(
            [CLI, "serve", "--port", "8400", "--interval", "3600", "--log-level", "error"],
            cwd=BENCH_DIR, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        try:
            startup = wait_for_server()
            if startup is None:
                print(f"  Run {i+1}: FAILED (timeout)")
                continue
            times.append(startup)
            print(f"  Run {i+1}: {startup*1000:.0f}ms")
        finally:
            server.terminate()
            server.wait(timeout=5)

    if times:
        avg = sum(times) / len(times)
        std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
        print(f"[barca] Startup avg: {avg*1000:.0f}ms +/- {std*1000:.0f}ms")
    return times


def bench_refresh_latency(runs):
    """Measure HTTP refresh latency (POST → materialization complete)."""
    # Start server once
    subprocess.run([CLI, "reset", "--db", "--artifacts"], cwd=BENCH_DIR,
                   capture_output=True)
    server = subprocess.Popen(
        [CLI, "serve", "--port", "8400", "--interval", "3600", "--log-level", "error"],
        cwd=BENCH_DIR, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )

    try:
        startup = wait_for_server()
        if startup is None:
            print("[barca] ERROR: Server failed to start")
            return []

        asset_id = find_asset_id("single_asset")
        print(f"[barca] HTTP refresh latency for single_asset (ID={asset_id}, {runs} runs):")

        times = []
        for i in range(runs):
            # Reset artifacts to force re-materialization
            subprocess.run([CLI, "reset", "--artifacts"], cwd=BENCH_DIR,
                           capture_output=True)
            # Re-index after reset
            api_post("/reconcile")
            time.sleep(0.1)

            latency_ms, status = measure_refresh(asset_id)
            times.append(latency_ms)
            print(f"  Run {i+1}: {latency_ms:.0f}ms ({status})")

        if times:
            avg = sum(times) / len(times)
            std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
            print(f"[barca] Refresh latency avg: {avg:.0f}ms +/- {std:.0f}ms")
        return times

    finally:
        server.terminate()
        server.wait(timeout=5)


def bench_reconcile_latency(runs):
    """Measure HTTP reconcile latency (POST → reconcile complete)."""
    subprocess.run([CLI, "reset", "--db", "--artifacts"], cwd=BENCH_DIR,
                   capture_output=True)
    server = subprocess.Popen(
        [CLI, "serve", "--port", "8400", "--interval", "3600", "--log-level", "error"],
        cwd=BENCH_DIR, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )

    try:
        startup = wait_for_server()
        if startup is None:
            print("[barca] ERROR: Server failed to start")
            return []

        print(f"[barca] HTTP reconcile latency ({runs} runs):")
        times = []
        for i in range(runs):
            t0 = time.perf_counter()
            result = api_post("/reconcile")
            elapsed = (time.perf_counter() - t0) * 1000
            n_assets = result.get("executed_assets", 0)
            n_sensors = result.get("executed_sensors", 0)
            times.append(elapsed)
            print(f"  Run {i+1}: {elapsed:.0f}ms (assets={n_assets}, sensors={n_sensors})")

        if times:
            avg = sum(times) / len(times)
            std = math.sqrt(sum((t - avg) ** 2 for t in times) / len(times))
            print(f"[barca] Reconcile latency avg: {avg:.0f}ms +/- {std:.0f}ms")
        return times

    finally:
        server.terminate()
        server.wait(timeout=5)


def main():
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 5
    mode = sys.argv[2] if len(sys.argv) > 2 else "all"

    if mode in ("all", "startup"):
        bench_startup(runs)
        print()
    if mode in ("all", "refresh"):
        bench_refresh_latency(runs)
        print()
    if mode in ("all", "reconcile"):
        bench_reconcile_latency(runs)
        print()


if __name__ == "__main__":
    main()
