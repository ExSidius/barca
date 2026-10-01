"""Tests for barca._transfer — the coordinator's artifact transfer helper.

The helper is driven in-process over a socketpair (the coordinator's end is
`peer`), so the memory:// filesystem is shared with the test. Wire format is
pinned by crates/barca-core/src/protocol.rs (TransferRequest/TransferReply).
"""

import json
import socket
import struct
import threading
import time

import pytest

from barca import _runtime, _storage, _transfer


@pytest.fixture(autouse=True)
def _clean_memory_fs():
    yield
    fs = _storage._fs_cache.get("memory")
    if fs is not None:
        fs.store.clear()


def _send(sock: socket.socket, msg: dict) -> None:
    body = json.dumps(msg).encode()
    sock.sendall(struct.pack(">I", len(body)) + body)


def _recv(sock: socket.socket) -> dict:
    def exact(n: int) -> bytes:
        buf = b""
        while len(buf) < n:
            chunk = sock.recv(n - len(buf))
            if not chunk:
                raise EOFError
            buf += chunk
        return buf

    (n,) = struct.unpack(">I", exact(4))
    return json.loads(exact(n))


class Helper:
    """A running helper plus the coordinator's end of its socket."""

    def __init__(self, **kwargs):
        ours, theirs = socket.socketpair(socket.AF_UNIX, socket.SOCK_STREAM)
        ours.settimeout(10)
        self.peer = ours
        self._saved = _runtime._socket
        kwargs.setdefault("backoff", 0.0)
        self.thread = threading.Thread(target=_transfer.serve, args=(theirs,), kwargs=kwargs)
        self.thread.start()

    def request(self, msg: dict) -> None:
        _send(self.peer, msg)

    def reply(self) -> dict:
        return _recv(self.peer)

    def replies(self, n: int) -> dict[int, dict]:
        out = {}
        for _ in range(n):
            r = self.reply()
            out[r["id"]] = r
        return out

    def close(self) -> None:
        try:
            _send(self.peer, {"type": "shutdown"})
        except OSError:
            pass
        self.thread.join(timeout=10)
        self.peer.close()
        _runtime._socket = self._saved


@pytest.fixture
def helper():
    h = Helper()
    yield h
    h.close()


class TestPutGet:
    def test_put_then_get_round_trips_bytes(self, helper, tmp_path):
        src = tmp_path / "a.json"
        src.write_bytes(b'{"v": 1}')
        helper.request({"type": "put", "id": 1, "local": str(src), "remote": "memory://s/n/h.json"})
        assert helper.reply() == {"type": "done", "id": 1, "size_bytes": 8}
        assert _storage.exists("memory://s/n/h.json")

        dest = tmp_path / "mirror" / "n" / "h.json"
        helper.request(
            {"type": "get", "id": 2, "remote": "memory://s/n/h.json", "local": str(dest)}
        )
        assert helper.reply() == {"type": "done", "id": 2, "size_bytes": 8}
        assert dest.read_bytes() == b'{"v": 1}'

    def test_get_is_atomic_no_temp_left_behind(self, helper, tmp_path):
        src = tmp_path / "a.bin"
        src.write_bytes(b"x" * 4096)
        _storage.put_file(src, "memory://s/a.bin")
        dest = tmp_path / "m" / "a.bin"
        helper.request({"type": "get", "id": 1, "remote": "memory://s/a.bin", "local": str(dest)})
        assert helper.reply()["type"] == "done"
        assert sorted(p.name for p in dest.parent.iterdir()) == ["a.bin"]

    def test_plain_path_store(self, helper, tmp_path):
        src = tmp_path / "a.bin"
        src.write_bytes(b"abc")
        store = tmp_path / "shared" / "default" / "artifacts" / "n" / "h.bin"
        helper.request({"type": "put", "id": 1, "local": str(src), "remote": str(store)})
        assert helper.reply()["type"] == "done"
        assert store.read_bytes() == b"abc"

        back = tmp_path / "back" / "h.bin"
        helper.request({"type": "get", "id": 2, "remote": f"file://{store}", "local": str(back)})
        assert helper.reply()["type"] == "done"
        assert back.read_bytes() == b"abc"


class TestConcurrency:
    def test_many_in_flight_all_answered_by_id(self, tmp_path):
        h = Helper(concurrency=4)
        try:
            for i in range(8):
                src = tmp_path / f"{i}.bin"
                src.write_bytes(bytes([i]) * (i + 1))
                h.request(
                    {
                        "type": "put",
                        "id": 100 + i,
                        "local": str(src),
                        "remote": f"memory://c/{i}.bin",
                    }
                )
            got = h.replies(8)
            assert set(got) == {100 + i for i in range(8)}
            for i in range(8):
                assert got[100 + i] == {"type": "done", "id": 100 + i, "size_bytes": i + 1}
        finally:
            h.close()

    def test_transfers_run_in_parallel(self, tmp_path, monkeypatch):
        """With concurrency N, N slow transfers overlap instead of queueing."""
        real_put = _storage.put_file

        def slow_put(local, dest):
            time.sleep(0.3)
            real_put(local, dest)

        monkeypatch.setattr(_storage, "put_file", slow_put)
        h = Helper(concurrency=4)
        try:
            t0 = time.monotonic()
            for i in range(4):
                src = tmp_path / f"{i}.bin"
                src.write_bytes(b"x")
                h.request({"type": "put", "id": i, "local": str(src), "remote": f"memory://p/{i}"})
            h.replies(4)
            assert time.monotonic() - t0 < 0.9
        finally:
            h.close()


class TestErrors:
    def test_missing_object_get_errors_and_helper_keeps_serving(self, helper, tmp_path):
        helper.request(
            {"type": "get", "id": 1, "remote": "memory://nope/x", "local": str(tmp_path / "x")}
        )
        r = helper.reply()
        assert r["type"] == "error" and r["id"] == 1
        assert "FileNotFoundError" in r["message"]
        assert not (tmp_path / "x").exists()

        src = tmp_path / "ok.bin"
        src.write_bytes(b"ok")
        helper.request({"type": "put", "id": 2, "local": str(src), "remote": "memory://ok/x"})
        assert helper.reply() == {"type": "done", "id": 2, "size_bytes": 2}

    def test_missing_local_put_errors(self, helper, tmp_path):
        helper.request(
            {"type": "put", "id": 9, "local": str(tmp_path / "gone"), "remote": "memory://x/y"}
        )
        r = helper.reply()
        assert r["type"] == "error" and r["id"] == 9

    def test_transient_failure_is_retried(self, tmp_path, monkeypatch):
        real_put = _storage.put_file
        attempts = []

        def flaky_put(local, dest):
            attempts.append(dest)
            if len(attempts) < 3:
                raise ConnectionError("reset by peer")
            real_put(local, dest)

        monkeypatch.setattr(_storage, "put_file", flaky_put)
        h = Helper(retries=3)
        try:
            src = tmp_path / "a"
            src.write_bytes(b"1")
            h.request({"type": "put", "id": 1, "local": str(src), "remote": "memory://r/a"})
            assert h.reply()["type"] == "done"
            assert len(attempts) == 3
        finally:
            h.close()

    def test_retries_exhausted_reports_last_error(self, tmp_path, monkeypatch):
        def always_fail(local, dest):
            raise ConnectionError("unreachable")

        monkeypatch.setattr(_storage, "put_file", always_fail)
        h = Helper(retries=2)
        try:
            src = tmp_path / "a"
            src.write_bytes(b"1")
            h.request({"type": "put", "id": 5, "local": str(src), "remote": "memory://r/a"})
            r = h.reply()
            assert r == {"type": "error", "id": 5, "message": "ConnectionError: unreachable"}
        finally:
            h.close()

    def test_permanent_failure_is_not_retried(self, tmp_path, monkeypatch):
        calls = []

        def denied(local, dest):
            calls.append(1)
            raise PermissionError("denied")

        monkeypatch.setattr(_storage, "put_file", denied)
        h = Helper(retries=5)
        try:
            src = tmp_path / "a"
            src.write_bytes(b"1")
            h.request({"type": "put", "id": 1, "local": str(src), "remote": "memory://r/a"})
            assert h.reply()["type"] == "error"
            assert calls == [1]
        finally:
            h.close()

    def test_unknown_request_type_is_ignored(self, helper, tmp_path):
        helper.request({"type": "bogus", "id": 1})
        src = tmp_path / "a"
        src.write_bytes(b"1")
        helper.request({"type": "put", "id": 2, "local": str(src), "remote": "memory://u/a"})
        assert helper.reply()["id"] == 2


class TestLifecycle:
    def test_shutdown_finishes_in_flight_work(self, tmp_path, monkeypatch):
        real_put = _storage.put_file

        def slow_put(local, dest):
            time.sleep(0.2)
            real_put(local, dest)

        monkeypatch.setattr(_storage, "put_file", slow_put)
        h = Helper()
        src = tmp_path / "a"
        src.write_bytes(b"1")
        h.request({"type": "put", "id": 1, "local": str(src), "remote": "memory://l/a"})
        h.request({"type": "shutdown"})
        assert h.reply() == {"type": "done", "id": 1, "size_bytes": 1}
        h.thread.join(timeout=5)
        assert not h.thread.is_alive()
        assert _storage.exists("memory://l/a")
        h.peer.close()
        _runtime._socket = h._saved

    def test_coordinator_disconnect_exits(self, tmp_path):
        h = Helper()
        h.peer.shutdown(socket.SHUT_RDWR)
        h.thread.join(timeout=5)
        assert not h.thread.is_alive()
        h.peer.close()
        _runtime._socket = h._saved


class TestEntryPoint:
    def test_main_requires_socket(self, monkeypatch, capsys):
        monkeypatch.delenv("BARCA_SOCKET", raising=False)
        assert _transfer.main() == 1
        assert "BARCA_SOCKET" in capsys.readouterr().err
