"""Prefect: wide join — 10 independent dimension tables -> 1 denormalized fact table."""

import json
import os
import random
import time

from prefect import flow, task
from prefect.task_runners import ConcurrentTaskRunner

# Matches barca's pool_size and dagster's max_concurrent for this benchmark run
# (see benchmarks/lib/env.sh) so no framework gets more/fewer workers than another.
BENCH_WORKERS = int(os.environ.get("BARCA_BENCH_WORKERS", "16"))


@task
def dim_users():
    rng = random.Random(40)
    return {
        "rows": [
            {"id": j, "name": f"user_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(500)
        ]
    }


@task
def dim_products():
    rng = random.Random(41)
    return {
        "rows": [
            {"id": j, "name": f"product_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(1000)
        ]
    }


@task
def dim_stores():
    rng = random.Random(42)
    return {
        "rows": [
            {"id": j, "name": f"store_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(200)
        ]
    }


@task
def dim_regions():
    rng = random.Random(43)
    return {
        "rows": [
            {"id": j, "name": f"region_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(50)
        ]
    }


@task
def dim_campaigns():
    rng = random.Random(44)
    return {
        "rows": [
            {"id": j, "name": f"campaign_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(300)
        ]
    }


@task
def dim_channels():
    rng = random.Random(45)
    return {
        "rows": [
            {"id": j, "name": f"channel_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(20)
        ]
    }


@task
def dim_devices():
    rng = random.Random(46)
    return {
        "rows": [
            {"id": j, "name": f"device_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(100)
        ]
    }


@task
def dim_categories():
    rng = random.Random(47)
    return {
        "rows": [
            {"id": j, "name": f"category_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(80)
        ]
    }


@task
def dim_suppliers():
    rng = random.Random(48)
    return {
        "rows": [
            {"id": j, "name": f"supplier_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(150)
        ]
    }


@task
def dim_currencies():
    rng = random.Random(49)
    return {
        "rows": [
            {"id": j, "name": f"currency_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(30)
        ]
    }


@task
def fact_table(
    users,
    products,
    stores,
    regions,
    campaigns,
    channels,
    devices,
    categories,
    suppliers,
    currencies,
):
    all_dims = [
        users,
        products,
        stores,
        regions,
        campaigns,
        channels,
        devices,
        categories,
        suppliers,
        currencies,
    ]
    total_rows = sum(len(d["rows"]) for d in all_dims)
    combined_score = sum(sum(r["score"] for r in d["rows"]) for d in all_dims)
    return {
        "total_dimension_rows": total_rows,
        "combined_score": round(combined_score, 2),
        "dimensions": 10,
    }


@flow(task_runner=ConcurrentTaskRunner(max_workers=BENCH_WORKERS))
def wide_join_flow():
    # Dimension tables (parallel)
    users = dim_users.submit()
    products = dim_products.submit()
    stores = dim_stores.submit()
    regions = dim_regions.submit()
    campaigns = dim_campaigns.submit()
    channels = dim_channels.submit()
    devices = dim_devices.submit()
    categories = dim_categories.submit()
    suppliers = dim_suppliers.submit()
    currencies = dim_currencies.submit()

    # Fact table (fan-in)
    result = fact_table(
        users,
        products,
        stores,
        regions,
        campaigns,
        channels,
        devices,
        categories,
        suppliers,
        currencies,
    )
    return result


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = wide_join_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 11,
                "result": result,
            },
            indent=2,
        )
    )
