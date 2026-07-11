"""Tests for barca._storage — scheme dispatch, URI helpers, remote round-trips.

Remote behavior is exercised against fsspec's in-memory filesystem
(memory://), which is a process-global singleton — these tests must run
in-process, never through a spawned worker subprocess.
"""

import builtins
import sys

import pytest

from barca import _storage


@pytest.fixture(autouse=True)
def _clean_memory_fs():
    """Reset the memory filesystem and the fs cache between tests."""
    yield
    fs = _storage._fs_cache.get("memory")
    if fs is not None:
        fs.store.clear()


# ─── Scheme detection ────────────────────────────────────────────────────────


class TestIsRemote:
    def test_plain_path(self):
        assert _storage.is_remote("/data/out.parquet") is False

    def test_relative_path(self):
        assert _storage.is_remote("out.parquet") is False

    def test_file_uri_is_local(self):
        assert _storage.is_remote("file:///data/out.parquet") is False

    def test_abfss(self):
        assert _storage.is_remote("abfss://cont@acct.dfs.core.windows.net/x") is True

    def test_s3(self):
        assert _storage.is_remote("s3://bucket/key.pkl") is True

    def test_gs(self):
        assert _storage.is_remote("gs://bucket/key.json") is True

    def test_memory(self):
        assert _storage.is_remote("memory://arts/x.json") is True

    def test_windows_style_colon_not_a_scheme(self):
        # No "://" — not a URI.
        assert _storage.is_remote("C:label.txt") is False


class TestGetFs:
    def test_local_path_rejected(self):
        with pytest.raises(ValueError, match="local path"):
            _storage.get_fs("/data/x.json")

    def test_unknown_scheme(self):
        with pytest.raises(ValueError, match="Unsupported storage scheme 'ftp://'"):
            _storage.get_fs("ftp://host/x.json")

    def test_memory_fs(self):
        fs = _storage.get_fs("memory://arts/x.json")
        assert fs.protocol == "memory" or "memory" in fs.protocol

    def test_cached_per_protocol(self):
        assert _storage.get_fs("memory://a/x") is _storage.get_fs("memory://b/y")

    def test_missing_driver_names_extra(self, monkeypatch):
        """abfss:// without adlfs installed → error names barca[azure]."""
        _storage._fs_cache.pop("abfs", None)
        real_import = builtins.__import__

        def fake_import(name, *args, **kwargs):
            if name == "adlfs" or name.startswith("adlfs."):
                raise ImportError("No module named 'adlfs'")
            return real_import(name, *args, **kwargs)

        monkeypatch.setattr(builtins, "__import__", fake_import)
        monkeypatch.delitem(sys.modules, "adlfs", raising=False)
        with pytest.raises(ImportError, match=r"barca\[azure\]"):
            _storage.get_fs("abfss://cont@acct.dfs.core.windows.net/x.parquet")

    def test_missing_fsspec_names_extra(self, monkeypatch):
        _storage._fs_cache.pop("s3", None)
        real_import = builtins.__import__

        def fake_import(name, *args, **kwargs):
            if name == "fsspec" or name.startswith("fsspec."):
                raise ImportError("No module named 'fsspec'")
            return real_import(name, *args, **kwargs)

        monkeypatch.setattr(builtins, "__import__", fake_import)
        with pytest.raises(ImportError, match=r"barca\[s3\]"):
            _storage.get_fs("s3://bucket/x.pkl")


# ─── URI helpers ─────────────────────────────────────────────────────────────


class TestJoin:
    def test_local_returns_path(self, tmp_path):
        p = _storage.join(tmp_path, "x.json")
        assert p == tmp_path / "x.json"

    def test_uri_join(self):
        assert (
            _storage.join("abfss://cont@acct.dfs.core.windows.net/prefix", "x.parquet")
            == "abfss://cont@acct.dfs.core.windows.net/prefix/x.parquet"
        )

    def test_uri_join_trailing_slash(self):
        assert _storage.join("s3://bucket/prefix/", "x.pkl") == "s3://bucket/prefix/x.pkl"

    def test_uri_double_slash_preserved(self):
        # Proof that pathlib is never applied to URIs (Path would mangle "//").
        joined = _storage.join("memory://arts", "x.json")
        assert joined == "memory://arts/x.json"
        assert "://" in joined


class TestSuffix:
    def test_local(self):
        assert _storage.suffix("/data/out.parquet") == ".parquet"

    def test_uri(self):
        assert _storage.suffix("abfss://cont@acct.dfs.core.windows.net/a/b/model.pkl") == ".pkl"

    def test_uri_with_query_ignored(self):
        assert _storage.suffix("s3://bucket/a/data.json?versionId=3") == ".json"

    def test_no_extension(self):
        assert _storage.suffix("s3://bucket/a/README") == ""

    def test_hidden_file_no_extension(self):
        assert _storage.suffix("/data/.hidden") == ""


# ─── Storage options ─────────────────────────────────────────────────────────


class TestStorageOptions:
    def test_unset(self, monkeypatch):
        monkeypatch.delenv("BARCA_STORAGE_OPTIONS", raising=False)
        assert _storage.storage_options("abfs") == {}

    def test_keyed_by_protocol(self, monkeypatch):
        monkeypatch.setenv(
            "BARCA_STORAGE_OPTIONS",
            '{"abfs": {"account_name": "myacct"}, "s3": {"anon": true}}',
        )
        assert _storage.storage_options("abfs") == {"account_name": "myacct"}
        assert _storage.storage_options("s3") == {"anon": True}
        assert _storage.storage_options("gcs") == {}

    def test_invalid_json(self, monkeypatch):
        monkeypatch.setenv("BARCA_STORAGE_OPTIONS", "{not json")
        with pytest.raises(ValueError, match="not valid JSON"):
            _storage.storage_options("abfs")

    def test_non_object(self, monkeypatch):
        monkeypatch.setenv("BARCA_STORAGE_OPTIONS", "[1, 2]")
        with pytest.raises(ValueError, match="JSON object"):
            _storage.storage_options("abfs")


# ─── Remote round-trip on memory:// ──────────────────────────────────────────


class TestRemoteRoundTrip:
    def test_put_get_exists_size(self, tmp_path):
        src = tmp_path / "payload.bin"
        src.write_bytes(b"x" * 1024)

        dest = "memory://arts/payload.bin"
        assert _storage.exists(dest) is False

        _storage.put_file(src, dest)
        assert _storage.exists(dest) is True
        assert _storage.size(dest) == 1024

        back = tmp_path / "back.bin"
        _storage.get_file(dest, back)
        assert back.read_bytes() == b"x" * 1024

    def test_local_exists_and_size(self, tmp_path):
        f = tmp_path / "x.json"
        f.write_text("{}")
        assert _storage.exists(f) is True
        assert _storage.exists(tmp_path / "missing.json") is False
        assert _storage.size(f) == 2
