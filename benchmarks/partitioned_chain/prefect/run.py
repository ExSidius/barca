"""Prefect: 3-step pipeline x 50 partitions = 150 tasks.
Each (step, ticker) pair is a separate task, mirroring barca's partitioned chain."""

import hashlib
import json
import time
from prefect import flow, task

TICKERS = [f"T{i:03d}" for i in range(50)]


# --- Step 1: fetch (50 tasks) ---


def _make_fetch(i, ticker):
    @task(name=f"fetch_{i:03d}")
    def _fetch():
        return {"ticker": ticker, "price": len(ticker) * 10 + hash(ticker) % 100}

    return _fetch


fetch_tasks = [_make_fetch(i, t) for i, t in enumerate(TICKERS)]


# --- Step 2: normalize (50 tasks) ---


def _make_normalize(i, ticker):
    @task(name=f"normalize_{i:03d}")
    def _normalize(data):
        return {"ticker": ticker, "normalized": data["price"] / 100.0}

    return _normalize


normalize_tasks = [_make_normalize(i, t) for i, t in enumerate(TICKERS)]


# --- Step 3: score (50 tasks) ---


def _make_score(i, ticker):
    @task(name=f"score_{i:03d}")
    def _score(data):
        h = hashlib.md5(f"{ticker}:{data['normalized']}".encode()).hexdigest()
        return {"ticker": ticker, "score": int(h[:8], 16) / 2**32}

    return _score


score_tasks = [_make_score(i, t) for i, t in enumerate(TICKERS)]


@flow
def partitioned_chain_flow():
    results = []
    for i in range(len(TICKERS)):
        fetched = fetch_tasks[i]()
        normalized = normalize_tasks[i](fetched)
        scored = score_tasks[i](normalized)
        results.append(scored)
    return results


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = partitioned_chain_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 150,
            },
            indent=2,
        )
    )
