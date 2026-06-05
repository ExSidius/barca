"""Airflow: 10 steps × 1000 partitions using expand() dynamic task mapping (idiomatic).

This is the Airflow 2.3+ way to handle partitioned workloads — define one task,
expand it across inputs. Uses LocalExecutor + PostgreSQL for genuine parallelism.
"""

from datetime import datetime

from airflow.decorators import dag, task

TICKERS = [f"T{i:04d}" for i in range(1000)]


@task
def step_0(ticker):
    return {"ticker": ticker, "v": 0}


@task
def step_1(data, ticker):
    return {"ticker": ticker, "v": data["v"] + 1}


@task
def step_2(data, ticker):
    return {"ticker": ticker, "v": data["v"] + 2}


@task
def step_3(data, ticker):
    return {"ticker": ticker, "v": data["v"] + 3}


@task
def step_4(data, ticker):
    return {"ticker": ticker, "v": data["v"] + 4}


@task
def step_5(data, ticker):
    return {"ticker": ticker, "v": data["v"] + 5}


@task
def step_6(data, ticker):
    return {"ticker": ticker, "v": data["v"] + 6}


@task
def step_7(data, ticker):
    return {"ticker": ticker, "v": data["v"] + 7}


@task
def step_8(data, ticker):
    return {"ticker": ticker, "v": data["v"] + 8}


@task
def step_9(data, ticker):
    return {"ticker": ticker, "v": data["v"] + 9}


@dag(
    dag_id="partitioned_chain",
    start_date=datetime(2024, 1, 1),
    schedule=None,
    catchup=False,
)
def partitioned_chain_dag():
    r0 = step_0.expand(ticker=TICKERS)
    r1 = step_1.expand(data=r0, ticker=TICKERS)
    r2 = step_2.expand(data=r1, ticker=TICKERS)
    r3 = step_3.expand(data=r2, ticker=TICKERS)
    r4 = step_4.expand(data=r3, ticker=TICKERS)
    r5 = step_5.expand(data=r4, ticker=TICKERS)
    r6 = step_6.expand(data=r5, ticker=TICKERS)
    r7 = step_7.expand(data=r6, ticker=TICKERS)
    r8 = step_8.expand(data=r7, ticker=TICKERS)
    step_9.expand(data=r8, ticker=TICKERS)


partitioned_chain_dag()
