"""Tests for v0.1.3 reliability features: partial results, timeouts, fan-in cache."""

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


def write_module(tmp_path, filename, code):
    p = tmp_path / filename
    p.write_text(textwrap.dedent(code))
    return str(p)


# ─── Partial result persistence (#38) ─────────────────────────────────────────


class TestPartialResults:
    def test_partial_results_persisted_on_failure(self, tmp_path):
        """When step 2 of 2 fails, step 1's output should still be cached."""
        f = write_module(
            tmp_path,
            "partial.py",
            """
            from barca import asset

            @asset()
            def step_a():
                return {"value": 42}

            @asset(inputs={"data": step_a})
            def step_b(data):
                raise ValueError("intentional failure")
        """,
        )
        # First run should fail
        with pytest.raises(BarcaError) as exc_info:
            barca.run(f)
        assert "ValueError" in str(exc_info.value)

        # step_a's result should be cached — second run of just step_a should be instant
        f2 = write_module(
            tmp_path,
            "partial.py",
            """
            from barca import asset

            @asset()
            def step_a():
                return {"value": 42}
        """,
        )
        result = barca.api._exec(["get", "step_a", f2])
        assert result["steps_executed"] == 0  # cached from the failed run

    def test_error_message_includes_traceback(self, tmp_path):
        """Worker failure should include the Python exception info."""
        f = write_module(
            tmp_path,
            "fail.py",
            """
            from barca import asset

            @asset()
            def explode():
                return 1 / 0
        """,
        )
        with pytest.raises(BarcaError) as exc_info:
            barca.run(f)
        msg = str(exc_info.value)
        assert "ZeroDivisionError" in msg
        assert "explode" in msg


# ─── Timeout enforcement (#42) ────────────────────────────────────────────────


class TestTimeout:
    def test_timeout_kills_hung_function(self, tmp_path):
        """A function exceeding timeout_seconds should raise TimeoutError."""
        f = write_module(
            tmp_path,
            "slow.py",
            """
            import time
            from barca import asset

            @asset(timeout_seconds=1)
            def hang():
                time.sleep(60)
                return {"done": True}
        """,
        )
        with pytest.raises(BarcaError) as exc_info:
            barca.run(f)
        msg = str(exc_info.value)
        assert "TimeoutError" in msg or "timeout" in msg.lower()

    def test_fast_function_within_timeout(self, tmp_path):
        """A function completing within timeout should succeed normally."""
        f = write_module(
            tmp_path,
            "fast.py",
            """
            from barca import asset

            @asset(timeout_seconds=10)
            def quick():
                return {"done": True}
        """,
        )
        result = barca.run(f)
        assert result["final_output"] == {"done": True}


# ─── Fan-in cache correctness (#40) ──────────────────────────────────────────


class TestFanInCache:
    @pytest.mark.xfail(reason="collect() fan-in not fully working in get path yet")
    def test_partitioned_upstream_change_invalidates_collector(self, tmp_path):
        """Changing a partitioned upstream should invalidate the collect() consumer."""
        f = write_module(
            tmp_path,
            "fanin.py",
            """
            from barca import asset, partitions, collect

            @asset(partitions={"key": partitions(["a", "b"])})
            def source(key):
                return {"key": key, "v": 1}

            @asset(inputs={"data": collect(source)})
            def sink(data):
                return {"count": len(data)}
        """,
        )
        # First run: executes all
        r1 = barca.api._exec(["get", "sink", f])
        assert r1["steps_executed"] == 3  # 2 source + 1 sink

        # Second run: fully cached
        r2 = barca.api._exec(["get", "sink", f])
        assert r2["steps_executed"] == 0

        # Change source function → should invalidate both source and sink
        f2 = write_module(
            tmp_path,
            "fanin.py",
            """
            from barca import asset, partitions, collect

            @asset(partitions={"key": partitions(["a", "b"])})
            def source(key):
                return {"key": key, "v": 2}

            @asset(inputs={"data": collect(source)})
            def sink(data):
                return {"count": len(data)}
        """,
        )
        r3 = barca.api._exec(["get", "sink", f2])
        assert r3["steps_executed"] == 3  # all re-execute
