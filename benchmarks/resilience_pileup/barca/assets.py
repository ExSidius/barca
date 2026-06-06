"""Resilience / anti-pileup workload (barca).

8 independent healthy 2-asset chains + one poison chain whose head fails
2 times before succeeding (retries/backoff). Measures whether a single
flaky asset stalls everything behind it. barca should track max(work, backoff),
not the sum, because healthy chains run in-process and the backing-off chain
holds no worker slot.
"""

import os
import time
from pathlib import Path
from barca import asset

_STATE = Path(os.environ.get("BARCA_BENCH_STATE", "/tmp/barca_bench_pileup"))
_STATE.mkdir(parents=True, exist_ok=True)


def _work():
    time.sleep(50 / 1000.0)


@asset()
def h0():
    _work()
    return {"chain": 0}


@asset(inputs={"x": h0})
def t0(x):
    _work()
    return {"chain": x["chain"], "done": True}


@asset()
def h1():
    _work()
    return {"chain": 1}


@asset(inputs={"x": h1})
def t1(x):
    _work()
    return {"chain": x["chain"], "done": True}


@asset()
def h2():
    _work()
    return {"chain": 2}


@asset(inputs={"x": h2})
def t2(x):
    _work()
    return {"chain": x["chain"], "done": True}


@asset()
def h3():
    _work()
    return {"chain": 3}


@asset(inputs={"x": h3})
def t3(x):
    _work()
    return {"chain": x["chain"], "done": True}


@asset()
def h4():
    _work()
    return {"chain": 4}


@asset(inputs={"x": h4})
def t4(x):
    _work()
    return {"chain": x["chain"], "done": True}


@asset()
def h5():
    _work()
    return {"chain": 5}


@asset(inputs={"x": h5})
def t5(x):
    _work()
    return {"chain": x["chain"], "done": True}


@asset()
def h6():
    _work()
    return {"chain": 6}


@asset(inputs={"x": h6})
def t6(x):
    _work()
    return {"chain": x["chain"], "done": True}


@asset()
def h7():
    _work()
    return {"chain": 7}


@asset(inputs={"x": h7})
def t7(x):
    _work()
    return {"chain": x["chain"], "done": True}


@asset(retries=3, retry_backoff=0.5)
def poison_head():
    counter = _STATE / "poison"
    n = int(counter.read_text()) if counter.exists() else 0
    counter.write_text(str(n + 1))
    if n < 2:
        raise RuntimeError(f"poison transient failure {n}")
    return {"recovered_after": n}


@asset(inputs={"x": poison_head})
def poison_tail(x):
    _work()
    return {"ok": True, "after": x["recovered_after"]}
