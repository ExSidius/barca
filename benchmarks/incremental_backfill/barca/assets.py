"""Incremental backfill: 10-step pipeline run repeatedly.
Tests per-invocation overhead when the same DAG is executed many times.
Each step does light work (simulating an incremental daily process)."""

import hashlib
import random

from barca import asset


@asset()
def daily_events() -> dict:
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


@asset(inputs={"raw": daily_events})
def filter_valid(raw: dict) -> dict:
    return {"events": [e for e in raw["events"] if e["value"] > 0.1]}


@asset(inputs={"data": filter_valid})
def enrich(data: dict) -> dict:
    return {
        "events": [
            {**e, "hash": hashlib.md5(str(e["id"]).encode()).hexdigest()[:8]}
            for e in data["events"]
        ]
    }


@asset(inputs={"data": enrich})
def aggregate_by_type(data: dict) -> dict:
    from collections import Counter

    counts = Counter(e["type"] for e in data["events"])
    return {"counts": dict(counts), "total": len(data["events"])}


@asset(inputs={"data": aggregate_by_type})
def compute_rates(data: dict) -> dict:
    total = data["total"]
    return {
        "rates": {k: round(v / total, 4) for k, v in data["counts"].items()},
        "total": total,
    }


@asset(inputs={"data": compute_rates})
def format_report(data: dict) -> dict:
    lines = [f"{k}: {v:.2%}" for k, v in data["rates"].items()]
    return {"report": "\n".join(lines), "total": data["total"]}


@asset(inputs={"data": format_report})
def validate(data: dict) -> dict:
    assert data["total"] > 0
    return {"valid": True, "total": data["total"]}


@asset(inputs={"data": validate})
def publish(data: dict) -> dict:
    return {"published": True, "total": data["total"]}


@asset(inputs={"data": publish})
def notify(data: dict) -> dict:
    return {"notified": True, "total": data["total"]}


@asset(inputs={"data": notify})
def cleanup(data: dict) -> dict:
    return {"done": True, "total": data["total"]}
