"""Dagster: 3-step pipeline x 50 partitions = 150 assets.
Each (step, ticker) pair is a separate asset, mirroring barca's partitioned chain."""

import hashlib
import json
import time
from dagster import asset, AssetIn, materialize

TICKERS = [f"T{i:03d}" for i in range(50)]

# --- Step 1: fetch_NNN (50 independent assets) ---

all_assets = []


def _make_fetch(i, ticker):
    @asset(name=f"fetch_{i:03d}")
    def _fetch():
        return {"ticker": ticker, "price": len(ticker) * 10 + hash(ticker) % 100}

    return _fetch


fetch_assets = []
for i, t in enumerate(TICKERS):
    a = _make_fetch(i, t)
    fetch_assets.append(a)
    all_assets.append(a)


# --- Step 2: normalize_NNN (50 assets, each depends on fetch_NNN) ---


def _make_normalize(i, ticker):
    @asset(name=f"normalize_{i:03d}", ins={"data": AssetIn(key=f"fetch_{i:03d}")})
    def _normalize(data):
        return {"ticker": ticker, "normalized": data["price"] / 100.0}

    return _normalize


normalize_assets = []
for i, t in enumerate(TICKERS):
    a = _make_normalize(i, t)
    normalize_assets.append(a)
    all_assets.append(a)


# --- Step 3: score_NNN (50 assets, each depends on normalize_NNN) ---


def _make_score(i, ticker):
    @asset(name=f"score_{i:03d}", ins={"data": AssetIn(key=f"normalize_{i:03d}")})
    def _score(data):
        h = hashlib.md5(f"{ticker}:{data['normalized']}".encode()).hexdigest()
        return {"ticker": ticker, "score": int(h[:8], 16) / 2**32}

    return _score


score_assets = []
for i, t in enumerate(TICKERS):
    a = _make_score(i, t)
    score_assets.append(a)
    all_assets.append(a)


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = materialize(all_assets)
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 150,
                "success": result.success,
            },
            indent=2,
        )
    )
