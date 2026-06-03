"""Wide join: 10 independent dimension tables -> 1 denormalized fact table.
11 assets. Tests fan-in with real data merging."""

import random

from barca import asset


@asset()
def dim_users() -> dict:
    rng = random.Random(40)
    return {
        "rows": [
            {"id": j, "name": f"user_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(500)
        ]
    }


@asset()
def dim_products() -> dict:
    rng = random.Random(41)
    return {
        "rows": [
            {"id": j, "name": f"product_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(1000)
        ]
    }


@asset()
def dim_stores() -> dict:
    rng = random.Random(42)
    return {
        "rows": [
            {"id": j, "name": f"store_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(200)
        ]
    }


@asset()
def dim_regions() -> dict:
    rng = random.Random(43)
    return {
        "rows": [
            {"id": j, "name": f"region_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(50)
        ]
    }


@asset()
def dim_campaigns() -> dict:
    rng = random.Random(44)
    return {
        "rows": [
            {"id": j, "name": f"campaign_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(300)
        ]
    }


@asset()
def dim_channels() -> dict:
    rng = random.Random(45)
    return {
        "rows": [
            {"id": j, "name": f"channel_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(20)
        ]
    }


@asset()
def dim_devices() -> dict:
    rng = random.Random(46)
    return {
        "rows": [
            {"id": j, "name": f"device_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(100)
        ]
    }


@asset()
def dim_categories() -> dict:
    rng = random.Random(47)
    return {
        "rows": [
            {"id": j, "name": f"category_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(80)
        ]
    }


@asset()
def dim_suppliers() -> dict:
    rng = random.Random(48)
    return {
        "rows": [
            {"id": j, "name": f"supplier_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(150)
        ]
    }


@asset()
def dim_currencies() -> dict:
    rng = random.Random(49)
    return {
        "rows": [
            {"id": j, "name": f"currency_{j}", "score": round(rng.random() * 100, 2)}
            for j in range(30)
        ]
    }


@asset(
    inputs={
        "users": dim_users,
        "products": dim_products,
        "stores": dim_stores,
        "regions": dim_regions,
        "campaigns": dim_campaigns,
        "channels": dim_channels,
        "devices": dim_devices,
        "categories": dim_categories,
        "suppliers": dim_suppliers,
        "currencies": dim_currencies,
    }
)
def fact_table(
    users: dict,
    products: dict,
    stores: dict,
    regions: dict,
    campaigns: dict,
    channels: dict,
    devices: dict,
    categories: dict,
    suppliers: dict,
    currencies: dict,
) -> dict:
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
