"""Dagster: wide join — 10 independent dimension tables -> 1 denormalized fact table."""

import json
import random
import time

from dagster import AssetIn, asset, materialize


@asset
def dim_users():
    rng = random.Random(40)
    return {
        "rows": [
            {"id": j, "name": f"user_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(500)
        ]
    }


@asset
def dim_products():
    rng = random.Random(41)
    return {
        "rows": [
            {"id": j, "name": f"product_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(1000)
        ]
    }


@asset
def dim_stores():
    rng = random.Random(42)
    return {
        "rows": [
            {"id": j, "name": f"store_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(200)
        ]
    }


@asset
def dim_regions():
    rng = random.Random(43)
    return {
        "rows": [
            {"id": j, "name": f"region_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(50)
        ]
    }


@asset
def dim_campaigns():
    rng = random.Random(44)
    return {
        "rows": [
            {"id": j, "name": f"campaign_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(300)
        ]
    }


@asset
def dim_channels():
    rng = random.Random(45)
    return {
        "rows": [
            {"id": j, "name": f"channel_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(20)
        ]
    }


@asset
def dim_devices():
    rng = random.Random(46)
    return {
        "rows": [
            {"id": j, "name": f"device_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(100)
        ]
    }


@asset
def dim_categories():
    rng = random.Random(47)
    return {
        "rows": [
            {"id": j, "name": f"category_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(80)
        ]
    }


@asset
def dim_suppliers():
    rng = random.Random(48)
    return {
        "rows": [
            {"id": j, "name": f"supplier_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(150)
        ]
    }


@asset
def dim_currencies():
    rng = random.Random(49)
    return {
        "rows": [
            {"id": j, "name": f"currency_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(30)
        ]
    }


@asset(
    ins={
        "users": AssetIn(key="dim_users"),
        "products": AssetIn(key="dim_products"),
        "stores": AssetIn(key="dim_stores"),
        "regions": AssetIn(key="dim_regions"),
        "campaigns": AssetIn(key="dim_campaigns"),
        "channels": AssetIn(key="dim_channels"),
        "devices": AssetIn(key="dim_devices"),
        "categories": AssetIn(key="dim_categories"),
        "suppliers": AssetIn(key="dim_suppliers"),
        "currencies": AssetIn(key="dim_currencies"),
    }
)
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


if __name__ == "__main__":
    all_assets = [
        dim_users,
        dim_products,
        dim_stores,
        dim_regions,
        dim_campaigns,
        dim_channels,
        dim_devices,
        dim_categories,
        dim_suppliers,
        dim_currencies,
        fact_table,
    ]
    t0 = time.perf_counter()
    result = materialize(all_assets)
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 11,
                "success": result.success,
            },
            indent=2,
        )
    )
