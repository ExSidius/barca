"""Collect() fan-in: 50 partitioned fetches gathered into ONE aggregate call
via collect(). Unlike partitioned_fan_in (a partition-aligned 1:1 chain, no
collect()), this is the many-to-one gather collect() actually exists for."""

import hashlib

from barca import asset, collect, partitions

REGIONS = [f"region_{i:02d}" for i in range(50)]


@asset(partitions={"region": partitions(REGIONS)})
def fetch_metrics(region: str) -> dict:
    h = int(hashlib.md5(region.encode()).hexdigest()[:8], 16)
    return {"region": region, "users": h % 10000, "revenue": (h % 1000000) / 100.0}


@asset(inputs={"reports": collect(fetch_metrics)})
def aggregate(reports: list[dict]) -> dict:
    return {
        "regions": len(reports),
        "total_users": sum(r["users"] for r in reports),
        "total_revenue": round(sum(r["revenue"] for r in reports), 2),
    }
