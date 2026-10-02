"""The built-in manual (`barca docs`) must stay accurate.

Every Python example in a topic parses, every pipeline-shaped example is discovered by
`barca list`, and the runnable examples execute and produce what the text claims. If you change
CLI flags, decorators or output behavior, update crates/barca-cli/docs/ and these tests fail
until the manual matches again.
"""

import ast
import json
import re
import subprocess
from pathlib import Path

import pytest

from barca.api import _find_binary

FENCE = re.compile(r"^```(\w*)\n(.*?)^```", re.S | re.M)


def blocks(body: str, lang: str) -> list[str]:
    return [m.group(2) for m in FENCE.finditer(body) if m.group(1) == lang]


def is_pipeline(code: str) -> bool:
    return "from barca import" in code and any(d in code for d in ("@asset", "@task", "@sensor"))


@pytest.fixture(scope="module")
def binary() -> str:
    return _find_binary()


@pytest.fixture(scope="module")
def topics(binary) -> dict[str, str]:
    out = subprocess.run(
        [binary, "docs", "--all", "--json"], capture_output=True, text=True, check=True
    )
    return {t["name"]: t["content"] for t in json.loads(out.stdout)["topics"]}


def barca(binary: str, cwd: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run([binary, *args], cwd=cwd, capture_output=True, text=True)


def result(proc: subprocess.CompletedProcess) -> dict:
    assert proc.returncode == 0, f"exit {proc.returncode}\nstderr:\n{proc.stderr}"
    out = proc.stdout.strip()
    try:
        return json.loads(out)  # pretty-printed (plan, list, docs, history, stats)
    except json.JSONDecodeError:
        return json.loads(out.splitlines()[-1])  # get/run: user prints may precede the JSON line


def write_example(topics: dict[str, str], name: str, cwd: Path) -> Path:
    code = blocks(topics[name], "python")[0]
    path = cwd / "pipeline.py"
    path.write_text(code)
    return path


# ─── Structure ────────────────────────────────────────────────────────────────


def test_every_python_block_parses(topics):
    for name, body in topics.items():
        for i, code in enumerate(blocks(body, "python")):
            try:
                ast.parse(code)
            except SyntaxError as e:
                pytest.fail(f"docs topic '{name}', python block {i}: {e}")


def test_every_pipeline_example_is_discovered_by_list(binary, topics, tmp_path):
    checked = 0
    for name, body in topics.items():
        for i, code in enumerate(blocks(body, "python")):
            if not is_pipeline(code):
                continue
            f = tmp_path / f"{name.replace('/', '_')}_{i}.py"
            f.write_text(code)
            proc = barca(binary, tmp_path, "list", str(f), "--json")
            nodes = result(proc)
            assert nodes, f"docs topic '{name}', block {i}: `barca list` found no nodes"
            checked += 1
    assert checked >= 8, "expected the manual to contain many pipeline examples"


def test_docs_command_surface(binary, tmp_path):
    index = result(barca(binary, tmp_path, "docs", "--json"))
    names = [t["name"] for t in index["topics"]]
    assert {"overview", "types", "cache", "agents", "examples/duckdb"} <= set(names)
    one = result(barca(binary, tmp_path, "docs", "types", "--json"))
    assert one["name"] == "types" and one["content"].startswith("# ")
    bad = barca(binary, tmp_path, "docs", "typs")
    assert bad.returncode == 2 and "Did you mean: types" in bad.stderr
    assert bad.stdout == ""


# ─── Runnable examples ────────────────────────────────────────────────────────


def test_example_duckdb_dag(binary, topics, tmp_path):
    pytest.importorskip("duckdb")
    write_example(topics, "examples/duckdb", tmp_path)
    out = result(barca(binary, tmp_path, "get", "top_region", "pipeline.py"))
    assert out["final_output"] == {"region": "EMEA", "orders": 3, "revenue": 465.5}
    arts = tmp_path / ".barca" / "artifacts"
    for node in ("orders", "customers", "orders_enriched", "revenue_by_region"):
        assert list((arts / f"pipeline.py--{node}").glob("*.parquet")), f"{node} not parquet"
    assert list((arts / "pipeline.py--top_region").glob("*.json"))
    # Parquet steps return a pointer on stdout, as the manual says.
    ptr = result(barca(binary, tmp_path, "get", "revenue_by_region", "pipeline.py"))
    assert ptr["final_output"]["_barca_artifact"]["format"] == "parquet"


def test_example_partitions(binary, topics, tmp_path):
    write_example(topics, "examples/partitions", tmp_path)
    plan = result(barca(binary, tmp_path, "plan", "pipeline.py"))
    steps = [s for p in plan["phases"] for st in p["streams"] for s in st["steps"]]
    assert steps.count("pipeline.py:sales") == 3
    first = result(barca(binary, tmp_path, "get", "summary", "pipeline.py"))
    assert first["steps_executed"] == 4
    assert first["final_output"] == {"regions": 3, "total": 400}
    second = result(barca(binary, tmp_path, "get", "summary", "pipeline.py"))
    assert second["steps_executed"] == 0  # every partition and the fan-in come from cache
    assert (tmp_path / ".barca" / "artifacts" / "pipeline.py--sales_region_emea").is_dir()


def test_example_deploy_task(binary, topics, tmp_path):
    write_example(topics, "examples/deploy-task", tmp_path)
    assert result(barca(binary, tmp_path, "run", "deploy", "pipeline.py"))["steps_executed"] == 2
    assert result(barca(binary, tmp_path, "run", "deploy", "pipeline.py"))["steps_executed"] == 1
    refreshed = barca(binary, tmp_path, "run", "deploy", "pipeline.py", "--refresh", "model")
    assert result(refreshed)["steps_executed"] == 2
    wrong = barca(binary, tmp_path, "get", "deploy", "pipeline.py")
    assert wrong.returncode == 2 and "barca run" in wrong.stderr


def test_types_topic_example_reads_one_parquet_two_ways(binary, topics, tmp_path, monkeypatch):
    pytest.importorskip("duckdb")
    pytest.importorskip("polars")
    pytest.importorskip("pyarrow")
    write_example(topics, "types", tmp_path)
    total = result(barca(binary, tmp_path, "get", "total", "pipeline.py"))
    assert total["final_output"] == {"total": 9.5}
    pointer = result(barca(binary, tmp_path, "get", "as_polars", "pipeline.py"))
    assert pointer["final_output"]["_barca_artifact"]["format"] == "parquet"

    import barca as barca_api

    monkeypatch.chdir(tmp_path)
    df = barca_api.get("as_polars", "pipeline.py")  # the Python API loads parquet for you
    assert df["doubled"].tolist() == [19.0]


def test_tasks_topic_example(binary, topics, tmp_path):
    write_example(topics, "tasks", tmp_path)
    for target in ("send_email", "notify"):
        assert result(barca(binary, tmp_path, "run", target, "pipeline.py"))["run_id"]


# ─── Machine-readable inspection commands ─────────────────────────────────────


def test_json_inspection_commands(binary, tmp_path):
    (tmp_path / "pipeline.py").write_text(
        "from barca import asset\n\n\n"
        "@asset()\ndef numbers() -> list:\n    return [1, 2, 3]\n\n\n"
        '@asset(inputs={"nums": numbers})\ndef total(nums: list) -> dict:\n'
        '    return {"total": sum(nums)}\n'
    )
    nodes = result(barca(binary, tmp_path, "list", "pipeline.py", "--json"))
    by_id = {n["id"]: n for n in nodes}
    assert by_id["pipeline.py:total"]["inputs"] == ["pipeline.py:numbers"]
    assert by_id["pipeline.py:numbers"]["kind"] == "asset"

    assert result(barca(binary, tmp_path, "get", "total", "pipeline.py"))["steps_executed"] == 2
    runs = result(barca(binary, tmp_path, "history", "--json"))
    assert isinstance(runs, list) and runs[0]["status"] == "success"
    stats = result(barca(binary, tmp_path, "stats", "total", "pipeline.py", "--json"))
    assert stats["node_id"] == "pipeline.py:total"
    assert barca(binary, tmp_path, "get", "total", "pipeline.py").stdout.count("\n") == 1
