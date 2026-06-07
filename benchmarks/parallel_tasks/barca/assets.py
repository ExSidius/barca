from functools import partial
from barca import task, parallel


@task()
def work(i: int) -> dict:
    """Minimal work unit — measures pure dispatch overhead."""
    return {"i": i}


@task()
def fan_out_10() -> list:
    return parallel(*(partial(work, i) for i in range(10)))


@task()
def fan_out_50() -> list:
    return parallel(*(partial(work, i) for i in range(50)))


@task()
def fan_out_100() -> list:
    return parallel(*(partial(work, i) for i in range(100)))
