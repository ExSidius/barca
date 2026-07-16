"""Prefect: 100 independent tasks (multi-file discovery equivalent)."""

import json
import os
import time
from prefect import flow, task
from prefect.task_runners import ConcurrentTaskRunner

# Matches barca's pool_size and dagster's max_concurrent for this benchmark run
# (see benchmarks/lib/env.sh) so no framework gets more/fewer workers than another.
BENCH_WORKERS = int(os.environ.get("BARCA_BENCH_WORKERS", "16"))


@task
def root_a():
    return {"source": "a", "value": 1}


@task
def root_b():
    return {"source": "b", "value": 2}


@task
def compute_a_00():
    return {"module": 0, "branch": "a", "value": 0**2}


@task
def compute_b_00():
    return {"module": 0, "branch": "b", "value": 0 * 3}


@task
def compute_a_01():
    return {"module": 1, "branch": "a", "value": 1**2}


@task
def compute_b_01():
    return {"module": 1, "branch": "b", "value": 1 * 3}


@task
def compute_a_02():
    return {"module": 2, "branch": "a", "value": 2**2}


@task
def compute_b_02():
    return {"module": 2, "branch": "b", "value": 2 * 3}


@task
def compute_a_03():
    return {"module": 3, "branch": "a", "value": 3**2}


@task
def compute_b_03():
    return {"module": 3, "branch": "b", "value": 3 * 3}


@task
def compute_a_04():
    return {"module": 4, "branch": "a", "value": 4**2}


@task
def compute_b_04():
    return {"module": 4, "branch": "b", "value": 4 * 3}


@task
def compute_a_05():
    return {"module": 5, "branch": "a", "value": 5**2}


@task
def compute_b_05():
    return {"module": 5, "branch": "b", "value": 5 * 3}


@task
def compute_a_06():
    return {"module": 6, "branch": "a", "value": 6**2}


@task
def compute_b_06():
    return {"module": 6, "branch": "b", "value": 6 * 3}


@task
def compute_a_07():
    return {"module": 7, "branch": "a", "value": 7**2}


@task
def compute_b_07():
    return {"module": 7, "branch": "b", "value": 7 * 3}


@task
def compute_a_08():
    return {"module": 8, "branch": "a", "value": 8**2}


@task
def compute_b_08():
    return {"module": 8, "branch": "b", "value": 8 * 3}


@task
def compute_a_09():
    return {"module": 9, "branch": "a", "value": 9**2}


@task
def compute_b_09():
    return {"module": 9, "branch": "b", "value": 9 * 3}


@task
def compute_a_10():
    return {"module": 10, "branch": "a", "value": 10**2}


@task
def compute_b_10():
    return {"module": 10, "branch": "b", "value": 10 * 3}


@task
def compute_a_11():
    return {"module": 11, "branch": "a", "value": 11**2}


@task
def compute_b_11():
    return {"module": 11, "branch": "b", "value": 11 * 3}


@task
def compute_a_12():
    return {"module": 12, "branch": "a", "value": 12**2}


@task
def compute_b_12():
    return {"module": 12, "branch": "b", "value": 12 * 3}


@task
def compute_a_13():
    return {"module": 13, "branch": "a", "value": 13**2}


@task
def compute_b_13():
    return {"module": 13, "branch": "b", "value": 13 * 3}


@task
def compute_a_14():
    return {"module": 14, "branch": "a", "value": 14**2}


@task
def compute_b_14():
    return {"module": 14, "branch": "b", "value": 14 * 3}


@task
def compute_a_15():
    return {"module": 15, "branch": "a", "value": 15**2}


@task
def compute_b_15():
    return {"module": 15, "branch": "b", "value": 15 * 3}


@task
def compute_a_16():
    return {"module": 16, "branch": "a", "value": 16**2}


@task
def compute_b_16():
    return {"module": 16, "branch": "b", "value": 16 * 3}


@task
def compute_a_17():
    return {"module": 17, "branch": "a", "value": 17**2}


@task
def compute_b_17():
    return {"module": 17, "branch": "b", "value": 17 * 3}


@task
def compute_a_18():
    return {"module": 18, "branch": "a", "value": 18**2}


@task
def compute_b_18():
    return {"module": 18, "branch": "b", "value": 18 * 3}


@task
def compute_a_19():
    return {"module": 19, "branch": "a", "value": 19**2}


@task
def compute_b_19():
    return {"module": 19, "branch": "b", "value": 19 * 3}


@task
def compute_a_20():
    return {"module": 20, "branch": "a", "value": 20**2}


@task
def compute_b_20():
    return {"module": 20, "branch": "b", "value": 20 * 3}


@task
def compute_a_21():
    return {"module": 21, "branch": "a", "value": 21**2}


@task
def compute_b_21():
    return {"module": 21, "branch": "b", "value": 21 * 3}


@task
def compute_a_22():
    return {"module": 22, "branch": "a", "value": 22**2}


@task
def compute_b_22():
    return {"module": 22, "branch": "b", "value": 22 * 3}


@task
def compute_a_23():
    return {"module": 23, "branch": "a", "value": 23**2}


@task
def compute_b_23():
    return {"module": 23, "branch": "b", "value": 23 * 3}


@task
def compute_a_24():
    return {"module": 24, "branch": "a", "value": 24**2}


@task
def compute_b_24():
    return {"module": 24, "branch": "b", "value": 24 * 3}


@task
def compute_a_25():
    return {"module": 25, "branch": "a", "value": 25**2}


@task
def compute_b_25():
    return {"module": 25, "branch": "b", "value": 25 * 3}


@task
def compute_a_26():
    return {"module": 26, "branch": "a", "value": 26**2}


@task
def compute_b_26():
    return {"module": 26, "branch": "b", "value": 26 * 3}


@task
def compute_a_27():
    return {"module": 27, "branch": "a", "value": 27**2}


@task
def compute_b_27():
    return {"module": 27, "branch": "b", "value": 27 * 3}


@task
def compute_a_28():
    return {"module": 28, "branch": "a", "value": 28**2}


@task
def compute_b_28():
    return {"module": 28, "branch": "b", "value": 28 * 3}


@task
def compute_a_29():
    return {"module": 29, "branch": "a", "value": 29**2}


@task
def compute_b_29():
    return {"module": 29, "branch": "b", "value": 29 * 3}


@task
def compute_a_30():
    return {"module": 30, "branch": "a", "value": 30**2}


@task
def compute_b_30():
    return {"module": 30, "branch": "b", "value": 30 * 3}


@task
def compute_a_31():
    return {"module": 31, "branch": "a", "value": 31**2}


@task
def compute_b_31():
    return {"module": 31, "branch": "b", "value": 31 * 3}


@task
def compute_a_32():
    return {"module": 32, "branch": "a", "value": 32**2}


@task
def compute_b_32():
    return {"module": 32, "branch": "b", "value": 32 * 3}


@task
def compute_a_33():
    return {"module": 33, "branch": "a", "value": 33**2}


@task
def compute_b_33():
    return {"module": 33, "branch": "b", "value": 33 * 3}


@task
def compute_a_34():
    return {"module": 34, "branch": "a", "value": 34**2}


@task
def compute_b_34():
    return {"module": 34, "branch": "b", "value": 34 * 3}


@task
def compute_a_35():
    return {"module": 35, "branch": "a", "value": 35**2}


@task
def compute_b_35():
    return {"module": 35, "branch": "b", "value": 35 * 3}


@task
def compute_a_36():
    return {"module": 36, "branch": "a", "value": 36**2}


@task
def compute_b_36():
    return {"module": 36, "branch": "b", "value": 36 * 3}


@task
def compute_a_37():
    return {"module": 37, "branch": "a", "value": 37**2}


@task
def compute_b_37():
    return {"module": 37, "branch": "b", "value": 37 * 3}


@task
def compute_a_38():
    return {"module": 38, "branch": "a", "value": 38**2}


@task
def compute_b_38():
    return {"module": 38, "branch": "b", "value": 38 * 3}


@task
def compute_a_39():
    return {"module": 39, "branch": "a", "value": 39**2}


@task
def compute_b_39():
    return {"module": 39, "branch": "b", "value": 39 * 3}


@task
def compute_a_40():
    return {"module": 40, "branch": "a", "value": 40**2}


@task
def compute_b_40():
    return {"module": 40, "branch": "b", "value": 40 * 3}


@task
def compute_a_41():
    return {"module": 41, "branch": "a", "value": 41**2}


@task
def compute_b_41():
    return {"module": 41, "branch": "b", "value": 41 * 3}


@task
def compute_a_42():
    return {"module": 42, "branch": "a", "value": 42**2}


@task
def compute_b_42():
    return {"module": 42, "branch": "b", "value": 42 * 3}


@task
def compute_a_43():
    return {"module": 43, "branch": "a", "value": 43**2}


@task
def compute_b_43():
    return {"module": 43, "branch": "b", "value": 43 * 3}


@task
def compute_a_44():
    return {"module": 44, "branch": "a", "value": 44**2}


@task
def compute_b_44():
    return {"module": 44, "branch": "b", "value": 44 * 3}


@task
def compute_a_45():
    return {"module": 45, "branch": "a", "value": 45**2}


@task
def compute_b_45():
    return {"module": 45, "branch": "b", "value": 45 * 3}


@task
def compute_a_46():
    return {"module": 46, "branch": "a", "value": 46**2}


@task
def compute_b_46():
    return {"module": 46, "branch": "b", "value": 46 * 3}


@task
def compute_a_47():
    return {"module": 47, "branch": "a", "value": 47**2}


@task
def compute_b_47():
    return {"module": 47, "branch": "b", "value": 47 * 3}


@flow(task_runner=ConcurrentTaskRunner(max_workers=BENCH_WORKERS))
def discovery_flow():
    futures = []
    futures.append(root_a.submit())
    futures.append(root_b.submit())
    futures.append(compute_a_00.submit())
    futures.append(compute_b_00.submit())
    futures.append(compute_a_01.submit())
    futures.append(compute_b_01.submit())
    futures.append(compute_a_02.submit())
    futures.append(compute_b_02.submit())
    futures.append(compute_a_03.submit())
    futures.append(compute_b_03.submit())
    futures.append(compute_a_04.submit())
    futures.append(compute_b_04.submit())
    futures.append(compute_a_05.submit())
    futures.append(compute_b_05.submit())
    futures.append(compute_a_06.submit())
    futures.append(compute_b_06.submit())
    futures.append(compute_a_07.submit())
    futures.append(compute_b_07.submit())
    futures.append(compute_a_08.submit())
    futures.append(compute_b_08.submit())
    futures.append(compute_a_09.submit())
    futures.append(compute_b_09.submit())
    futures.append(compute_a_10.submit())
    futures.append(compute_b_10.submit())
    futures.append(compute_a_11.submit())
    futures.append(compute_b_11.submit())
    futures.append(compute_a_12.submit())
    futures.append(compute_b_12.submit())
    futures.append(compute_a_13.submit())
    futures.append(compute_b_13.submit())
    futures.append(compute_a_14.submit())
    futures.append(compute_b_14.submit())
    futures.append(compute_a_15.submit())
    futures.append(compute_b_15.submit())
    futures.append(compute_a_16.submit())
    futures.append(compute_b_16.submit())
    futures.append(compute_a_17.submit())
    futures.append(compute_b_17.submit())
    futures.append(compute_a_18.submit())
    futures.append(compute_b_18.submit())
    futures.append(compute_a_19.submit())
    futures.append(compute_b_19.submit())
    futures.append(compute_a_20.submit())
    futures.append(compute_b_20.submit())
    futures.append(compute_a_21.submit())
    futures.append(compute_b_21.submit())
    futures.append(compute_a_22.submit())
    futures.append(compute_b_22.submit())
    futures.append(compute_a_23.submit())
    futures.append(compute_b_23.submit())
    futures.append(compute_a_24.submit())
    futures.append(compute_b_24.submit())
    futures.append(compute_a_25.submit())
    futures.append(compute_b_25.submit())
    futures.append(compute_a_26.submit())
    futures.append(compute_b_26.submit())
    futures.append(compute_a_27.submit())
    futures.append(compute_b_27.submit())
    futures.append(compute_a_28.submit())
    futures.append(compute_b_28.submit())
    futures.append(compute_a_29.submit())
    futures.append(compute_b_29.submit())
    futures.append(compute_a_30.submit())
    futures.append(compute_b_30.submit())
    futures.append(compute_a_31.submit())
    futures.append(compute_b_31.submit())
    futures.append(compute_a_32.submit())
    futures.append(compute_b_32.submit())
    futures.append(compute_a_33.submit())
    futures.append(compute_b_33.submit())
    futures.append(compute_a_34.submit())
    futures.append(compute_b_34.submit())
    futures.append(compute_a_35.submit())
    futures.append(compute_b_35.submit())
    futures.append(compute_a_36.submit())
    futures.append(compute_b_36.submit())
    futures.append(compute_a_37.submit())
    futures.append(compute_b_37.submit())
    futures.append(compute_a_38.submit())
    futures.append(compute_b_38.submit())
    futures.append(compute_a_39.submit())
    futures.append(compute_b_39.submit())
    futures.append(compute_a_40.submit())
    futures.append(compute_b_40.submit())
    futures.append(compute_a_41.submit())
    futures.append(compute_b_41.submit())
    futures.append(compute_a_42.submit())
    futures.append(compute_b_42.submit())
    futures.append(compute_a_43.submit())
    futures.append(compute_b_43.submit())
    futures.append(compute_a_44.submit())
    futures.append(compute_b_44.submit())
    futures.append(compute_a_45.submit())
    futures.append(compute_b_45.submit())
    futures.append(compute_a_46.submit())
    futures.append(compute_b_46.submit())
    futures.append(compute_a_47.submit())
    futures.append(compute_b_47.submit())
    results = [f.result() for f in futures]
    return results[-1]


if __name__ == "__main__":
    t0 = time.perf_counter()
    result = discovery_flow()
    elapsed = time.perf_counter() - t0
    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 100,
                "result": result,
            },
            indent=2,
        )
    )
