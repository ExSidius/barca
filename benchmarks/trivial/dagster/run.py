"""Dagster trivial benchmark — materialize a single asset."""

import json
import time

from dagster import asset, materialize


@asset
def single_asset():
    return {"status": "ok"}


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = materialize([single_asset])
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 1,
                "success": result.success,
            },
            indent=2,
        )
    )
