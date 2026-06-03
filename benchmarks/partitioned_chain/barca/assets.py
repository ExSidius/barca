"""Partitioned chain: 3-step pipeline × 50 partitions.
Tests partition-parallel linear chain execution.
fetch(ticker) → normalize(ticker) → score(ticker) for 50 tickers."""

import hashlib

from barca import asset, partitions

TICKERS = [f"T{i:03d}" for i in range(50)]


@asset(partitions={"ticker": partitions(TICKERS)})
def fetch(ticker: str) -> dict:
    return {"ticker": ticker, "price": len(ticker) * 10 + hash(ticker) % 100}


@asset(inputs={"data": fetch}, partitions={"ticker": partitions(TICKERS)})
def normalize(data: dict, ticker: str) -> dict:
    return {"ticker": ticker, "normalized": data["price"] / 100.0}


@asset(inputs={"data": normalize}, partitions={"ticker": partitions(TICKERS)})
def score(data: dict, ticker: str) -> dict:
    h = hashlib.md5(f"{ticker}:{data['normalized']}".encode()).hexdigest()
    return {"ticker": ticker, "score": int(h[:8], 16) / 2**32}
