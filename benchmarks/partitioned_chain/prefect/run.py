"""Prefect: 3-step pipeline x 50 partitions = 150 tasks.
Each (step, ticker) pair is a separate task, mirroring barca's partitioned chain."""

import hashlib
import json
import os
import time
from prefect import flow, task
from prefect.task_runners import ConcurrentTaskRunner

TICKERS = [f"T{i:03d}" for i in range(50)]

# Matches barca's pool_size and dagster's max_concurrent for this benchmark run
# (see benchmarks/lib/env.sh) so no framework gets more/fewer workers than another.
BENCH_WORKERS = int(os.environ.get("BARCA_BENCH_WORKERS", "16"))


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


@flow(task_runner=ConcurrentTaskRunner(max_workers=BENCH_WORKERS))
def partitioned_chain_flow():
    # Each ticker's 3-step chain is independent of every other ticker's chain
    # (parallel across tickers), but sequential within a chain.
    futures = []
    for i in range(len(TICKERS)):
        fetched = fetch_tasks[i].submit()
        normalized = normalize_tasks[i].submit(fetched)
        scored = score_tasks[i].submit(normalized)
        futures.append(scored)
    return [f.result() for f in futures]


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
