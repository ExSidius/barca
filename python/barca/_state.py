"""Shared-state blob transfer — pull/push of the metadata DB with optimistic
concurrency.

Invoked by the Rust coordinator as ``python -m barca._state pull|push`` so
the fsspec extras (and their credential chains) remain the only cloud-auth
surface. This module never opens the database — it moves opaque bytes.

Every backend implements the same contract:

  pull(uri, local)   -> opaque token (etag / generation / sha256) or None
                        when the remote object does not exist. The token is
                        read BEFORE downloading, so a concurrent replace
                        between the token read and the download yields a
                        stale token — which safely fails the later push with
                        a conflict, never the reverse.
  push(uri, local, token) -> new token. token=None means create-only (the
                        object must not exist). Raises ConflictError when
                        the precondition fails (someone else pushed first).

Backends:
  abfs/abfss  adlfs — etag If-Match via pipe_file kwargs
  s3/s3a      s3fs — IfMatch via pipe_file kwargs (multipart guard)
  gs/gcs      google-cloud-storage directly (gcsfs cannot express
              generation-match on overwrite); it ships with the gcs extra
  file:// or plain path — sha256 tokens + lock file + atomic replace.
              Used by integration tests and works on shared/NFS mounts.
"""

import hashlib
import json
import os
import sys
import tempfile
from pathlib import Path

from barca import _storage

# s3fs switches to multipart above its chunk size, where IfMatch is invalid.
_S3_SINGLE_PUT_LIMIT = 48 * 1024 * 1024

_EXIT_CONFLICT = 3


class ConflictError(Exception):
    """The remote state changed since our token was read (precondition failed)."""


# ─── Helpers ─────────────────────────────────────────────────────────────────


def _local_path_of(uri: str) -> "Path | None":
    """Path for file:// URIs and plain local paths; None for remote URIs."""
    if uri.startswith("file://"):
        return Path(uri[len("file://") :])
    if "://" not in uri:
        return Path(uri)
    return None


def _protocol(uri: str) -> str:
    return uri.split("://", 1)[0].lower()


def _sha256(path: "Path | str") -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def _staged_download(fetch, local_path: "Path | str") -> None:
    """Download via `fetch(tmp_path)` then atomically replace local_path."""
    local_path = Path(local_path)
    local_path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=local_path.parent, prefix=".state.", suffix=".tmp")
    os.close(fd)
    try:
        fetch(tmp)
        os.replace(tmp, local_path)
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise


def _remote_token(uri: str) -> "str | None":
    """Current etag/generation of a remote object, or None when absent."""
    fs = _storage.get_fs(uri)
    try:
        info = fs.info(uri)
    except FileNotFoundError:
        return None
    for key in ("etag", "ETag", "generation"):
        if key in info and info[key] is not None:
            return str(info[key])
    raise RuntimeError(f"no etag/generation in object info for {uri}: keys={sorted(info)}")


def _is_conflict(exc: Exception) -> bool:
    """Best-effort classification of precondition-failure errors across SDKs."""
    name = type(exc).__name__
    if name in ("ResourceModifiedError", "ResourceExistsError", "PreconditionFailed"):
        return True
    text = str(exc)
    return (
        "PreconditionFailed" in text
        or "ConditionNotMet" in text
        or "412" in text
        or "At least one of the pre-conditions you specified did not hold" in text
    )


# ─── file:// backend (tests, NFS-ish shared mounts) ──────────────────────────


def _file_lock(target: Path):
    import fcntl
    from contextlib import contextmanager

    @contextmanager
    def lock():
        target.parent.mkdir(parents=True, exist_ok=True)
        lock_path = target.with_name(target.name + ".lock")
        with open(lock_path, "w") as lf:
            fcntl.flock(lf, fcntl.LOCK_EX)
            try:
                yield
            finally:
                fcntl.flock(lf, fcntl.LOCK_UN)

    return lock()


def _file_pull(target: Path, local_path: "Path | str") -> "str | None":
    with _file_lock(target):
        if not target.exists():
            return None
        token = _sha256(target)

        def fetch(tmp):
            import shutil

            shutil.copyfile(target, tmp)

        _staged_download(fetch, local_path)
        return token


def _file_push(target: Path, local_path: "Path | str", token: "str | None") -> str:
    with _file_lock(target):
        current = _sha256(target) if target.exists() else None
        if current != token:
            raise ConflictError(f"state at {target} changed (expected {token}, found {current})")
        target.parent.mkdir(parents=True, exist_ok=True)
        fd, tmp = tempfile.mkstemp(dir=target.parent, prefix=".push.", suffix=".tmp")
        os.close(fd)
        try:
            import shutil

            shutil.copyfile(local_path, tmp)
            os.replace(tmp, target)
        except BaseException:
            Path(tmp).unlink(missing_ok=True)
            raise
        return _sha256(target)


# ─── Public API ──────────────────────────────────────────────────────────────


def pull(state_uri: str, local_path: "Path | str") -> "str | None":
    """Download the state blob to local_path (atomic replace).

    Returns the concurrency token, or None when the remote object is absent
    (local_path is then left untouched).
    """
    local_target = _local_path_of(state_uri)
    if local_target is not None:
        return _file_pull(local_target, local_path)

    token = _remote_token(state_uri)
    if token is None:
        return None
    _staged_download(lambda tmp: _storage.get_file(state_uri, tmp), local_path)
    return token


def push(state_uri: str, local_path: "Path | str", token: "str | None") -> str:
    """Conditionally upload local_path over the state blob.

    token=None → create-only. Raises ConflictError when the remote no longer
    matches the token (or already exists, for create-only).
    """
    local_target = _local_path_of(state_uri)
    if local_target is not None:
        return _file_push(local_target, local_path, token)

    protocol = _protocol(state_uri)
    data = Path(local_path).read_bytes()

    try:
        if protocol in ("abfs", "abfss"):
            fs = _storage.get_fs(state_uri)
            if token is None:
                fs.pipe_file(state_uri, data, mode="create")
            else:
                from azure.core import MatchConditions

                fs.pipe_file(
                    state_uri,
                    data,
                    overwrite=True,
                    etag=token,
                    match_condition=MatchConditions.IfNotModified,
                )
        elif protocol in ("s3", "s3a"):
            if len(data) > _S3_SINGLE_PUT_LIMIT:
                raise RuntimeError(
                    f"state blob is {len(data)} bytes — larger than the single-request "
                    f"S3 limit ({_S3_SINGLE_PUT_LIMIT}); conditional multipart uploads "
                    "are not supported"
                )
            fs = _storage.get_fs(state_uri)
            if token is None:
                fs.pipe_file(state_uri, data, mode="create")
            else:
                fs.pipe_file(state_uri, data, IfMatch=token)
        elif protocol in ("gs", "gcs"):
            # gcsfs cannot express a generation precondition on overwrite;
            # use the official SDK (a gcsfs dependency) directly.
            from google.cloud import storage as gcs_storage

            rest = state_uri.split("://", 1)[1]
            bucket_name, blob_name = rest.split("/", 1)
            client = gcs_storage.Client()
            blob = client.bucket(bucket_name).blob(blob_name)
            blob.upload_from_filename(
                str(local_path), if_generation_match=int(token) if token else 0
            )
        else:
            raise RuntimeError(f"unsupported state backend '{protocol}://'")
    except ConflictError:
        raise
    except Exception as exc:
        if _is_conflict(exc):
            raise ConflictError(str(exc)) from exc
        raise

    new_token = _remote_token(state_uri)
    if new_token is None:
        raise RuntimeError(f"push succeeded but {state_uri} has no token")
    return new_token


# ─── CLI (invoked by the Rust coordinator) ───────────────────────────────────


def main(argv: "list[str] | None" = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    if len(argv) >= 3 and argv[0] == "pull":
        uri, local = argv[1], argv[2]
        token = pull(uri, local)
        print(json.dumps({"exists": token is not None, "token": token}))
        return 0
    if len(argv) >= 3 and argv[0] == "push":
        uri, local = argv[1], argv[2]
        token = None
        if "--token" in argv:
            token = argv[argv.index("--token") + 1]
        try:
            new_token = push(uri, local, token)
        except ConflictError as exc:
            print(f"conflict: {exc}", file=sys.stderr)
            return _EXIT_CONFLICT
        print(json.dumps({"token": new_token}))
        return 0
    print(
        "usage: python -m barca._state pull <uri> <local>\n"
        "       python -m barca._state push <uri> <local> [--token T]",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except ConflictError as exc:  # pull never raises this; belt and braces
        print(f"conflict: {exc}", file=sys.stderr)
        sys.exit(_EXIT_CONFLICT)
    except Exception as exc:
        print(f"error: {type(exc).__name__}: {exc}", file=sys.stderr)
        sys.exit(1)
