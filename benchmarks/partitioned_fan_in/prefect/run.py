"""Prefect: 50 partitioned fetches + 50 partitioned enrichments = 100 tasks.
Each (step, region) pair is a separate task, mirroring barca's partitioned fan-in."""

import hashlib
import json
import time
from prefect import flow, task

REGIONS = [f"region_{i:02d}" for i in range(50)]


# --- Step 1: fetch_metrics (50 tasks) ---


def _make_fetch(i, region):
    @task(name=f"fetch_metrics_{i:02d}")
    def _fetch():
        h = int(hashlib.md5(region.encode()).hexdigest()[:8], 16)
        return {"region": region, "users": h % 10000, "revenue": (h % 1000000) / 100.0}

    return _fetch


fetch_tasks = [_make_fetch(i, r) for i, r in enumerate(REGIONS)]


# --- Step 2: enrich (50 tasks) ---


def _make_enrich(i, region):
    @task(name=f"enrich_{i:02d}")
    def _enrich(data):
        return {**data, "enriched": True, "score": data["users"] * data["revenue"]}

    return _enrich


enrich_tasks = [_make_enrich(i, r) for i, r in enumerate(REGIONS)]


@flow
def partitioned_fan_in_flow():
    results = []
    for i in range(len(REGIONS)):
        fetched = fetch_tasks[i]()
        enriched = enrich_tasks[i](fetched)
        results.append(enriched)
    return results


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = partitioned_fan_in_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 100,
            },
            indent=2,
        )
    )
