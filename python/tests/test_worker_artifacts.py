"""Integration tests for barca._worker with artifact-based I/O.

These tests exercise the full worker flow: execute a user function,
serialize the result to an artifact file, emit a v2 protocol receipt,
and resolve cross-phase artifact references as inputs.
"""

import json
import textwrap
from pathlib import Path

import pandas as pd

from barca._artifacts import deserialize


# ─── Helpers ──────────────────────────────────────────────────────────────────


def _make_batch(steps, provided_inputs=None, artifact_dir=None):
    """Build a batch dict matching what Rust sends to the worker."""
    return {
        "stream_id": "test-w0",
        "artifact_dir": str(artifact_dir) if artifact_dir else None,
        "provided_inputs": provided_inputs or {},
        "steps": steps,
    }


def _make_step(
    node_id,
    function_name,
    source_file,
    kind="asset",
    inputs=None,
    partition=None,
    serializer=None,
):
    step = {
        "node_id": node_id,
        "kind": kind,
        "function_name": function_name,
        "source_file": source_file,
        "inputs": inputs or {},
    }
    if partition:
        step["partition"] = partition
    if serializer:
        step["serializer"] = serializer
    return step


def _write_module(tmp_path, filename, code):
    """Write a Python module file and return its path."""
    p = tmp_path / filename
    p.write_text(textwrap.dedent(code))
    return str(p)


def _run_batch_capture_protocol(batch):
    """Run a batch through the worker and capture protocol messages from stderr.

    Returns (protocol_messages: list[dict], error_lines: list[str]).
    """
    import io
    from contextlib import redirect_stderr

    from barca._worker import run_batch

    buf = io.StringIO()
    with redirect_stderr(buf):
        run_batch(batch)

    protocol_msgs = []
    error_lines = []
    for line in buf.getvalue().splitlines():
        if line.startswith("BARCA:2:"):
            json_str = line[len("BARCA:2:") :]
            protocol_msgs.append(json.loads(json_str))
        elif line.strip():
            error_lines.append(line)

    return protocol_msgs, error_lines


# ─── Protocol emission tests ─────────────────────────────────────────────────


class TestProtocolEmission:
    """Worker emits v2 receipt with artifact metadata."""

    def test_dict_emits_artifact_receipt(self, tmp_path):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()
        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def my_func():
                return {"x": 1}
        """,
        )
        batch = _make_batch(
            [_make_step("mod.py:my_func", "my_func", src)],
            artifact_dir=artifact_dir,
        )
        msgs, errors = _run_batch_capture_protocol(batch)

        assert len(msgs) == 1
        msg = msgs[0]
        assert msg["type"] == "result"
        assert msg["node_id"] == "mod.py:my_func"
        assert "artifact" in msg
        assert "path" in msg["artifact"]
        assert "format" in msg["artifact"]
        assert "size_bytes" in msg["artifact"]
        assert msg["artifact"]["size_bytes"] > 0
        assert "elapsed" in msg

    def test_artifact_file_exists(self, tmp_path):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()
        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def my_func():
                return {"x": 1}
        """,
        )
        batch = _make_batch(
            [_make_step("mod.py:my_func", "my_func", src)],
            artifact_dir=artifact_dir,
        )
        msgs, _ = _run_batch_capture_protocol(batch)

        artifact_path = Path(msgs[0]["artifact"]["path"])
        assert artifact_path.exists()

    def test_artifact_deserializes_correctly(self, tmp_path):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()
        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def my_func():
                return {"x": 1, "y": [2, 3]}
        """,
        )
        batch = _make_batch(
            [_make_step("mod.py:my_func", "my_func", src)],
            artifact_dir=artifact_dir,
        )
        msgs, _ = _run_batch_capture_protocol(batch)

        art = msgs[0]["artifact"]
        result = deserialize(Path(art["path"]), art["format"])
        assert result == {"x": 1, "y": [2, 3]}


# ─── Various output types ────────────────────────────────────────────────────


class TestOutputTypes:
    """Different return types produce correct artifact formats."""

    def _run_func(self, tmp_path, code, func_name="my_func"):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()
        src = _write_module(tmp_path, "mod.py", code)
        batch = _make_batch(
            [_make_step(f"mod.py:{func_name}", func_name, src)],
            artifact_dir=artifact_dir,
        )
        msgs, errors = _run_batch_capture_protocol(batch)
        assert len(msgs) == 1, f"Expected 1 msg, got {len(msgs)}. Errors: {errors}"
        art = msgs[0]["artifact"]
        return art, deserialize(Path(art["path"]), art["format"])

    def test_int(self, tmp_path):
        art, result = self._run_func(
            tmp_path,
            """
            def my_func():
                return 42
        """,
        )
        assert art["format"] == "json"
        assert result == 42

    def test_dict(self, tmp_path):
        art, result = self._run_func(
            tmp_path,
            """
            def my_func():
                return {"key": "value"}
        """,
        )
        assert art["format"] == "json"
        assert result == {"key": "value"}

    def test_list(self, tmp_path):
        art, result = self._run_func(
            tmp_path,
            """
            def my_func():
                return [1, 2, 3]
        """,
        )
        assert art["format"] == "json"
        assert result == [1, 2, 3]

    def test_set(self, tmp_path):
        art, result = self._run_func(
            tmp_path,
            """
            def my_func():
                return {1, 2, 3}
        """,
        )
        assert art["format"] == "pickle"
        assert result == {1, 2, 3}

    def test_custom_object(self, tmp_path):
        art, result = self._run_func(
            tmp_path,
            """
            class Point:
                def __init__(self, x, y):
                    self.x = x
                    self.y = y

            def my_func():
                return Point(3, 4)
        """,
        )
        assert art["format"] == "pickle"
        assert result.x == 3
        assert result.y == 4

    def test_pandas_dataframe(self, tmp_path):
        art, result = self._run_func(
            tmp_path,
            """
            import pandas as pd

            def my_func():
                return pd.DataFrame({"a": [1, 2], "b": [3, 4]})
        """,
        )
        assert art["format"] == "parquet"
        assert list(result.columns) == ["a", "b"]
        assert len(result) == 2

    def test_sensor_unpacking(self, tmp_path):
        """Sensors return (updated, data) — only data gets serialized."""
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()
        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def my_sensor():
                return (True, {"data": 1})
        """,
        )
        batch = _make_batch(
            [_make_step("mod.py:my_sensor", "my_sensor", src, kind="sensor")],
            artifact_dir=artifact_dir,
        )
        msgs, _ = _run_batch_capture_protocol(batch)
        art = msgs[0]["artifact"]
        result = deserialize(Path(art["path"]), art["format"])
        assert result == {"data": 1}


# ─── Cross-phase input resolution ────────────────────────────────────────────


class TestCrossPhaseInputs:
    """provided_inputs with artifact refs are deserialized for downstream steps."""

    def test_json_artifact_as_input(self, tmp_path):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()

        # Pre-create an artifact that represents an upstream output.
        from barca._artifacts import serialize as art_serialize

        upstream_path = artifact_dir / "upstream.json"
        art_serialize({"x": 10}, upstream_path, "json")

        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def downstream(data):
                return data["x"] + 1
        """,
        )
        batch = _make_batch(
            [
                _make_step(
                    "mod.py:downstream",
                    "downstream",
                    src,
                    inputs={"data": "mod.py:upstream"},
                )
            ],
            provided_inputs={
                "mod.py:upstream": {
                    "path": str(upstream_path),
                    "format": "json",
                },
            },
            artifact_dir=artifact_dir,
        )
        msgs, errors = _run_batch_capture_protocol(batch)
        assert len(msgs) == 1, f"Errors: {errors}"
        result = deserialize(
            Path(msgs[0]["artifact"]["path"]), msgs[0]["artifact"]["format"]
        )
        assert result == 11

    def test_parquet_artifact_as_input(self, tmp_path):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()

        from barca._artifacts import serialize as art_serialize

        upstream_path = artifact_dir / "upstream.parquet"
        df = pd.DataFrame({"val": [10, 20, 30]})
        art_serialize(df, upstream_path, "parquet")

        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def downstream(data):
                return int(data["val"].sum())
        """,
        )
        batch = _make_batch(
            [
                _make_step(
                    "mod.py:downstream",
                    "downstream",
                    src,
                    inputs={"data": "mod.py:upstream"},
                )
            ],
            provided_inputs={
                "mod.py:upstream": {
                    "path": str(upstream_path),
                    "format": "parquet",
                },
            },
            artifact_dir=artifact_dir,
        )
        msgs, errors = _run_batch_capture_protocol(batch)
        assert len(msgs) == 1, f"Errors: {errors}"
        result = deserialize(
            Path(msgs[0]["artifact"]["path"]), msgs[0]["artifact"]["format"]
        )
        assert result == 60

    def test_pickle_artifact_as_input(self, tmp_path):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()

        from barca._artifacts import serialize as art_serialize

        upstream_path = artifact_dir / "upstream.pkl"
        art_serialize({1, 2, 3}, upstream_path, "pickle")

        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def downstream(data):
                return len(data)
        """,
        )
        batch = _make_batch(
            [
                _make_step(
                    "mod.py:downstream",
                    "downstream",
                    src,
                    inputs={"data": "mod.py:upstream"},
                )
            ],
            provided_inputs={
                "mod.py:upstream": {
                    "path": str(upstream_path),
                    "format": "pickle",
                },
            },
            artifact_dir=artifact_dir,
        )
        msgs, errors = _run_batch_capture_protocol(batch)
        assert len(msgs) == 1, f"Errors: {errors}"
        result = deserialize(
            Path(msgs[0]["artifact"]["path"]), msgs[0]["artifact"]["format"]
        )
        assert result == 3


# ─── Serializer override per step ─────────────────────────────────────────────


class TestSerializerOverride:
    """Step-level serializer field forces a specific format."""

    def test_force_pickle_on_dict(self, tmp_path):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()
        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def my_func():
                return {"key": "value"}
        """,
        )
        batch = _make_batch(
            [_make_step("mod.py:my_func", "my_func", src, serializer="pickle")],
            artifact_dir=artifact_dir,
        )
        msgs, _ = _run_batch_capture_protocol(batch)
        assert msgs[0]["artifact"]["format"] == "pickle"
        result = deserialize(Path(msgs[0]["artifact"]["path"]), "pickle")
        assert result == {"key": "value"}

    def test_no_serializer_uses_autodetect(self, tmp_path):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()
        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def my_func():
                return {"key": "value"}
        """,
        )
        batch = _make_batch(
            [_make_step("mod.py:my_func", "my_func", src)],
            artifact_dir=artifact_dir,
        )
        msgs, _ = _run_batch_capture_protocol(batch)
        assert msgs[0]["artifact"]["format"] == "json"


# ─── Multi-step batch ─────────────────────────────────────────────────────────


class TestMultiStepBatch:
    """Multiple steps in a single batch — each produces its own artifact."""

    def test_two_steps_same_batch(self, tmp_path):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()
        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def step_a():
                return [1, 2, 3]

            def step_b(data):
                return sum(data)
        """,
        )
        batch = _make_batch(
            [
                _make_step("mod.py:step_a", "step_a", src),
                _make_step(
                    "mod.py:step_b",
                    "step_b",
                    src,
                    inputs={"data": "mod.py:step_a"},
                ),
            ],
            artifact_dir=artifact_dir,
        )
        msgs, errors = _run_batch_capture_protocol(batch)
        assert len(msgs) == 2, f"Errors: {errors}"

        # First step: list
        art_a = msgs[0]["artifact"]
        assert art_a["format"] == "json"
        assert deserialize(Path(art_a["path"]), art_a["format"]) == [1, 2, 3]

        # Second step: int (computed from first)
        art_b = msgs[1]["artifact"]
        assert art_b["format"] == "json"
        assert deserialize(Path(art_b["path"]), art_b["format"]) == 6

    def test_artifacts_have_distinct_paths(self, tmp_path):
        artifact_dir = tmp_path / "artifacts"
        artifact_dir.mkdir()
        src = _write_module(
            tmp_path,
            "mod.py",
            """
            def step_a():
                return 1

            def step_b():
                return 2
        """,
        )
        batch = _make_batch(
            [
                _make_step("mod.py:step_a", "step_a", src),
                _make_step("mod.py:step_b", "step_b", src),
            ],
            artifact_dir=artifact_dir,
        )
        msgs, _ = _run_batch_capture_protocol(batch)
        paths = [m["artifact"]["path"] for m in msgs]
        assert len(set(paths)) == 2, f"Paths should be distinct: {paths}"
