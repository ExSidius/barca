import time

from barca import asset, partitions


# --- Workflow 1: Single assets, no inputs ---


@asset()
def hello_world() -> dict:
    return {"message": "Hello from barca!"}


@asset()
def greeting() -> str:
    return "Hello from Barca!"


@asset()
def slow_computation() -> dict:
    time.sleep(3)
    return {"status": "ok", "count": 42}


# --- Workflow 2: Single asset with one upstream input ---


@asset()
def fruit() -> str:
    return "banana"


@asset(inputs={"fruit": fruit})
def uppercased(fruit: str) -> str:
    return fruit.upper()


# --- Workflow 3: Partitioned assets ---


@asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
def fetch_prices(ticker: str) -> dict:
    return {"ticker": ticker, "price": len(ticker) * 100}
