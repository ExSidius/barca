"""Dagster: 50 partitioned fetches + 50 partitioned enrichments = 100 assets.
Each (step, region) pair is a separate asset, mirroring barca's partitioned fan-in."""

import hashlib
import json
import time
from dagster import asset, AssetIn, materialize

REGIONS = [f"region_{i:02d}" for i in range(50)]

all_assets = []


# --- Step 1: fetch_metrics_NN (50 independent assets) ---


def _make_fetch(i, region):
    @asset(name=f"fetch_metrics_{i:02d}")
    def _fetch():
        h = int(hashlib.md5(region.encode()).hexdigest()[:8], 16)
        return {"region": region, "users": h % 10000, "revenue": (h % 1000000) / 100.0}

    return _fetch


fetch_assets = []
for i, r in enumerate(REGIONS):
    a = _make_fetch(i, r)
    fetch_assets.append(a)
    all_assets.append(a)


# --- Step 2: enrich_NN (50 assets, each depends on fetch_metrics_NN) ---


def _make_enrich(i, region):
    @asset(name=f"enrich_{i:02d}", ins={"data": AssetIn(key=f"fetch_metrics_{i:02d}")})
    def _enrich(data):
        return {**data, "enriched": True, "score": data["users"] * data["revenue"]}

    return _enrich


enrich_assets = []
for i, r in enumerate(REGIONS):
    a = _make_enrich(i, r)
    enrich_assets.append(a)
    all_assets.append(a)


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = materialize(all_assets)
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 100,
                "success": result.success,
            },
            indent=2,
        )
    )
