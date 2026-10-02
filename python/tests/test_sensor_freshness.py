"""Do `@sensor` outputs reach downstream run hashes? (#151)

The external-freshness idea: a sensor returns a blob's etag / mtime, a bronze asset depends on
the sensor, so when the blob changes in place the bronze asset's run hash changes and it
re-materializes. These tests pin down what barca actually does today.

Finding: sensors always run, but an asset's run hash is built from its definition hash and its
upstream steps' *run hashes*. A sensor has no inputs, so its run hash is a function of its code
alone and is identical on every run, whatever it returns. A downstream asset's run hash therefore
does not change when the sensor's output changes, and the asset is served from cache with the
old data. The pattern does not work yet; `--refresh` is the way to pick up changed external data.
"""

import json
import os
import subprocess
from pathlib import Path

import pytest

from barca.api import _find_binary

PIPELINE = """
from pathlib import Path
from barca import asset, sensor


@sensor()
def blob_etag() -> tuple[bool, str]:
    etag = Path("etag.txt").read_text().strip()   # stands in for an Azure/S3 blob's etag
    return True, etag


@asset(inputs={"etag": blob_etag})
def bronze(etag: str) -> dict:
    return {"etag": etag}
"""


@pytest.fixture()
def project(tmp_path) -> Path:
    (tmp_path / "pipeline.py").write_text(PIPELINE)
    (tmp_path / "etag.txt").write_text("v1")
    return tmp_path


def barca(project: Path, *args: str) -> dict:
    proc = subprocess.run(
        [_find_binary(), *args], cwd=project, env=os.environ, capture_output=True, text=True
    )
    assert proc.returncode == 0, proc.stderr
    return json.loads(proc.stdout.strip().splitlines()[-1])


def steps(result: dict) -> dict:
    return {s["id"].split(":")[-1]: s for s in result["steps"]}


def test_the_sensor_runs_every_time_but_its_run_hash_does_not_depend_on_its_output(project):
    first = steps(barca(project, "get", "bronze", "pipeline.py"))
    (project / "etag.txt").write_text("v2")
    second = steps(barca(project, "get", "bronze", "pipeline.py"))
    assert first["blob_etag"]["status"] == "ran" and second["blob_etag"]["status"] == "ran"
    assert first["blob_etag"]["run_hash"] == second["blob_etag"]["run_hash"]


def test_a_changed_sensor_output_does_not_change_the_downstream_run_hash(project):
    first = barca(project, "get", "bronze", "pipeline.py")
    assert first["final_output"] == {"etag": "v1"}
    (project / "etag.txt").write_text("v2")
    second = barca(project, "get", "bronze", "pipeline.py")
    bronze = steps(second)["bronze"]
    # Known limitation: the asset is served from cache with the old etag.
    assert bronze["run_hash"] == steps(first)["bronze"]["run_hash"]
    assert bronze["status"] == "cached"
    assert second["final_output"] == {"etag": "v1"}


@pytest.mark.xfail(
    strict=True,
    reason="sensor outputs do not participate in downstream run hashes (#151); when this "
    "passes, document the sensor freshness pattern in `barca docs cache`",
)
def test_desired_a_changed_sensor_output_re_materializes_the_downstream_asset(project):
    barca(project, "get", "bronze", "pipeline.py")
    (project / "etag.txt").write_text("v2")
    assert barca(project, "get", "bronze", "pipeline.py")["final_output"] == {"etag": "v2"}
