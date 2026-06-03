"""Prefect trivial benchmark — run a single task in a flow."""

import json
import time

from prefect import flow, task


@task
def single_asset():
    return {"status": "ok"}


@flow
def bench_flow():
    return single_asset()


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = bench_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 1,
                "result": result,
            },
            indent=2,
        )
    )
