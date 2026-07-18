"""Prefect: 50 independent fetches gathered into ONE aggregate task.
The true collect() shape (many-to-one), unlike partitioned_fan_in's 1:1
partition-aligned chain. Prefect's natural idiom for this is exactly what
collect() expresses in barca: submit every fetch, gather the futures into a
list, hand the whole list to one downstream task."""

import hashlib
import json
import os
import time

from prefect import flow, task
from prefect.task_runners import ConcurrentTaskRunner

REGIONS = [f"region_{i:02d}" for i in range(50)]

# Matches barca's pool_size and dagster's max_concurrent for this benchmark run
# (see benchmarks/lib/env.sh) so no framework gets more/fewer workers than another.
BENCH_WORKERS = int(os.environ.get("BARCA_BENCH_WORKERS", "16"))


def _make_fetch(i, region):
    @task(name=f"fetch_metrics_{i:02d}")
    def _fetch():
        h = int(hashlib.md5(region.encode()).hexdigest()[:8], 16)
        return {"region": region, "users": h % 10000, "revenue": (h % 1000000) / 100.0}

    return _fetch


fetch_tasks = [_make_fetch(i, r) for i, r in enumerate(REGIONS)]


@task
def aggregate(reports):
    return {
        "regions": len(reports),
        "total_users": sum(r["users"] for r in reports),
        "total_revenue": round(sum(r["revenue"] for r in reports), 2),
    }


@flow(task_runner=ConcurrentTaskRunner(max_workers=BENCH_WORKERS))
def collect_fan_in_flow():
    futures = [t.submit() for t in fetch_tasks]
    reports = [f.result() for f in futures]
    return aggregate(reports)


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = collect_fan_in_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": len(REGIONS) + 1,
                "result": result,
            },
            indent=2,
        )
    )
