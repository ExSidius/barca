"""Prefect: incremental backfill — 10-step linear chain."""

import hashlib
import json
import random
import time
from collections import Counter

from prefect import flow, task


@task
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


@task
def filter_valid(raw):
    return {"events": [e for e in raw["events"] if e["value"] > 0.1]}


@task
def enrich(data):
    return {
        "events": [
            {**e, "hash": hashlib.md5(str(e["id"]).encode()).hexdigest()[:8]}
            for e in data["events"]
        ]
    }


@task
def aggregate_by_type(data):
    counts = Counter(e["type"] for e in data["events"])
    return {"counts": dict(counts), "total": len(data["events"])}


@task
def compute_rates(data):
    total = data["total"]
    return {
        "rates": {k: round(v / total, 4) for k, v in data["counts"].items()},
        "total": total,
    }


@task
def format_report(data):
    lines = [f"{k}: {v:.2%}" for k, v in data["rates"].items()]
    return {"report": "\n".join(lines), "total": data["total"]}


@task
def validate(data):
    assert data["total"] > 0
    return {"valid": True, "total": data["total"]}


@task
def publish(data):
    return {"published": True, "total": data["total"]}


@task
def notify(data):
    return {"notified": True, "total": data["total"]}


@task
def cleanup(data):
    return {"done": True, "total": data["total"]}


@flow
def incremental_backfill_flow():
    events = daily_events()
    filtered = filter_valid(events)
    enriched = enrich(filtered)
    aggregated = aggregate_by_type(enriched)
    rates = compute_rates(aggregated)
    report = format_report(rates)
    validated = validate(report)
    published = publish(validated)
    notified = notify(published)
    result = cleanup(notified)
    return result


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = incremental_backfill_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 10,
                "result": result,
            },
            indent=2,
        )
    )
