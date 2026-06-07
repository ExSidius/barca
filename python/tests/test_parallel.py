"""End-to-end tests for the parallel() primitive.

Tests the full stack: Python parallel() → stderr protocol → Rust sub-worker
dispatch → result collection → response to parent.
"""

import shutil
import textwrap
from pathlib import Path

import pytest

import barca
from barca.api import BarcaError


@pytest.fixture(autouse=True)
def clean_barca_dir():
    barca_dir = Path(".barca")
    if barca_dir.exists():
        shutil.rmtree(barca_dir)
    yield
    if barca_dir.exists():
        shutil.rmtree(barca_dir)


def write_module(tmp_path, filename, body):
    code = textwrap.dedent(body).lstrip("\n")
    p = tmp_path / filename
    p.write_text(code)
    return str(p)


# ─── Happy paths ─────────────────────────────────────────────────────────────


class TestParallelHappyPaths:
    def test_static_two_tasks(self, tmp_path):
        """Two partials, both succeed, results returned in arg order."""
        f = write_module(
            tmp_path,
            "pipeline.py",
            """
            from functools import partial
            from barca import asset, task, parallel

            @asset()
            def config() -> dict:
                return {"env": "prod"}

            @task()
            def deploy_us(cfg: dict) -> dict:
                return {"region": "us", "env": cfg["env"]}

            @task()
            def deploy_eu(cfg: dict) -> dict:
                return {"region": "eu", "env": cfg["env"]}

            @task(inputs={"cfg": config})
            def deploy_all(cfg: dict) -> list:
                return parallel(
                    partial(deploy_us, cfg),
                    partial(deploy_eu, cfg),
                )
            """,
        )
        result = barca.run("deploy_all", f)
        assert isinstance(result, list)
        assert len(result) == 2
        assert result[0]["region"] == "us"
        assert result[1]["region"] == "eu"
        assert result[0]["env"] == "prod"

    def test_parallel_results_ordering(self, tmp_path):
        """Results match argument position, not completion order."""
        f = write_module(
            tmp_path,
            "pipeline.py",
            """
            from functools import partial
            from barca import asset, task, parallel

            @task()
            def slow() -> str:
                import time; time.sleep(0.05)
                return "slow"

            @task()
            def fast() -> str:
                return "fast"

            @task()
            def run_both() -> list:
                return parallel(partial(slow), partial(fast))
            """,
        )
        result = barca.run("run_both", f)
        assert result[0] == "slow"
        assert result[1] == "fast"

    def test_parallel_single_item(self, tmp_path):
        """Single item degenerates to a single worker dispatch."""
        f = write_module(
            tmp_path,
            "pipeline.py",
            """
            from functools import partial
            from barca import task, parallel

            @task()
            def greet(name: str) -> str:
                return f"hello {name}"

            @task()
            def run_one() -> list:
                return parallel(partial(greet, "world"))
            """,
        )
        result = barca.run("run_one", f)
        assert result == ["hello world"]

    def test_parallel_with_kwargs(self, tmp_path):
        """Keyword arguments passed through to sub-tasks."""
        f = write_module(
            tmp_path,
            "pipeline.py",
            """
            from functools import partial
            from barca import task, parallel

            @task()
            def deploy(region: str, replicas: int = 1) -> dict:
                return {"region": region, "replicas": replicas}

            @task()
            def deploy_all() -> list:
                return parallel(
                    partial(deploy, "us", replicas=3),
                    partial(deploy, "eu", replicas=2),
                )
            """,
        )
        result = barca.run("deploy_all", f)
        assert result[0] == {"region": "us", "replicas": 3}
        assert result[1] == {"region": "eu", "replicas": 2}


# ─── Error paths ─────────────────────────────────────────────────────────────


class TestParallelErrors:
    def test_one_branch_fails(self, tmp_path):
        """One branch fails, other succeeds — both results returned."""
        f = write_module(
            tmp_path,
            "pipeline.py",
            """
            from functools import partial
            from barca import task, parallel, ParallelError

            @task()
            def succeed() -> str:
                return "ok"

            @task()
            def fail_task() -> str:
                raise ValueError("boom")

            @task()
            def run_mixed() -> list:
                results = parallel(partial(succeed), partial(fail_task))
                # Convert ParallelError to a serializable dict for output
                return [
                    r if not isinstance(r, ParallelError) else {"error": str(r)}
                    for r in results
                ]
            """,
        )
        result = barca.run("run_mixed", f)
        assert isinstance(result, list)
        assert len(result) == 2
        assert result[0] == "ok"
        assert "error" in result[1] or "boom" in str(result[1])

    def test_all_branches_fail(self, tmp_path):
        """All branches fail — all errors collected."""
        f = write_module(
            tmp_path,
            "pipeline.py",
            """
            from functools import partial
            from barca import task, parallel, ParallelError

            @task()
            def fail_a() -> str:
                raise ValueError("boom a")

            @task()
            def fail_b() -> str:
                raise RuntimeError("boom b")

            @task()
            def run_all_fail() -> list:
                results = parallel(partial(fail_a), partial(fail_b))
                return [
                    r if not isinstance(r, ParallelError) else {"error": str(r)}
                    for r in results
                ]
            """,
        )
        result = barca.run("run_all_fail", f)
        assert isinstance(result, list)
        assert len(result) == 2
        assert "error" in result[0] or "boom" in str(result[0])
        assert "error" in result[1] or "boom" in str(result[1])

    def test_parallel_empty(self, tmp_path):
        """parallel() with no args returns empty list."""
        f = write_module(
            tmp_path,
            "pipeline.py",
            """
            from barca import task, parallel

            @task()
            def run_empty() -> list:
                return parallel()
            """,
        )
        result = barca.run("run_empty", f)
        assert result == []


# ─── Enforcement ─────────────────────────────────────────────────────────────


class TestGetRunEnforcement:
    def test_get_rejects_task(self, tmp_path):
        """barca get on a task gives a clear error."""
        f = write_module(
            tmp_path,
            "pipeline.py",
            """
            from barca import task

            @task()
            def my_task() -> None:
                pass
            """,
        )
        with pytest.raises(BarcaError, match="task.*barca run"):
            barca.get("my_task", f)

    def test_run_rejects_asset(self, tmp_path):
        """barca run on an asset gives a clear error."""
        f = write_module(
            tmp_path,
            "pipeline.py",
            """
            from barca import asset

            @asset()
            def my_asset() -> int:
                return 42
            """,
        )
        with pytest.raises(BarcaError, match="asset.*barca get"):
            barca.run("my_asset", f)


# ─── Ordering-only deps ─────────────────────────────────────────────────────


class TestOrderingOnlyDeps:
    def test_underscore_prefix_skips_deserialization(self, tmp_path):
        """_-prefixed params receive None instead of the artifact value."""
        f = write_module(
            tmp_path,
            "pipeline.py",
            """
            from barca import asset, task

            @asset()
            def setup() -> dict:
                return {"large": "data"}

            @task(inputs={"_setup": setup})
            def deploy(_setup) -> str:
                assert _setup is None, f"expected None, got {_setup}"
                return "deployed"
            """,
        )
        result = barca.run("deploy", f)
        assert result == "deployed"
