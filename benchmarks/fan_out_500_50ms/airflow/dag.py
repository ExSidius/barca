"""Airflow DAG: 500 independent tasks x 50ms each."""

import time
from datetime import datetime

from airflow.decorators import dag, task


@task
def work_item(i: int) -> dict:
    time.sleep(0.05)
    return {"i": i, "status": "ok"}


@dag(
    dag_id="fan_out_500_50ms",
    start_date=datetime(2024, 1, 1),
    schedule=None,
    catchup=False,
)
def fan_out_dag():
    [work_item(i) for i in range(500)]


fan_out_dag()
