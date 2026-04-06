"""Barca server job pickup benchmark: time from HTTP POST to job completion.

Starts the barca server, POSTs a materialize request, and polls the job API
until the status transitions to 'success'.
"""

import json
import os
import subprocess
import sys
import time
import urllib.error
import urllib.request

BENCH_DIR = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.dirname(os.path.dirname(BENCH_DIR))
CLI = os.path.join(REPO_ROOT, "target", "release", "barca")
BASE_URL = "http://127.0.0.1:3000"


def wait_for_server(timeout=30):
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            urllib.request.urlopen(f"{BASE_URL}/api/assets", timeout=1)
            return True
        except (urllib.error.URLError, ConnectionRefusedError, OSError):
            time.sleep(0.1)
    return False


def api_get(path):
    req = urllib.request.Request(f"{BASE_URL}{path}")
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def api_post(path):
    req = urllib.request.Request(f"{BASE_URL}{path}", method="POST", data=b"")
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.loads(resp.read())


def find_asset_id(name):
    assets = api_get("/api/assets")
    for a in assets:
        if name in a.get("continuity_key", "") or name in a.get("function_name", ""):
            return a["asset_id"]
    raise RuntimeError(f"Asset '{name}' not found via API")


def measure_pickup(asset_id):
    """POST materialize, poll until success, return latencies."""
    t0 = time.perf_counter()
    detail = api_post(f"/api/assets/{asset_id}/materialize")
    t_post = time.perf_counter()

    mat = detail.get("latest_materialization")
    if not mat:
        return None

    job_id = mat["materialization_id"]

    t_running = None
    while True:
        job = api_get(f"/api/jobs/{job_id}")
        status = job["job"]["status"]
        if status == "running" and t_running is None:
            t_running = time.perf_counter()
        if status in ("success", "failed"):
            t_done = time.perf_counter()
            break
        time.sleep(0.002)  # 2ms poll

    pickup = ((t_running or t_done) - t0) * 1000
    total = (t_done - t0) * 1000
    post = (t_post - t0) * 1000
    return pickup, total, post


def main():
    runs = int(sys.argv[1]) if len(sys.argv) > 1 else 5

    print("[barca] Starting server...")
    server = subprocess.Popen(
        [CLI, "serve"],
        cwd=BENCH_DIR,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env={**os.environ, "RUST_LOG": "warn"},
    )

    try:
        if not wait_for_server():
            print("[barca] ERROR: Server failed to start")
            return

        asset_id = find_asset_id("single_asset")
        print(f"[barca] Job pickup latency for single_asset (ID={asset_id}, {runs} runs):")

        pickups = []
        totals = []
        for i in range(runs):
            # Force fresh materialization by resetting artifacts
            subprocess.run([CLI, "reset", "--artifacts"], cwd=BENCH_DIR, capture_output=True, env={**os.environ, "RUST_LOG": "error"})
            api_post("/api/reindex")
            time.sleep(0.3)

            result = measure_pickup(asset_id)
            if result:
                pickup, total, post = result
                pickups.append(pickup)
                totals.append(total)
                print(f"  Run {i + 1}: pickup={pickup:.0f}ms, total={total:.0f}ms (POST={post:.0f}ms)")

        if pickups:
            print(f"[barca] Avg pickup: {sum(pickups) / len(pickups):.0f}ms, avg total: {sum(totals) / len(totals):.0f}ms")
    finally:
        server.terminate()
        server.wait(timeout=5)


if __name__ == "__main__":
    main()
