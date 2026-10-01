from prefect import flow, task
from prefect.task_runners import ConcurrentTaskRunner
import os

# Matches barca's pool_size and dagster's max_concurrent for this benchmark run
# (see benchmarks/lib/env.sh) so no framework gets more/fewer workers than another.
BENCH_WORKERS = int(os.environ.get("BARCA_BENCH_WORKERS", "16"))


@task
def work(i: int) -> dict:
    return {"i": i}


@flow(task_runner=ConcurrentTaskRunner(max_workers=BENCH_WORKERS))
def parallel_flow():
    n = int(os.environ.get("BENCH_N", "10"))
    futures = work.map(list(range(n)))
    return [f.result() for f in futures]


if __name__ == "__main__":
    parallel_flow()
