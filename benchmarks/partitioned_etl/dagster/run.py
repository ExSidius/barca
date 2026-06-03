"""Dagster: 1 source + 30 fetches + 30 enrichments = 61 assets.
Mirrors barca's partitioned ETL with deferred partition resolution."""

import hashlib
import json
import random
import time
from dagster import asset, AssetIn, materialize

# In barca, ticker_universe() returns the partition list at runtime.
# Here we resolve it eagerly (same result) and expand into individual assets.
TICKERS = [f"TICK_{i:03d}" for i in range(30)]

all_assets = []


# --- Source: ticker_universe (1 asset) ---


@asset
def ticker_universe():
    return [f"TICK_{i:03d}" for i in range(30)]


all_assets.append(ticker_universe)


# --- Step 1: fetch_prices_NN (30 assets, each independent) ---


def _make_fetch(i, ticker):
    @asset(name=f"fetch_prices_{i:02d}")
    def _fetch():
        rng = random.Random(hash(ticker))
        return {"ticker": ticker, "price": round(rng.uniform(10, 500), 2)}

    return _fetch


fetch_assets = []
for i, t in enumerate(TICKERS):
    a = _make_fetch(i, t)
    fetch_assets.append(a)
    all_assets.append(a)


# --- Step 2: enrich_NN (30 assets, each depends on fetch_prices_NN) ---


def _make_enrich(i, ticker):
    @asset(name=f"enrich_{i:02d}", ins={"data": AssetIn(key=f"fetch_prices_{i:02d}")})
    def _enrich(data):
        h = hashlib.sha256(f"{ticker}:{data['price']}".encode()).hexdigest()[:8]
        return {**data, "hash": h}

    return _enrich


enrich_assets = []
for i, t in enumerate(TICKERS):
    a = _make_enrich(i, t)
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
                "steps_executed": 61,
                "success": result.success,
            },
            indent=2,
        )
    )
