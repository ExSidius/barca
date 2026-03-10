import time

from barca import asset


@asset()
def greeting() -> str:
    return "Hello from Barca!"


@asset()
def slow_computation() -> dict:
    time.sleep(3)
    return {"status": "ok", "count": 42}
