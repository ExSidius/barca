"""Trigger the prefect flow and time it."""

import json
import time

from flow import fan_out_50ms_flow

if __name__ == "__main__":
    t0 = time.perf_counter()
    result = fan_out_50ms_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 500,
                "result": result,
            },
            indent=2,
        )
    )
