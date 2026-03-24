import time

from barca import asset, partitions


@asset()
def single_asset() -> dict:
    """A single trivial asset for cold start benchmarking."""
    return {"status": "ok"}


@asset(partitions={"i": partitions([str(i) for i in range(500)])})
def parallel_500(i: str) -> dict:
    """500 partitions, each doing 50ms of simulated work (I/O, API call, etc)."""
    time.sleep(0.05)
    return {"i": int(i), "status": "ok"}
