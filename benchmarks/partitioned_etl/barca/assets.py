"""Partitioned ETL: partitions_from → partition chain.
Tests deferred partition resolution + aligned execution."""

import hashlib
import random

from barca import asset, partitions_from


@asset()
def ticker_universe() -> list:
    """Source of partition values — resolved at runtime."""
    return [f"TICK_{i:03d}" for i in range(30)]


@asset(partitions={"ticker": partitions_from(ticker_universe)})
def fetch_prices(ticker: str) -> dict:
    rng = random.Random(hash(ticker))
    return {"ticker": ticker, "price": round(rng.uniform(10, 500), 2)}


@asset(
    inputs={"data": fetch_prices},
    partitions={"ticker": partitions_from(ticker_universe)},
)
def enrich(data: dict, ticker: str) -> dict:
    h = hashlib.sha256(f"{ticker}:{data['price']}".encode()).hexdigest()[:8]
    return {**data, "hash": h}
