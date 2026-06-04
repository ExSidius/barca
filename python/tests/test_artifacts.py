"""Tests for barca._artifacts — format detection, round-trip serialization, path sanitization."""

import datetime

import pandas as pd
import polars as pl

from barca._artifacts import (
    artifact_path,
    deserialize,
    detect_format,
    safe_node_id,
    serialize,
)


class _Point:
    """Module-level class for pickle round-trip tests (local classes can't be pickled)."""

    def __init__(self, x, y):
        self.x = x
        self.y = y

    def __eq__(self, other):
        return isinstance(other, _Point) and self.x == other.x and self.y == other.y


# ─── Format detection ────────────────────────────────────────────────────────


class TestDetectFormat:
    """detect_format(value, explicit=None) → 'json' | 'pickle' | 'parquet'"""

    # --- JSON-serializable primitives ---

    def test_int(self):
        assert detect_format(42) == "json"

    def test_float(self):
        assert detect_format(3.14) == "json"

    def test_bool(self):
        assert detect_format(True) == "json"

    def test_none(self):
        assert detect_format(None) == "json"

    def test_str(self):
        assert detect_format("hello") == "json"

    # --- JSON-serializable containers ---

    def test_simple_dict(self):
        assert detect_format({"key": "value"}) == "json"

    def test_nested_dict(self):
        assert detect_format({"a": {"b": [1, 2, 3]}}) == "json"

    def test_list_of_primitives(self):
        assert detect_format([1, 2, 3]) == "json"

    def test_list_of_dicts(self):
        assert detect_format([{"x": 1}, {"x": 2}]) == "json"

    def test_empty_dict(self):
        assert detect_format({}) == "json"

    def test_empty_list(self):
        assert detect_format([]) == "json"

    # --- Non-JSON-serializable → pickle ---

    def test_set(self):
        assert detect_format({1, 2, 3}) == "pickle"

    def test_bytes(self):
        assert detect_format(b"binary data") == "pickle"

    def test_datetime(self):
        assert detect_format(datetime.datetime(2025, 1, 1)) == "pickle"

    def test_custom_class(self):
        class Foo:
            pass

        assert detect_format(Foo()) == "pickle"

    def test_tuple(self):
        # Tuples are JSON-serializable (as arrays) in json.dumps,
        # but round-trip as lists. If we detect json, that's fine.
        # If pickle, that's also fine. The key is it doesn't crash.
        fmt = detect_format((1, 2, 3))
        assert fmt in ("json", "pickle")

    def test_dict_with_non_string_keys(self):
        # json.dumps accepts int keys (converts to strings), so this is JSON-serializable.
        # However, round-trip is lossy (int keys become string keys).
        # detect_format checks json.dumps succeeds, so this is "json".
        assert detect_format({1: "a", 2: "b"}) == "json"

    def test_dict_with_tuple_keys(self):
        assert detect_format({(1, 2): "a"}) == "pickle"

    # --- DataFrames → parquet ---

    def test_pandas_dataframe(self):
        df = pd.DataFrame({"x": [1, 2, 3]})
        assert detect_format(df) == "parquet"

    def test_polars_dataframe(self):
        df = pl.DataFrame({"x": [1, 2, 3]})
        assert detect_format(df) == "parquet"

    def test_polars_lazyframe(self):
        lf = pl.LazyFrame({"x": [1, 2, 3]})
        assert detect_format(lf) == "parquet"

    def test_pandas_series(self):
        # Series are not DataFrames — should go to pickle
        s = pd.Series([1, 2, 3])
        assert detect_format(s) == "pickle"

    # --- Explicit override wins ---

    def test_override_pickle_on_dict(self):
        assert detect_format({"key": "value"}, explicit="pickle") == "pickle"

    def test_override_json_on_dataframe(self):
        df = pd.DataFrame({"x": [1, 2, 3]})
        assert detect_format(df, explicit="json") == "json"

    def test_override_parquet_on_dict(self):
        assert detect_format({"key": "value"}, explicit="parquet") == "parquet"

    def test_override_pickle_on_int(self):
        assert detect_format(42, explicit="pickle") == "pickle"


# ─── Round-trip serialization ─────────────────────────────────────────────────


class TestRoundTripJSON:
    """serialize → deserialize with JSON format."""

    def test_dict(self, tmp_path):
        value = {"key": "value", "nested": {"a": 1}}
        path = tmp_path / "out.json"
        size = serialize(value, path, "json")
        assert size > 0
        assert path.exists()
        assert deserialize(path, "json") == value

    def test_list_of_mixed_primitives(self, tmp_path):
        value = [1, "two", 3.0, True, None]
        path = tmp_path / "out.json"
        serialize(value, path, "json")
        assert deserialize(path, "json") == value

    def test_nested_dict_with_lists(self, tmp_path):
        value = {
            "users": [
                {"name": "alice", "scores": [10, 20]},
                {"name": "bob", "scores": [30]},
            ]
        }
        path = tmp_path / "out.json"
        serialize(value, path, "json")
        assert deserialize(path, "json") == value

    def test_primitive_int(self, tmp_path):
        path = tmp_path / "out.json"
        serialize(42, path, "json")
        assert deserialize(path, "json") == 42

    def test_primitive_string(self, tmp_path):
        path = tmp_path / "out.json"
        serialize("hello", path, "json")
        assert deserialize(path, "json") == "hello"

    def test_primitive_bool(self, tmp_path):
        path = tmp_path / "out.json"
        serialize(True, path, "json")
        assert deserialize(path, "json") is True

    def test_primitive_none(self, tmp_path):
        path = tmp_path / "out.json"
        serialize(None, path, "json")
        assert deserialize(path, "json") is None


class TestRoundTripPickle:
    """serialize → deserialize with pickle format."""

    def test_set(self, tmp_path):
        value = {1, 2, 3}
        path = tmp_path / "out.pkl"
        size = serialize(value, path, "pickle")
        assert size > 0
        assert deserialize(path, "pickle") == value

    def test_datetime(self, tmp_path):
        value = datetime.datetime(2025, 6, 15, 12, 30, 0)
        path = tmp_path / "out.pkl"
        serialize(value, path, "pickle")
        assert deserialize(path, "pickle") == value

    def test_custom_class(self, tmp_path):
        value = _Point(3, 4)
        path = tmp_path / "out.pkl"
        serialize(value, path, "pickle")
        result = deserialize(path, "pickle")
        assert result == value

    def test_bytes(self, tmp_path):
        value = b"\x00\x01\x02\xff"
        path = tmp_path / "out.pkl"
        serialize(value, path, "pickle")
        assert deserialize(path, "pickle") == value

    def test_dict_with_non_string_keys(self, tmp_path):
        value = {1: "a", (2, 3): "b"}
        path = tmp_path / "out.pkl"
        serialize(value, path, "pickle")
        assert deserialize(path, "pickle") == value

    def test_nested_complex_structure(self, tmp_path):
        value = {"data": {frozenset([1, 2]): [datetime.date(2025, 1, 1)]}}
        path = tmp_path / "out.pkl"
        serialize(value, path, "pickle")
        assert deserialize(path, "pickle") == value


class TestRoundTripParquet:
    """serialize → deserialize with parquet format."""

    def test_pandas_dataframe(self, tmp_path):
        df = pd.DataFrame({"a": [1, 2, 3], "b": ["x", "y", "z"]})
        path = tmp_path / "out.parquet"
        size = serialize(df, path, "parquet")
        assert size > 0
        result = deserialize(path, "parquet")
        pd.testing.assert_frame_equal(result, df)

    def test_polars_dataframe(self, tmp_path):
        df = pl.DataFrame({"a": [1, 2, 3], "b": ["x", "y", "z"]})
        path = tmp_path / "out.parquet"
        serialize(df, path, "parquet")
        result = deserialize(path, "parquet")
        # Deserializing parquet without knowing the original library
        # should return a pandas DataFrame by default (or polars).
        # We just check the data matches.
        if isinstance(result, pl.DataFrame):
            assert result.frame_equal(df)
        else:
            expected_pd = df.to_pandas()
            pd.testing.assert_frame_equal(result, expected_pd)

    def test_pandas_multiple_dtypes(self, tmp_path):
        df = pd.DataFrame(
            {
                "int_col": [1, 2, 3],
                "float_col": [1.1, 2.2, 3.3],
                "str_col": ["a", "b", "c"],
                "bool_col": [True, False, True],
                "dt_col": pd.to_datetime(["2025-01-01", "2025-01-02", "2025-01-03"]),
            }
        )
        path = tmp_path / "out.parquet"
        serialize(df, path, "parquet")
        result = deserialize(path, "parquet")
        pd.testing.assert_frame_equal(result, df)

    def test_polars_lazyframe(self, tmp_path):
        lf = pl.LazyFrame({"a": [1, 2, 3], "b": [4.0, 5.0, 6.0]})
        path = tmp_path / "out.parquet"
        serialize(lf, path, "parquet")
        result = deserialize(path, "parquet")
        # LazyFrames must be collected before writing; result may come back as DataFrame
        assert result is not None

    def test_empty_pandas_dataframe(self, tmp_path):
        df = pd.DataFrame(
            {"a": pd.Series([], dtype="int64"), "b": pd.Series([], dtype="str")}
        )
        path = tmp_path / "out.parquet"
        serialize(df, path, "parquet")
        result = deserialize(path, "parquet")
        assert len(result) == 0
        assert list(result.columns) == ["a", "b"]

    def test_large_pandas_dataframe(self, tmp_path):
        df = pd.DataFrame({"value": range(10_000)})
        path = tmp_path / "out.parquet"
        serialize(df, path, "parquet")
        result = deserialize(path, "parquet")
        pd.testing.assert_frame_equal(result, df)


# ─── Path sanitization ───────────────────────────────────────────────────────


class TestSafeNodeId:
    """safe_node_id(node_id) → filesystem-safe string."""

    def test_simple(self):
        result = safe_node_id("file.py:func")
        assert "/" not in result
        assert ":" not in result
        assert len(result) > 0

    def test_with_partition(self):
        result = safe_node_id("file.py:func[ticker=AAPL]")
        assert "[" not in result
        assert "]" not in result
        assert "=" not in result

    def test_with_multi_dim_partition(self):
        result = safe_node_id("dir/file.py:func[a=1,b=2]")
        assert "/" not in result
        assert "[" not in result
        assert "," not in result

    def test_no_collisions(self):
        ids = [
            "file.py:func",
            "file.py:func2",
            "file.py:func[t=A]",
            "file.py:func[t=B]",
            "other.py:func",
        ]
        safe_ids = [safe_node_id(nid) for nid in ids]
        assert len(set(safe_ids)) == len(ids), f"Collision detected: {safe_ids}"

    def test_deterministic(self):
        nid = "file.py:func[ticker=AAPL]"
        assert safe_node_id(nid) == safe_node_id(nid)


class TestArtifactPath:
    """artifact_path(artifact_dir, node_id, fmt) → Path."""

    def test_json_extension(self, tmp_path):
        p = artifact_path(tmp_path, "file.py:func", "json")
        assert p.suffix == ".json"
        assert p.parent == tmp_path

    def test_pickle_extension(self, tmp_path):
        p = artifact_path(tmp_path, "file.py:func", "pickle")
        assert p.suffix == ".pkl"
        assert p.parent == tmp_path

    def test_parquet_extension(self, tmp_path):
        p = artifact_path(tmp_path, "file.py:func", "parquet")
        assert p.suffix == ".parquet"
        assert p.parent == tmp_path

    def test_uses_safe_node_id(self, tmp_path):
        p = artifact_path(tmp_path, "file.py:func[t=X]", "json")
        # Should not contain raw special chars in the filename
        assert ":" not in p.stem
        assert "[" not in p.stem


# ─── End-to-end: detect + serialize + deserialize ────────────────────────────


class TestEndToEnd:
    """Full flow: detect_format → serialize → deserialize for various types."""

    def _round_trip(self, value, tmp_path, explicit=None):
        fmt = detect_format(value, explicit=explicit)
        path = artifact_path(tmp_path, "test.py:func", fmt)
        serialize(value, path, fmt)
        return deserialize(path, fmt), fmt

    def test_dict_auto(self, tmp_path):
        value = {"key": "value"}
        result, fmt = self._round_trip(value, tmp_path)
        assert fmt == "json"
        assert result == value

    def test_set_auto(self, tmp_path):
        value = {1, 2, 3}
        result, fmt = self._round_trip(value, tmp_path)
        assert fmt == "pickle"
        assert result == value

    def test_pandas_auto(self, tmp_path):
        df = pd.DataFrame({"x": [1, 2]})
        result, fmt = self._round_trip(df, tmp_path)
        assert fmt == "parquet"
        pd.testing.assert_frame_equal(result, df)

    def test_override_forces_format(self, tmp_path):
        value = {"key": "value"}
        result, fmt = self._round_trip(value, tmp_path, explicit="pickle")
        assert fmt == "pickle"
        assert result == value
