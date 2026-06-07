from dagster import op, job


@op
def work(context, i: int) -> dict:
    return {"i": i}


@op
def fan_out(context) -> list:
    import os

    n = int(os.environ.get("BENCH_N", "10"))
    return [work(context, i) for i in range(n)]


@job
def parallel_job():
    fan_out()


if __name__ == "__main__":
    parallel_job.execute_in_process()
