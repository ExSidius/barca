"""Partitioned fan-in: 50 partitioned fetches → partitioned enrichment.
Tests partition expansion + partition-aligned chains."""

import hashlib

from barca import asset, partitions

REGIONS = [f"region_{i:02d}" for i in range(50)]


@asset(partitions={"region": partitions(REGIONS)})
def fetch_metrics(region: str) -> dict:
    h = int(hashlib.md5(region.encode()).hexdigest()[:8], 16)
    return {"region": region, "users": h % 10000, "revenue": (h % 1000000) / 100.0}


@asset(inputs={"data": fetch_metrics}, partitions={"region": partitions(REGIONS)})
def enrich(data: dict, region: str) -> dict:
    return {**data, "enriched": True, "score": data["users"] * data["revenue"]}
