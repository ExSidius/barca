from functools import partial
from barca import task, parallel


@task()
def work(i: int) -> dict:
    """Minimal work unit — measures pure dispatch overhead."""
    return {"i": i}


@task()
def fan_out_10000() -> list:
    return parallel(*(partial(work, i) for i in range(10000)))
