"""Barca worker — executes a batch of steps sequentially.

Invoked by Rust: python -m barca._worker <batch.json>

Protocol:
  - Input: batch JSON file with steps, provided_inputs, and artifact_dir
  - Protocol output: prefixed JSON lines on STDERR: BARCA:2:{...}
  - User output: stdout passes through to terminal (print() works normally)
  - Non-prefixed stderr lines are treated as errors/tracebacks
  - No DB access — Rust owns all persistence
"""

import importlib.util
import json
import os
import sys
import time
import traceback
from pathlib import Path

from barca import _storage
from barca._artifacts import (
    artifact_path,
    clean_staging,
    deserialize,
    detect_format,
    resolve_format,
    safe_node_id,
    serialize,
)

_EXT_FORMATS = {
    ".json": "json",
    ".pkl": "pickle",
    ".pickle": "pickle",
    ".parquet": "parquet",
}


def _peak_rss_bytes() -> int:
    """Peak RSS of this process in bytes (0 if unavailable).

    `ru_maxrss` is kilobytes on Linux and bytes on macOS.
    """
    try:
        import resource

        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        if sys.platform == "darwin":
            return int(peak)
        return int(peak) * 1024
    except Exception:
        return 0


# Local artifacts above this size skip the tier-1 cache: the deepcopy that
# guards against mutation would cost more than the disk read it saves.
# Remote artifacts always cache — skipping a network fetch beats any copy.
_LRU_MAX_ARTIFACT_BYTES = 8 * 1024 * 1024


def _lru_cacheable(path: str, size_bytes=None) -> bool:
    # Remote first: skipping a network fetch beats any copy, whatever the size.
    if _storage.is_remote(path):
        return True
    if size_bytes is not None:
        return size_bytes <= _LRU_MAX_ARTIFACT_BYTES
    try:
        return os.path.getsize(path) <= _LRU_MAX_ARTIFACT_BYTES
    except OSError:
        return False


class _ArtifactLRU:
    """Tier-1 read-through cache: deserialized artifacts hot in this process.

    Keyed by artifact path — paths are content-addressed
    ({node}/{run_hash}{ext}), so a path uniquely identifies content and
    invalidation is automatic (changed input → changed hash → new path → miss).
    Values are returned as deep copies so a task mutating its input can never
    poison a later task's view; if a value can't be deep-copied, the entry is
    dropped and the caller falls through to the store (tier 2). Pure
    luck-optimization: always safe to miss, never persisted, never gates
    correctness.
    """

    def __init__(self, max_entries: int = 16):
        from collections import OrderedDict

        self._entries: "OrderedDict[str, object]" = OrderedDict()
        self._max = max_entries

    def get(self, path: str):
        """Return a safe copy of the cached value, or None on miss."""
        if path not in self._entries:
            return None
        import copy

        self._entries.move_to_end(path)
        try:
            return copy.deepcopy(self._entries[path])
        except Exception:
            del self._entries[path]
            return None

    def put(self, path: str, value) -> None:
        import copy

        try:
            self._entries[path] = copy.deepcopy(value)
        except Exception:
            return
        self._entries.move_to_end(path)
        while len(self._entries) > self._max:
            self._entries.popitem(last=False)


def _default_artifact_dir() -> str:
    """Artifact store root: BARCA_ARTIFACT_URI if set, else local .barca/artifacts."""
    uri = os.environ.get("BARCA_ARTIFACT_URI")
    if uri:
        return uri
    return str(Path(".barca/artifacts").resolve())


_PROTOCOL_VERSION = 2
_use_socket = False


def _emit(msg_type, **fields):
    """Emit a protocol message — via socket if available, else stderr."""
    if _use_socket:
        from barca import _runtime

        if msg_type == "result":
            _runtime.emit_step_completed(fields["node_id"], fields["artifact"])
        elif msg_type == "error":
            _runtime.emit_step_error(
                node_id=fields["node_id"],
                error_type=fields["error_type"],
                message=fields["message"],
                traceback=fields["traceback"],
                elapsed=fields.get("elapsed", 0.0),
            )
        elif msg_type == "blocked":
            _runtime.emit_blocked(fields["node_id"], fields["reason"])
        return
    # Original stderr protocol
    payload = json.dumps({"type": msg_type, **fields})
    print(f"BARCA:{_PROTOCOL_VERSION}:{payload}", file=sys.stderr, flush=True)


def _user_traceback(exc) -> str:
    """Format the traceback with barca-internal frames stripped.

    Keeps only frames from user code so surfaced errors point at the user's
    file/line, never at _worker.py plumbing. Returns "" when every frame is
    internal (e.g. a TypeError raised by the fn(**kwargs) call itself) — the
    caller always leads with "ErrorType: message", which carries the detail.
    """
    barca_dir = str(Path(__file__).resolve().parent)
    frames = [
        f
        for f in traceback.extract_tb(exc.__traceback__)
        if not str(Path(f.filename).resolve()).startswith(barca_dir)
    ]
    if not frames:
        return ""
    return "".join(traceback.format_list(frames)).rstrip("\n")


def _emit_error(node_id, exc, elapsed=0.0):
    """Emit a structured failure for a single step. Rust owns the retry decision."""
    _emit(
        "error",
        node_id=node_id,
        error_type=type(exc).__name__,
        message=str(exc),
        traceback=_user_traceback(exc),
        elapsed=elapsed,
    )


def load_module(source_file):
    path = Path(source_file).resolve()
    # Add the file's directory to sys.path so cross-file imports work.
    module_dir = str(path.parent)
    if module_dir not in sys.path:
        sys.path.insert(0, module_dir)
    mod_name = f"_barca_{path.stem}"
    spec = importlib.util.spec_from_file_location(mod_name, str(path))
    if spec is None:
        raise RuntimeError(f"Could not load module spec for {path}")
    mod = importlib.util.module_from_spec(spec)
    # Register in sys.modules so pickle can find classes defined in user code.
    sys.modules[mod_name] = mod
    if spec.loader is None:
        raise RuntimeError(f"No loader for {path}")
    spec.loader.exec_module(mod)
    return mod


def _run_with_timeout(fn, kwargs, timeout_seconds):
    """Run a function with a timeout. Raises TimeoutError if exceeded."""
    import threading

    result = None
    exception = None

    def target():
        nonlocal result, exception
        try:
            result = fn(**kwargs) if kwargs else fn()
        except Exception as e:
            exception = e

    thread = threading.Thread(target=target)
    thread.daemon = True
    thread.start()
    thread.join(timeout=timeout_seconds)

    if thread.is_alive():
        raise TimeoutError(f"Function '{fn.__name__}' exceeded timeout of {timeout_seconds}s")
    if exception is not None:
        raise exception
    return result


def _resolve_input(raw_value):
    """Resolve a provided input: artifact ref → deserialized value, else raw.

    For collected (fan-in) inputs, deserializes each partition artifact into a list.
    """
    if isinstance(raw_value, dict):
        if raw_value.get("_collected") and "artifacts" in raw_value:
            return [deserialize(a["path"], a["format"]) for a in raw_value["artifacts"]]
        if "path" in raw_value and "format" in raw_value:
            return deserialize(raw_value["path"], raw_value["format"])
    return raw_value


def _load_artifact(path, lru, fmt=None):
    """Resolve one artifact path to its deserialized value via the tier-1 LRU
    cache, falling through to the artifact store on miss."""
    hot = lru.get(path)
    if hot is not None:
        return hot
    if not _storage.exists(path):
        raise FileNotFoundError(f"Input artifact not found: {path}")
    if fmt is None:
        fmt = _EXT_FORMATS.get(_storage.suffix(path), "json")
    value = deserialize(path, fmt)
    if _lru_cacheable(path):
        lru.put(path, value)
    return value


def _execute(fn, kwargs, step):
    """Run a step function with optional timeout, unpacking sensor tuples."""
    timeout = step.get("timeout_seconds", 0)
    t0 = time.perf_counter()
    if timeout and timeout > 0:
        result = _run_with_timeout(fn, kwargs, timeout)
    else:
        result = fn(**kwargs) if kwargs else fn()
    elapsed = time.perf_counter() - t0

    # Sensors return (updated: bool, data) tuples — unpack for downstream.
    if step.get("kind") == "sensor" and isinstance(result, tuple) and len(result) == 2:
        _updated, result = result
    return result, elapsed


def _sink_dest(path: str, node_id: str, base_node_id: str) -> str:
    """Sink destination path, with a partition suffix injected before the
    extension for partitioned assets so partitions don't clobber each other
    (e.g. out.parquet → out_ticker_AAPL.parquet)."""
    if node_id == base_node_id or not node_id.startswith(base_node_id):
        return path
    part = safe_node_id(node_id[len(base_node_id) :])
    ext = _storage.suffix(path)
    if ext:
        return path[: -len(ext)] + part + ext
    return path + part


def _write_sinks(result, step, node_id, primary_fmt):
    """Write each @sink declared on the step. Error-isolated: a sink failure
    never fails the parent asset — it is logged and reported in the outcome."""
    outcomes = []
    base_id = step.get("node_id", node_id)
    for sink in step.get("sinks") or []:
        dest = sink.get("path", "")
        try:
            fmt = sink.get("serializer") or _EXT_FORMATS.get(_storage.suffix(dest)) or primary_fmt
            if fmt not in ("json", "pickle", "parquet"):
                raise ValueError(
                    f"sink serializer '{fmt}' is not supported yet "
                    "(supported: json, pickle, parquet)"
                )
            fmt = resolve_format(result, fmt)
            dest = _sink_dest(dest, node_id, base_id)
            size = serialize(result, dest, fmt)
            outcomes.append({"path": str(dest), "status": "ok", "size_bytes": size})
        except Exception as exc:
            print(
                f"[barca] SINK FAILED: {node_id} -> {dest}: {type(exc).__name__}: {exc}",
                file=sys.stderr,
                flush=True,
            )
            outcomes.append(
                {
                    "path": str(dest),
                    "status": "error",
                    "error": f"{type(exc).__name__}: {exc}",
                }
            )
    return outcomes


def _materialize(result, node_id, art_dir, step, elapsed, elapsed_in_artifact=False, timing=None):
    """Serialize a result to its artifact and emit a `result` protocol message.

    `timing` (cpu_seconds, max_rss_bytes) rides on the artifact dict — the
    completion message does triple duty: closes the lease, carries the output
    ref, and feeds the coordinator's cost estimator. `elapsed`/`timing` as
    passed in cover only the step function's own execution; this adds the
    serialization time measured here on top, so a step's true cost — what the
    cost model, `barca stats`, and `barca history` all see — isn't
    systematically undercounted for large payloads (serialization can be the
    majority of a step's real wall time and was previously invisible).
    """
    explicit_fmt = step.get("serializer")
    fmt = resolve_format(result, detect_format(result, explicit=explicit_fmt))
    # Content-addressed layout when the coordinator supplies a run hash.
    # Batch mode's legacy partitioned loop reuses the step-level hash only for
    # unpartitioned steps (a per-step hash is wrong per-partition; the daemon
    # path gets a per-item hash from Rust and batch mode is test-only).
    run_hash = step.get("run_hash") if node_id == step.get("node_id") else None
    path = artifact_path(art_dir, node_id, fmt, run_hash)
    _ser_wall0 = time.perf_counter()
    _ser_cpu0 = time.process_time()
    size = serialize(result, path, fmt)
    elapsed += time.perf_counter() - _ser_wall0
    if timing and timing.get("cpu_seconds") is not None:
        timing = {
            **timing,
            "cpu_seconds": timing["cpu_seconds"] + (time.process_time() - _ser_cpu0),
        }
    artifact = {"path": str(path), "format": fmt, "size_bytes": size}
    if elapsed_in_artifact:
        artifact["elapsed_seconds"] = elapsed
    if timing:
        artifact.update(timing)
    sink_outcomes = _write_sinks(result, step, node_id, fmt)
    if sink_outcomes:
        artifact["sinks"] = sink_outcomes
    _emit("result", node_id=node_id, artifact=artifact, elapsed=elapsed)
    return artifact


def run_batch(batch):
    cache = {}
    modules = {}
    # node_ids (base or partition-suffixed) that failed or were blocked. A step is
    # skipped (blocked) if any input it depends on is unavailable — this lets
    # independent chains bundled in the same batch finish even when one fails.
    unavailable = set()

    # Artifact directory for writing outputs (local path or remote URI).
    art_dir = batch.get("artifact_dir") or os.environ.get("BARCA_ARTIFACT_URI")
    if art_dir and not _storage.is_remote(art_dir):
        Path(art_dir).mkdir(parents=True, exist_ok=True)
    clean_staging()

    # Pre-load provided inputs (cross-phase values injected by Rust).
    # Values may be artifact references — resolve them lazily when accessed.
    provided = batch.get("provided_inputs", {})
    for key, value in provided.items():
        cache[key] = _resolve_input(value)

    for step in batch["steps"]:
        partition_keys = step.get("partition_keys", [])
        if partition_keys:
            # Late partition expansion: worker loops over partition_keys internally.
            # Each partition key is a dict like {"ticker": "AAPL"}. Partitions are
            # independent — one bad partition does not block the others.
            for pk in partition_keys:
                suffix = ",".join(f"{k}={v}" for k, v in sorted(pk.items()))
                full_node_id = f"{step['node_id']}[{suffix}]"

                # Is any upstream this partition depends on unavailable?
                blocked_on = None
                for _param, upstream_id in step.get("inputs", {}).items():
                    aligned_id = f"{upstream_id}[{suffix}]"
                    if aligned_id in unavailable or upstream_id in unavailable:
                        blocked_on = upstream_id
                        break
                if blocked_on is not None:
                    unavailable.add(full_node_id)
                    _emit(
                        "blocked",
                        node_id=full_node_id,
                        reason=f"upstream '{blocked_on}' unavailable",
                    )
                    continue

                try:
                    source = str(Path(step["source_file"]).resolve())
                    if source not in modules:
                        modules[source] = load_module(source)
                    fn = getattr(modules[source], step["function_name"])

                    # Direct args/kwargs from parallel() dispatch — skip artifact lookup.
                    if "direct_args" in step or "direct_kwargs" in step:
                        d_args = step.get("direct_args", [])
                        d_kwargs = step.get("direct_kwargs", {})
                        timeout = step.get("timeout_seconds", 0)
                        t0 = time.time()
                        if timeout and timeout > 0:
                            result = _run_with_timeout(lambda: fn(*d_args, **d_kwargs), {}, timeout)
                        else:
                            result = fn(*d_args, **d_kwargs)
                        elapsed = time.time() - t0
                        cache[full_node_id] = result
                        _materialize(result, full_node_id, art_dir, step, elapsed)
                        continue
                    else:
                        kwargs = {}
                        for param_name, upstream_id in step.get("inputs", {}).items():
                            if param_name.startswith("_"):
                                kwargs[param_name] = None
                                continue
                            aligned_id = f"{upstream_id}[{suffix}]"
                            if aligned_id in cache:
                                kwargs[param_name] = cache[aligned_id]
                            elif upstream_id in cache:
                                kwargs[param_name] = cache[upstream_id]
                            else:
                                raise RuntimeError(
                                    f"Input '{param_name}' (from '{upstream_id}') not found in cache. "
                                    f"Tried aligned '{aligned_id}' and base '{upstream_id}'. "
                                    f"Available: {list(cache.keys())}"
                                )
                        kwargs.update(pk)  # inject partition values (e.g., ticker="AAPL").

                    result, elapsed = _execute(fn, kwargs, step)
                except Exception as exc:
                    unavailable.add(full_node_id)
                    _emit_error(full_node_id, exc)
                    continue

                cache[full_node_id] = result
                _materialize(result, full_node_id, art_dir, step, elapsed)
        else:
            node_id = step["node_id"]

            # Is any upstream this step depends on unavailable?
            blocked_on = None
            for _param, upstream_id in step.get("inputs", {}).items():
                if upstream_id in unavailable:
                    blocked_on = upstream_id
                    break
            if blocked_on is not None:
                unavailable.add(node_id)
                _emit("blocked", node_id=node_id, reason=f"upstream '{blocked_on}' unavailable")
                continue

            try:
                source = str(Path(step["source_file"]).resolve())
                if source not in modules:
                    modules[source] = load_module(source)
                fn = getattr(modules[source], step["function_name"])

                # Direct args/kwargs from parallel() dispatch — skip artifact lookup.
                if "direct_args" in step or "direct_kwargs" in step:
                    d_args = step.get("direct_args", [])
                    d_kwargs = step.get("direct_kwargs", {})
                    timeout = step.get("timeout_seconds", 0)
                    t0 = time.time()
                    if timeout and timeout > 0:
                        result = _run_with_timeout(lambda: fn(*d_args, **d_kwargs), {}, timeout)
                    else:
                        result = fn(*d_args, **d_kwargs)
                    elapsed = time.time() - t0
                    cache[node_id] = result
                    _materialize(result, node_id, art_dir, step, elapsed)
                    continue
                else:
                    kwargs = {}
                    for param_name, upstream_id in step.get("inputs", {}).items():
                        if param_name.startswith("_"):
                            kwargs[param_name] = None
                            continue
                        if upstream_id in cache:
                            kwargs[param_name] = cache[upstream_id]
                        else:
                            raise RuntimeError(
                                f"Input '{param_name}' (from '{upstream_id}') not found in cache. "
                                f"Available: {list(cache.keys())}"
                            )
                    if "partition" in step:
                        kwargs.update(step["partition"])

                result, elapsed = _execute(fn, kwargs, step)
            except Exception as exc:
                unavailable.add(node_id)
                _emit_error(node_id, exc)
                continue

            cache[node_id] = result
            _materialize(result, node_id, art_dir, step, elapsed)


def _run_daemon_step(step, modules, art_dir, lru):
    """Execute one step in daemon mode and emit its result or error.

    Returns True on success, False on step failure. Socket errors raised while
    emitting propagate to the caller (the connection is gone — exit the loop).
    Per-task self-timing: CPU time (`process_time`, the truest measure of
    work), wall time, and peak RSS ride back on the completion message.
    """
    from barca import _runtime

    node_id = step.get("node_id", "unknown")
    t0 = time.perf_counter()
    c0 = time.process_time()

    try:
        source = str(Path(step["source_file"]).resolve())
        if source not in modules:
            modules[source] = load_module(source)
        fn = getattr(modules[source], step["function_name"])

        # Direct args/kwargs from parallel() dispatch.
        d_args = step.get("direct_args", [])
        d_kwargs = step.get("direct_kwargs", {})

        # Resolve dag_inputs as function arguments.
        inputs = step.get("inputs", {})
        kwargs = dict(d_kwargs) if d_kwargs else {}
        for param, value in inputs.items():
            # Skip ordering-only deps (underscore-prefixed params carry no data).
            if param.startswith("_"):
                kwargs[param] = None
                continue
            # Fan-in (collect()): every partition artifact of the upstream,
            # deserialized into a list — matches batch mode's _resolve_input.
            if isinstance(value, dict) and value.get("_collected"):
                try:
                    kwargs[param] = [
                        _load_artifact(a["path"], lru, fmt=a.get("format"))
                        for a in value.get("artifacts", [])
                    ]
                except FileNotFoundError as e:
                    raise FileNotFoundError(
                        f"Input artifact for parameter '{param}' not found: {e}"
                    ) from e
                continue
            if not value:
                continue
            try:
                kwargs[param] = _load_artifact(value, lru)
            except FileNotFoundError:
                raise FileNotFoundError(
                    f"Input artifact for parameter '{param}' not found: {value}"
                ) from None

        timeout = step.get("timeout_seconds", 0)
        if d_args:
            if timeout and timeout > 0:
                result = _run_with_timeout(lambda: fn(*d_args, **kwargs), {}, timeout)
            else:
                result = fn(*d_args, **kwargs)
        else:
            if timeout and timeout > 0:
                result = _run_with_timeout(lambda: fn(**kwargs), {}, timeout)
            else:
                result = fn(**kwargs)

        wall = time.perf_counter() - t0
        cpu = time.process_time() - c0

        # Sensors return (updated: bool, data) tuples — unpack for downstream.
        if step.get("kind") == "sensor" and isinstance(result, tuple) and len(result) == 2:
            _updated, result = result

        # Convert ParallelError instances so results are JSON-serializable.
        from barca import ParallelError

        def _make_serializable(v):
            if isinstance(v, ParallelError):
                return v.to_dict()
            if isinstance(v, list):
                return [_make_serializable(x) for x in v]
            return v

        result = _make_serializable(result)

        # Serialize result to artifact (and write any declared sinks).
        artifact = _materialize(
            result,
            node_id,
            art_dir,
            step,
            wall,
            elapsed_in_artifact=True,
            timing={"cpu_seconds": cpu, "max_rss_bytes": _peak_rss_bytes()},
        )
        # A downstream step in this worker may consume what we just produced.
        if _lru_cacheable(artifact["path"], artifact.get("size_bytes")):
            lru.put(artifact["path"], result)
        return True

    except BaseException as exc:
        # Any failure inside the step — including TimeoutError and OSError
        # from user code — is a step error. (TimeoutError and the socket
        # errors are OSError subclasses, so a socket-error catch here would
        # swallow them; genuine socket death surfaces when the emit below
        # fails, and that propagates to the caller.)
        wall = time.perf_counter() - t0
        _runtime.emit_step_error(
            node_id=node_id,
            error_type=type(exc).__name__,
            message=str(exc),
            traceback=_user_traceback(exc),
            elapsed=wall,
        )
        return False


def run_daemon():
    """Daemon mode: read execute commands from socket, run each step, send results."""
    global _use_socket

    from barca import _runtime

    if _runtime.connect() is None:
        print("BARCA_SOCKET not set", file=sys.stderr)
        sys.exit(1)
    _use_socket = True

    # Install SIGTERM handler so graceful_kill flushes buffered progress output
    # before the process goes away. Exit via os._exit, not sys.exit(0): a
    # SystemExit raised from the handler while the interpreter is already
    # shutting down is uncatchable, and Python prints it as a noisy
    # "Exception ignored in: _on_sigterm ... SystemExit: 0" on stderr — which
    # then leaks into surfaced worker errors (reliably on Linux). os._exit
    # cannot raise, so that noise is impossible.
    import signal

    def _on_sigterm(_signum, _frame):
        try:
            sys.stdout.flush()
            sys.stderr.flush()
        except Exception:
            pass
        os._exit(0)

    signal.signal(signal.SIGTERM, _on_sigterm)

    modules = {}
    lru = _ArtifactLRU()
    art_dir = _default_artifact_dir()
    if not _storage.is_remote(art_dir):
        Path(art_dir).mkdir(parents=True, exist_ok=True)
    clean_staging()

    while True:
        try:
            msg = _runtime.recv_message()
        except (BrokenPipeError, ConnectionResetError, OSError):
            break
        except Exception:
            break

        if msg.get("type") == "done":
            break

        # Batch pull: K steps per round-trip. The lease closes per-step as
        # each result message goes back; a failure stops the batch (the
        # coordinator kills this worker for a fresh interpreter and re-queues
        # the unstarted remainder).
        if msg.get("type") == "execute_batch":
            steps = msg.get("steps", [])
        elif msg.get("type") == "execute":
            steps = [msg.get("step", {})]
        else:
            continue

        try:
            for step in steps:
                if not _run_daemon_step(step, modules, art_dir, lru):
                    break
        except (BrokenPipeError, ConnectionResetError, OSError):
            # Socket was closed (e.g. replacement worker killed) — exit cleanly.
            break

    _runtime.disconnect()


def main():
    global _use_socket

    if len(sys.argv) >= 2 and sys.argv[1] == "--daemon":
        run_daemon()
        return

    if len(sys.argv) < 2:
        print("Usage: python -m barca._worker <batch.json>", file=sys.stderr)
        sys.exit(1)

    # Connect to executor's Unix socket if available.
    from barca import _runtime

    if _runtime.connect() is not None:
        _use_socket = True
        _runtime.start_heartbeat()

    with open(sys.argv[1]) as f:
        batch = json.load(f)

    try:
        run_batch(batch)
    finally:
        if _use_socket:
            _runtime.stop_heartbeat()
            _runtime.disconnect()


if __name__ == "__main__":
    main()
