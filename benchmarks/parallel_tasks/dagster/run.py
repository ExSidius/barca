"""Dagster parallel fan-out benchmark.

Uses @op with DynamicOutput to fan out N tasks in parallel.
"""

import os

from dagster import DynamicOut, DynamicOutput, In, job, op


@op(out=DynamicOut())
def generate_work():
    n = int(os.environ.get("BENCH_N", "10"))
    for i in range(n):
        yield DynamicOutput(i, mapping_key=str(i))


@op
def work(i: int) -> dict:
    return {"i": i}


@op(ins={"results": In(dagster_type=list)})
def collect(results):
    return results


@job
def parallel_job():
    results = generate_work().map(work)
    collect(results.collect())


if __name__ == "__main__":
    parallel_job.execute_in_process()
