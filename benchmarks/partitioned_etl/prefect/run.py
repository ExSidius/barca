"""Prefect: 1 source + 30 fetches + 30 enrichments = 61 tasks.
Mirrors barca's partitioned ETL with deferred partition resolution."""

import hashlib
import json
import random
import time
from prefect import flow, task

TICKERS = [f"TICK_{i:03d}" for i in range(30)]


# --- Source: ticker_universe (1 task) ---


@task
def ticker_universe():
    return [f"TICK_{i:03d}" for i in range(30)]


# --- Step 1: fetch_prices (30 tasks) ---


def _make_fetch(i, ticker):
    @task(name=f"fetch_prices_{i:02d}")
    def _fetch():
        rng = random.Random(hash(ticker))
        return {"ticker": ticker, "price": round(rng.uniform(10, 500), 2)}

    return _fetch


fetch_tasks = [_make_fetch(i, t) for i, t in enumerate(TICKERS)]


# --- Step 2: enrich (30 tasks) ---


def _make_enrich(i, ticker):
    @task(name=f"enrich_{i:02d}")
    def _enrich(data):
        h = hashlib.sha256(f"{ticker}:{data['price']}".encode()).hexdigest()[:8]
        return {**data, "hash": h}

    return _enrich


enrich_tasks = [_make_enrich(i, t) for i, t in enumerate(TICKERS)]


@flow
def partitioned_etl_flow():
    universe = ticker_universe()  # noqa: F841 — mirrors barca's source asset
    results = []
    for i in range(len(TICKERS)):
        fetched = fetch_tasks[i]()
        enriched = enrich_tasks[i](fetched)
        results.append(enriched)
    return results


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = partitioned_etl_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 61,
            },
            indent=2,
        )
    )
