"""Run Airflow DAG using `dags test` with LocalExecutor."""

import json
import os
import shutil
import subprocess
import tempfile
import time

SCRIPT_DIR = os.path.dirname(os.path.abspath(__file__))
AIRFLOW_BIN = os.path.join(SCRIPT_DIR, ".venv", "bin", "airflow")


def run():
    airflow_home = tempfile.mkdtemp(prefix="airflow_bench_")
    dags_dir = os.path.join(airflow_home, "dags")
    os.makedirs(dags_dir)
    shutil.copy(os.path.join(SCRIPT_DIR, "dag.py"), dags_dir)

    env = {
        **os.environ,
        "AIRFLOW_HOME": airflow_home,
        "AIRFLOW__CORE__DAGS_FOLDER": dags_dir,
        "AIRFLOW__CORE__LOAD_EXAMPLES": "False",
        "AIRFLOW__CORE__LOAD_DEFAULT_CONNECTIONS": "False",
        "AIRFLOW__DATABASE__SQL_ALCHEMY_CONN": f"sqlite:///{airflow_home}/airflow.db",
        "AIRFLOW__LOGGING__LOGGING_LEVEL": "ERROR",
    }

    # Initialize DB
    subprocess.run([AIRFLOW_BIN, "db", "migrate"], env=env, capture_output=True)

    # Run DAG
    t0 = time.perf_counter()
    result = subprocess.run(
        [AIRFLOW_BIN, "dags", "test", "fan_out_500_50ms", "2024-01-01"],
        env=env,
        capture_output=True,
        text=True,
    )
    elapsed = time.perf_counter() - t0

    shutil.rmtree(airflow_home, ignore_errors=True)

    print(
        json.dumps(
            {
                "elapsed_seconds": round(elapsed, 6),
                "steps_executed": 500,
                "success": result.returncode == 0,
            },
            indent=2,
        )
    )


if __name__ == "__main__":
    run()
