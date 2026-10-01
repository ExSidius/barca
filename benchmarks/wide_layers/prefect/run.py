"""Prefect: wide layers — 3 layers of 20 assets, each layer depends on previous aggregate."""

import json
import math
import os
import time

from prefect import flow, task
from prefect.task_runners import ConcurrentTaskRunner

# Matches barca's pool_size and dagster's max_concurrent for this benchmark run
# (see benchmarks/lib/env.sh) so no framework gets more/fewer workers than another.
BENCH_WORKERS = int(os.environ.get("BARCA_BENCH_WORKERS", "16"))


# ── Layer 0: 20 independent sources ──


@task
def layer0_00():
    return {"layer": 0, "id": 0, "value": math.sin(0 * 0.3) * 100}


@task
def layer0_01():
    return {"layer": 0, "id": 1, "value": math.sin(1 * 0.3) * 100}


@task
def layer0_02():
    return {"layer": 0, "id": 2, "value": math.sin(2 * 0.3) * 100}


@task
def layer0_03():
    return {"layer": 0, "id": 3, "value": math.sin(3 * 0.3) * 100}


@task
def layer0_04():
    return {"layer": 0, "id": 4, "value": math.sin(4 * 0.3) * 100}


@task
def layer0_05():
    return {"layer": 0, "id": 5, "value": math.sin(5 * 0.3) * 100}


@task
def layer0_06():
    return {"layer": 0, "id": 6, "value": math.sin(6 * 0.3) * 100}


@task
def layer0_07():
    return {"layer": 0, "id": 7, "value": math.sin(7 * 0.3) * 100}


@task
def layer0_08():
    return {"layer": 0, "id": 8, "value": math.sin(8 * 0.3) * 100}


@task
def layer0_09():
    return {"layer": 0, "id": 9, "value": math.sin(9 * 0.3) * 100}


@task
def layer0_10():
    return {"layer": 0, "id": 10, "value": math.sin(10 * 0.3) * 100}


@task
def layer0_11():
    return {"layer": 0, "id": 11, "value": math.sin(11 * 0.3) * 100}


@task
def layer0_12():
    return {"layer": 0, "id": 12, "value": math.sin(12 * 0.3) * 100}


@task
def layer0_13():
    return {"layer": 0, "id": 13, "value": math.sin(13 * 0.3) * 100}


@task
def layer0_14():
    return {"layer": 0, "id": 14, "value": math.sin(14 * 0.3) * 100}


@task
def layer0_15():
    return {"layer": 0, "id": 15, "value": math.sin(15 * 0.3) * 100}


@task
def layer0_16():
    return {"layer": 0, "id": 16, "value": math.sin(16 * 0.3) * 100}


@task
def layer0_17():
    return {"layer": 0, "id": 17, "value": math.sin(17 * 0.3) * 100}


@task
def layer0_18():
    return {"layer": 0, "id": 18, "value": math.sin(18 * 0.3) * 100}


@task
def layer0_19():
    return {"layer": 0, "id": 19, "value": math.sin(19 * 0.3) * 100}


# ── Aggregation 0 ──


@task
def agg_0(
    l0_00,
    l0_01,
    l0_02,
    l0_03,
    l0_04,
    l0_05,
    l0_06,
    l0_07,
    l0_08,
    l0_09,
    l0_10,
    l0_11,
    l0_12,
    l0_13,
    l0_14,
    l0_15,
    l0_16,
    l0_17,
    l0_18,
    l0_19,
):
    values = [
        l0_00["value"]
        + l0_01["value"]
        + l0_02["value"]
        + l0_03["value"]
        + l0_04["value"]
        + l0_05["value"]
        + l0_06["value"]
        + l0_07["value"]
        + l0_08["value"]
        + l0_09["value"]
        + l0_10["value"]
        + l0_11["value"]
        + l0_12["value"]
        + l0_13["value"]
        + l0_14["value"]
        + l0_15["value"]
        + l0_16["value"]
        + l0_17["value"]
        + l0_18["value"]
        + l0_19["value"]
    ]
    return {"sum": sum(values), "count": 20}


# ── Layer 1: 20 assets depending on agg_0 ──


@task
def layer1_00(agg):
    return {"layer": 1, "id": 0, "value": agg["sum"] * math.cos(0 * 0.2)}


@task
def layer1_01(agg):
    return {"layer": 1, "id": 1, "value": agg["sum"] * math.cos(1 * 0.2)}


@task
def layer1_02(agg):
    return {"layer": 1, "id": 2, "value": agg["sum"] * math.cos(2 * 0.2)}


@task
def layer1_03(agg):
    return {"layer": 1, "id": 3, "value": agg["sum"] * math.cos(3 * 0.2)}


@task
def layer1_04(agg):
    return {"layer": 1, "id": 4, "value": agg["sum"] * math.cos(4 * 0.2)}


@task
def layer1_05(agg):
    return {"layer": 1, "id": 5, "value": agg["sum"] * math.cos(5 * 0.2)}


@task
def layer1_06(agg):
    return {"layer": 1, "id": 6, "value": agg["sum"] * math.cos(6 * 0.2)}


@task
def layer1_07(agg):
    return {"layer": 1, "id": 7, "value": agg["sum"] * math.cos(7 * 0.2)}


@task
def layer1_08(agg):
    return {"layer": 1, "id": 8, "value": agg["sum"] * math.cos(8 * 0.2)}


@task
def layer1_09(agg):
    return {"layer": 1, "id": 9, "value": agg["sum"] * math.cos(9 * 0.2)}


@task
def layer1_10(agg):
    return {"layer": 1, "id": 10, "value": agg["sum"] * math.cos(10 * 0.2)}


@task
def layer1_11(agg):
    return {"layer": 1, "id": 11, "value": agg["sum"] * math.cos(11 * 0.2)}


@task
def layer1_12(agg):
    return {"layer": 1, "id": 12, "value": agg["sum"] * math.cos(12 * 0.2)}


@task
def layer1_13(agg):
    return {"layer": 1, "id": 13, "value": agg["sum"] * math.cos(13 * 0.2)}


@task
def layer1_14(agg):
    return {"layer": 1, "id": 14, "value": agg["sum"] * math.cos(14 * 0.2)}


@task
def layer1_15(agg):
    return {"layer": 1, "id": 15, "value": agg["sum"] * math.cos(15 * 0.2)}


@task
def layer1_16(agg):
    return {"layer": 1, "id": 16, "value": agg["sum"] * math.cos(16 * 0.2)}


@task
def layer1_17(agg):
    return {"layer": 1, "id": 17, "value": agg["sum"] * math.cos(17 * 0.2)}


@task
def layer1_18(agg):
    return {"layer": 1, "id": 18, "value": agg["sum"] * math.cos(18 * 0.2)}


@task
def layer1_19(agg):
    return {"layer": 1, "id": 19, "value": agg["sum"] * math.cos(19 * 0.2)}


# ── Aggregation 1 ──


@task
def agg_1(
    l1_00,
    l1_01,
    l1_02,
    l1_03,
    l1_04,
    l1_05,
    l1_06,
    l1_07,
    l1_08,
    l1_09,
    l1_10,
    l1_11,
    l1_12,
    l1_13,
    l1_14,
    l1_15,
    l1_16,
    l1_17,
    l1_18,
    l1_19,
):
    values = [
        l1_00["value"]
        + l1_01["value"]
        + l1_02["value"]
        + l1_03["value"]
        + l1_04["value"]
        + l1_05["value"]
        + l1_06["value"]
        + l1_07["value"]
        + l1_08["value"]
        + l1_09["value"]
        + l1_10["value"]
        + l1_11["value"]
        + l1_12["value"]
        + l1_13["value"]
        + l1_14["value"]
        + l1_15["value"]
        + l1_16["value"]
        + l1_17["value"]
        + l1_18["value"]
        + l1_19["value"]
    ]
    return {"sum": sum(values), "count": 20}


# ── Layer 2: 20 assets depending on agg_1 ──


@task
def layer2_00(agg):
    return {"layer": 2, "id": 0, "value": agg["sum"] * math.tan(0 * 0.1 + 0.01)}


@task
def layer2_01(agg):
    return {"layer": 2, "id": 1, "value": agg["sum"] * math.tan(1 * 0.1 + 0.01)}


@task
def layer2_02(agg):
    return {"layer": 2, "id": 2, "value": agg["sum"] * math.tan(2 * 0.1 + 0.01)}


@task
def layer2_03(agg):
    return {"layer": 2, "id": 3, "value": agg["sum"] * math.tan(3 * 0.1 + 0.01)}


@task
def layer2_04(agg):
    return {"layer": 2, "id": 4, "value": agg["sum"] * math.tan(4 * 0.1 + 0.01)}


@task
def layer2_05(agg):
    return {"layer": 2, "id": 5, "value": agg["sum"] * math.tan(5 * 0.1 + 0.01)}


@task
def layer2_06(agg):
    return {"layer": 2, "id": 6, "value": agg["sum"] * math.tan(6 * 0.1 + 0.01)}


@task
def layer2_07(agg):
    return {"layer": 2, "id": 7, "value": agg["sum"] * math.tan(7 * 0.1 + 0.01)}


@task
def layer2_08(agg):
    return {"layer": 2, "id": 8, "value": agg["sum"] * math.tan(8 * 0.1 + 0.01)}


@task
def layer2_09(agg):
    return {"layer": 2, "id": 9, "value": agg["sum"] * math.tan(9 * 0.1 + 0.01)}


@task
def layer2_10(agg):
    return {"layer": 2, "id": 10, "value": agg["sum"] * math.tan(10 * 0.1 + 0.01)}


@task
def layer2_11(agg):
    return {"layer": 2, "id": 11, "value": agg["sum"] * math.tan(11 * 0.1 + 0.01)}


@task
def layer2_12(agg):
    return {"layer": 2, "id": 12, "value": agg["sum"] * math.tan(12 * 0.1 + 0.01)}


@task
def layer2_13(agg):
    return {"layer": 2, "id": 13, "value": agg["sum"] * math.tan(13 * 0.1 + 0.01)}


@task
def layer2_14(agg):
    return {"layer": 2, "id": 14, "value": agg["sum"] * math.tan(14 * 0.1 + 0.01)}


@task
def layer2_15(agg):
    return {"layer": 2, "id": 15, "value": agg["sum"] * math.tan(15 * 0.1 + 0.01)}


@task
def layer2_16(agg):
    return {"layer": 2, "id": 16, "value": agg["sum"] * math.tan(16 * 0.1 + 0.01)}


@task
def layer2_17(agg):
    return {"layer": 2, "id": 17, "value": agg["sum"] * math.tan(17 * 0.1 + 0.01)}


@task
def layer2_18(agg):
    return {"layer": 2, "id": 18, "value": agg["sum"] * math.tan(18 * 0.1 + 0.01)}


@task
def layer2_19(agg):
    return {"layer": 2, "id": 19, "value": agg["sum"] * math.tan(19 * 0.1 + 0.01)}


# ── Final output ──


@task
def final_output(
    l2_00,
    l2_01,
    l2_02,
    l2_03,
    l2_04,
    l2_05,
    l2_06,
    l2_07,
    l2_08,
    l2_09,
    l2_10,
    l2_11,
    l2_12,
    l2_13,
    l2_14,
    l2_15,
    l2_16,
    l2_17,
    l2_18,
    l2_19,
):
    values = [
        l2_00["value"]
        + l2_01["value"]
        + l2_02["value"]
        + l2_03["value"]
        + l2_04["value"]
        + l2_05["value"]
        + l2_06["value"]
        + l2_07["value"]
        + l2_08["value"]
        + l2_09["value"]
        + l2_10["value"]
        + l2_11["value"]
        + l2_12["value"]
        + l2_13["value"]
        + l2_14["value"]
        + l2_15["value"]
        + l2_16["value"]
        + l2_17["value"]
        + l2_18["value"]
        + l2_19["value"]
    ]
    return {"final_sum": round(sum(values), 4), "total_assets": 63}


@flow(task_runner=ConcurrentTaskRunner(max_workers=BENCH_WORKERS))
def wide_layers_flow():
    # Layer 0 (parallel)
    l0 = [
        layer0_00.submit(),
        layer0_01.submit(),
        layer0_02.submit(),
        layer0_03.submit(),
        layer0_04.submit(),
        layer0_05.submit(),
        layer0_06.submit(),
        layer0_07.submit(),
        layer0_08.submit(),
        layer0_09.submit(),
        layer0_10.submit(),
        layer0_11.submit(),
        layer0_12.submit(),
        layer0_13.submit(),
        layer0_14.submit(),
        layer0_15.submit(),
        layer0_16.submit(),
        layer0_17.submit(),
        layer0_18.submit(),
        layer0_19.submit(),
    ]

    # Aggregation 0
    a0 = agg_0.submit(*l0)

    # Layer 1 (parallel, each depends on agg_0)
    l1 = [
        layer1_00.submit(a0),
        layer1_01.submit(a0),
        layer1_02.submit(a0),
        layer1_03.submit(a0),
        layer1_04.submit(a0),
        layer1_05.submit(a0),
        layer1_06.submit(a0),
        layer1_07.submit(a0),
        layer1_08.submit(a0),
        layer1_09.submit(a0),
        layer1_10.submit(a0),
        layer1_11.submit(a0),
        layer1_12.submit(a0),
        layer1_13.submit(a0),
        layer1_14.submit(a0),
        layer1_15.submit(a0),
        layer1_16.submit(a0),
        layer1_17.submit(a0),
        layer1_18.submit(a0),
        layer1_19.submit(a0),
    ]

    # Aggregation 1
    a1 = agg_1.submit(*l1)

    # Layer 2 (parallel, each depends on agg_1)
    l2 = [
        layer2_00.submit(a1),
        layer2_01.submit(a1),
        layer2_02.submit(a1),
        layer2_03.submit(a1),
        layer2_04.submit(a1),
        layer2_05.submit(a1),
        layer2_06.submit(a1),
        layer2_07.submit(a1),
        layer2_08.submit(a1),
        layer2_09.submit(a1),
        layer2_10.submit(a1),
        layer2_11.submit(a1),
        layer2_12.submit(a1),
        layer2_13.submit(a1),
        layer2_14.submit(a1),
        layer2_15.submit(a1),
        layer2_16.submit(a1),
        layer2_17.submit(a1),
        layer2_18.submit(a1),
        layer2_19.submit(a1),
    ]

    # Final output
    result = final_output(*l2)
    return result


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = wide_layers_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 63,
                "result": result,
            },
            indent=2,
        )
    )
