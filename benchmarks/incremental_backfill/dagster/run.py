"""Dagster: incremental backfill — 10-step linear chain."""

import hashlib
import json
import random
import time
from collections import Counter

from dagster import AssetIn, asset, materialize


@asset
def daily_events():
    rng = random.Random(42)
    return {
        "events": [
            {
                "id": i,
                "type": rng.choice(["click", "view", "purchase"]),
                "value": rng.random(),
            }
            for i in range(1000)
        ]
    }


@asset(ins={"raw": AssetIn(key="daily_events")})
def filter_valid(raw):
    return {"events": [e for e in raw["events"] if e["value"] > 0.1]}


@asset(ins={"data": AssetIn(key="filter_valid")})
def enrich(data):
    return {
        "events": [
            {**e, "hash": hashlib.md5(str(e["id"]).encode()).hexdigest()[:8]}
            for e in data["events"]
        ]
    }


@asset(ins={"data": AssetIn(key="enrich")})
def aggregate_by_type(data):
    counts = Counter(e["type"] for e in data["events"])
    return {"counts": dict(counts), "total": len(data["events"])}


@asset(ins={"data": AssetIn(key="aggregate_by_type")})
def compute_rates(data):
    total = data["total"]
    return {
        "rates": {k: round(v / total, 4) for k, v in data["counts"].items()},
        "total": total,
    }


@asset(ins={"data": AssetIn(key="compute_rates")})
def format_report(data):
    lines = [f"{k}: {v:.2%}" for k, v in data["rates"].items()]
    return {"report": "\n".join(lines), "total": data["total"]}


@asset(ins={"data": AssetIn(key="format_report")})
def validate(data):
    assert data["total"] > 0
    return {"valid": True, "total": data["total"]}


@asset(ins={"data": AssetIn(key="validate")})
def publish(data):
    return {"published": True, "total": data["total"]}


@asset(ins={"data": AssetIn(key="publish")})
def notify(data):
    return {"notified": True, "total": data["total"]}


@asset(ins={"data": AssetIn(key="notify")})
def cleanup(data):
    return {"done": True, "total": data["total"]}


if __name__ == "__main__":
    all_assets = [
        daily_events,
        filter_valid,
        enrich,
        aggregate_by_type,
        compute_rates,
        format_report,
        validate,
        publish,
        notify,
        cleanup,
    ]
    t0 = time.perf_counter()
    result = materialize(all_assets)
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 10,
                "success": result.success,
            },
            indent=2,
        )
    )
