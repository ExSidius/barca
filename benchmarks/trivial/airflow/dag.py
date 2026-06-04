from datetime import datetime
from airflow.decorators import dag, task


@task
def single_asset():
    return {"status": "ok"}


@dag(dag_id="trivial", start_date=datetime(2024, 1, 1), schedule=None, catchup=False)
def trivial_dag():
    single_asset()


trivial_dag()
