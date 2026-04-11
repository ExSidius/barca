"""Basic barca example — demonstrates every major feature.

Showcases:

- Bare ``@asset`` (default ``freshness=Always()``)
- ``@asset(inputs=...)`` with upstream dependencies
- ``freshness=Manual()`` for on-demand assets
- ``freshness=Schedule("*/5 * * * *")`` for cron-driven assets
- ``@asset @sink(path)`` for writing outputs to files via fsspec
- ``@sensor(freshness=Schedule(...))`` for observing external state
- ``@effect`` for standalone side effects
- ``@asset(partitions=...)`` with static partitions
- Partition inheritance — downstream assets auto-inherit upstream partitions
- ``collect(asset)`` for aggregating all partitions of an upstream
"""

import time

from barca import (
    Always,
    Manual,
    Schedule,
    asset,
    collect,
    effect,
    partitions,
    sensor,
    sink,
)

# ---------------------------------------------------------------------------
# Workflow 1: Single assets, no inputs
# ---------------------------------------------------------------------------


@asset
def bare_asset() -> dict:
    """Bare @asset — default freshness is Always."""
    return {"bare": True}


@asset()
def hello_world() -> dict:
    return {"message": "Hello from barca!"}


@asset()
def greeting() -> str:
    return "Hello from Barca!"


@asset(freshness=Manual())
def manual_only() -> dict:
    """Manual assets never auto-materialise — only via `barca assets refresh`."""
    return {"manual": True, "ran_at": time.time()}


@asset(freshness=Schedule("0 */6 * * *"))
def six_hourly() -> dict:
    """Runs every 6 hours when `barca run` is active."""
    return {"schedule": "6h", "ts": time.time()}


# ---------------------------------------------------------------------------
# Workflow 2: Asset with upstream inputs
# ---------------------------------------------------------------------------


@asset()
def fruit() -> str:
    return "banana"


@asset(inputs={"fruit": fruit})
def uppercased(fruit: str) -> str:
    return fruit.upper()


# ---------------------------------------------------------------------------
# Workflow 3: Sinks — stack @sink decorators on an @asset
# ---------------------------------------------------------------------------


@asset()
@sink("tmp/greeting.json", serializer="json")
@sink("tmp/greeting.txt", serializer="text")
def greeting_for_world() -> dict:
    """Every materialisation writes to both sinks via fsspec."""
    return {"hi": "world", "lang": "en"}


# ---------------------------------------------------------------------------
# Workflow 4: Partitioned assets + inheritance
# ---------------------------------------------------------------------------


@asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
def fetch_prices(ticker: str) -> dict:
    return {"ticker": ticker, "price": len(ticker) * 100}


# This downstream auto-inherits fetch_prices' 3 partitions — runs 1:1.
@asset(inputs={"price": fetch_prices})
def normalised_price(price: dict) -> dict:
    return {"ticker": price["ticker"], "normalized": price["price"] / 100.0}


# This downstream uses collect() to consume ALL partitions at once.
@asset(inputs={"prices": collect(fetch_prices)})
def price_summary(prices: dict) -> dict:
    """``prices`` is ``dict[tuple, T]`` — each partition's output keyed by tuple."""
    total = sum(v["price"] for v in prices.values())
    return {"tickers": sorted(k[0] for k in prices.keys()), "total": total}


# ---------------------------------------------------------------------------
# Workflow 5: Large partition set
# ---------------------------------------------------------------------------


@asset(partitions={"key": partitions([f"p{i:05d}" for i in range(10000)])})
def wide_asset(key: str) -> dict:
    return {"key": key, "index": int(key[1:])}


# ---------------------------------------------------------------------------
# Workflow 6: Sensors and effects
# ---------------------------------------------------------------------------


@sensor(freshness=Schedule("*/5 * * * *"))
def heartbeat_sensor():
    """Fires every 5 minutes; always reports an update."""
    return (True, {"ts": time.time(), "healthy": True})


@asset(inputs={"tick": heartbeat_sensor}, freshness=Always())
def last_heartbeat_seen(tick):
    """Sensor inputs are the full ``(update_detected, output)`` tuple."""
    update_detected, payload = tick
    return {"saw_update": update_detected, "last_ts": payload.get("ts")}


@effect(inputs={"summary": price_summary}, freshness=Always())
def log_summary(summary):
    """Runs as a side effect whenever price_summary materialises."""
    print(f"[barca effect] price summary: {summary}")
