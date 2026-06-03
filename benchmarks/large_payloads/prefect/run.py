"""Prefect: 5-step chain passing ~1MB dicts between steps."""

import json
import random
import time
from prefect import flow, task


@task
def generate_data():
    """Generate a 10k-row dataset with 10 numeric columns."""
    rng = random.Random(42)
    rows = []
    for i in range(10000):
        rows.append(
            {
                "id": i,
                "a": rng.random(),
                "b": rng.random(),
                "c": rng.random(),
                "d": rng.random(),
                "e": rng.random(),
                "f": rng.random(),
                "g": rng.random(),
                "h": rng.random(),
                "i": rng.random(),
                "j": rng.random(),
            }
        )
    return {
        "rows": rows,
        "schema": ["id", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j"],
    }


@task
def normalize(data):
    """Normalize all numeric columns to [0, 1] range."""
    cols = [c for c in data["schema"] if c != "id"]
    mins = {c: min(r[c] for r in data["rows"]) for c in cols}
    maxs = {c: max(r[c] for r in data["rows"]) for c in cols}
    rows = []
    for r in data["rows"]:
        row = {"id": r["id"]}
        for c in cols:
            span = maxs[c] - mins[c]
            row[c] = (r[c] - mins[c]) / span if span > 0 else 0
        rows.append(row)
    return {"rows": rows, "schema": data["schema"]}


@task
def add_features(data):
    """Add derived feature columns."""
    rows = []
    for r in data["rows"]:
        row = dict(r)
        row["ab_product"] = r["a"] * r["b"]
        row["cd_sum"] = r["c"] + r["d"]
        row["efg_mean"] = (r["e"] + r["f"] + r["g"]) / 3
        rows.append(row)
    return {
        "rows": rows,
        "schema": data["schema"] + ["ab_product", "cd_sum", "efg_mean"],
    }


@task
def filter_outliers(data):
    """Remove rows where any feature > 0.95 (simulated outlier removal)."""
    cols = [c for c in data["schema"] if c != "id"]
    rows = [r for r in data["rows"] if all(r.get(c, 0) <= 0.95 for c in cols)]
    return {
        "rows": rows,
        "schema": data["schema"],
        "removed": len(data["rows"]) - len(rows),
    }


@task
def aggregate(data):
    """Compute summary statistics."""
    cols = [c for c in data["schema"] if c != "id"]
    n = len(data["rows"])
    stats = {}
    for c in cols:
        values = [r.get(c, 0) for r in data["rows"]]
        stats[c] = {
            "mean": sum(values) / n if n else 0,
            "min": min(values) if values else 0,
            "max": max(values) if values else 0,
        }
    return {"stats": stats, "row_count": n, "removed": data.get("removed", 0)}


@flow
def large_payloads_flow():
    data = generate_data()
    data = normalize(data)
    data = add_features(data)
    data = filter_outliers(data)
    result = aggregate(data)
    return result


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = large_payloads_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 5,
                "result": result,
            },
            indent=2,
        )
    )
