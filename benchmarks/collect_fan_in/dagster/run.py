"""Dagster: 50 independent fetches gathered into ONE aggregate asset.
The true collect() shape (many-to-one), unlike partitioned_fan_in's 1:1
partition-aligned chain. Dagster has no first-class "gather all partitions
into a list" primitive used elsewhere in this suite, so this wires 50 named
AssetIn deps into one asset — the closest equivalent to barca's collect().
`ins`/the aggregate's `**kwargs` are built programmatically (verified
against the installed dagster that `**kwargs` resolves named `ins` in
declaration order) rather than hand-duplicated per index."""

import hashlib
import json
import time

from dagster import AssetIn, asset, materialize

REGIONS = [f"region_{i:02d}" for i in range(50)]


def _make_fetch(i, region):
    @asset(name=f"fetch_metrics_{i:02d}")
    def _fetch():
        h = int(hashlib.md5(region.encode()).hexdigest()[:8], 16)
        return {"region": region, "users": h % 10000, "revenue": (h % 1000000) / 100.0}

    return _fetch


fetch_assets = [_make_fetch(i, r) for i, r in enumerate(REGIONS)]

_aggregate_ins = {f"r{i:02d}": AssetIn(key=f"fetch_metrics_{i:02d}") for i in range(len(REGIONS))}


@asset(ins=_aggregate_ins)
def aggregate(**kwargs):
    reports = list(kwargs.values())
    return {
        "regions": len(reports),
        "total_users": sum(r["users"] for r in reports),
        "total_revenue": round(sum(r["revenue"] for r in reports), 2),
    }


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = materialize([*fetch_assets, aggregate])
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": len(REGIONS) + 1,
                "success": result.success,
            },
            indent=2,
        )
    )
