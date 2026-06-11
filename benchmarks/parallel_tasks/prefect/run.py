from prefect import flow, task
import os


@task
def work(i: int) -> dict:
    return {"i": i}


@flow
def parallel_flow():
    n = int(os.environ.get("BENCH_N", "10"))
    futures = work.map(list(range(n)))
    return [f.result() for f in futures]


if __name__ == "__main__":
    parallel_flow()
