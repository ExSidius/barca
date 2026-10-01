"""Artifact transfer helper — moves artifact files between local disk and the store.

Spawned once per run by the Rust coordinator (crates/barca-core/src/transfer.rs)
when the artifact store is not the local artifact directory. Workers only ever
read and write local files; this process uploads finished artifacts in the
background and fetches cached artifacts from other machines, so object-store
latency never sits on a step's critical path.

Protocol: the coordinator's Unix socket (BARCA_SOCKET), length-prefixed JSON
frames as in barca._runtime. Requests may be in flight concurrently; each
reply carries the request id.

  → {"type": "put", "id", "local", "remote"}    upload local → remote
  → {"type": "get", "id", "remote", "local"}    download remote → local (atomic)
  → {"type": "shutdown"}                        finish in-flight work, exit
  ← {"type": "done", "id", "size_bytes"}
  ← {"type": "error", "id", "message"}          final — transient errors are retried here

Transfers go through barca._storage, so credentials and BARCA_STORAGE_OPTIONS
behave exactly as they do for workers and the state helper.
"""

import os
import socket
import sys
import tempfile
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from barca import _runtime, _storage

_DEFAULT_CONCURRENCY = 4
_DEFAULT_RETRIES = 3
_DEFAULT_BACKOFF = 0.5

# Failures no retry can fix: missing objects/files, auth, bad config.
_PERMANENT = (
    FileNotFoundError,
    PermissionError,
    IsADirectoryError,
    NotADirectoryError,
    ValueError,
    TypeError,
    ImportError,
)


def _staged_get(remote: str, local: str) -> None:
    """Download into a temp file beside `local`, then rename into place."""
    dest = Path(local)
    dest.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=dest.parent, prefix=f".{dest.name}.", suffix=".tmp")
    os.close(fd)
    try:
        _storage.get_file(remote, tmp)
        os.replace(tmp, dest)
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise


def _transfer(msg: dict) -> int:
    """Perform one put/get; return the transferred file's size."""
    if msg["type"] == "put":
        _storage.put_file(msg["local"], msg["remote"])
        return os.stat(msg["local"]).st_size
    _staged_get(msg["remote"], msg["local"])
    return os.stat(msg["local"]).st_size


def _handle(msg: dict, retries: int, backoff: float) -> None:
    attempt = 0
    while True:
        try:
            size = _transfer(msg)
            reply = {"type": "done", "id": msg["id"], "size_bytes": size}
            break
        except _PERMANENT as exc:
            reply = _error(msg, exc)
            break
        except Exception as exc:
            attempt += 1
            if attempt > retries:
                reply = _error(msg, exc)
                break
            time.sleep(backoff * 2 ** (attempt - 1))
    try:
        _runtime.send_message(reply)
    except OSError:
        pass  # coordinator is gone; nothing to report to


def _error(msg: dict, exc: BaseException) -> dict:
    return {"type": "error", "id": msg["id"], "message": f"{type(exc).__name__}: {exc}"}


def serve(
    sock: socket.socket,
    *,
    concurrency: int = _DEFAULT_CONCURRENCY,
    retries: int = _DEFAULT_RETRIES,
    backoff: float = _DEFAULT_BACKOFF,
) -> None:
    """Serve transfer requests on `sock` until shutdown or disconnect."""
    _runtime._socket = sock
    pool = ThreadPoolExecutor(max_workers=max(1, concurrency), thread_name_prefix="barca-xfer")
    try:
        while True:
            try:
                msg = _runtime.recv_message()
            except (RuntimeError, OSError):
                # Coordinator exited: abandon queued work.
                pool.shutdown(wait=False, cancel_futures=True)
                return
            kind = msg.get("type")
            if kind == "shutdown":
                break
            if kind in ("put", "get"):
                pool.submit(_handle, msg, retries, backoff)
        pool.shutdown(wait=True)
    finally:
        pool.shutdown(wait=False, cancel_futures=True)


def _env_int(name: str, default: int) -> int:
    raw = os.environ.get(name)
    return int(raw) if raw else default


def main() -> int:
    if not os.environ.get("BARCA_SOCKET"):
        print("BARCA_SOCKET not set", file=sys.stderr)
        return 1
    sock = _runtime.connect()
    assert sock is not None
    serve(
        sock,
        concurrency=_env_int("BARCA_TRANSFER_CONCURRENCY", _DEFAULT_CONCURRENCY),
        retries=_env_int("BARCA_TRANSFER_RETRIES", _DEFAULT_RETRIES),
    )
    _runtime.disconnect()
    return 0


if __name__ == "__main__":
    sys.exit(main())
