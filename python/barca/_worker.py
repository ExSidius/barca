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
import sys
import time
import traceback
from pathlib import Path

from barca._artifacts import artifact_path, deserialize, detect_format, serialize


_PROTOCOL_VERSION = 2


def _emit(msg_type, **fields):
    """Emit a protocol message on stderr: BARCA:<version>:<json>"""
    payload = json.dumps({"type": msg_type, **fields})
    print(f"BARCA:{_PROTOCOL_VERSION}:{payload}", file=sys.stderr, flush=True)


def _emit_error(node_id, exc, elapsed=0.0):
    """Emit a structured failure for a single step. Rust owns the retry decision."""
    _emit(
        "error",
        node_id=node_id,
        error_type=type(exc).__name__,
        message=str(exc),
        traceback=traceback.format_exc(),
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


def _materialize(result, node_id, art_dir, step, elapsed):
    """Serialize a result to its artifact and emit a `result` protocol message."""
    explicit_fmt = step.get("serializer")
    fmt = detect_format(result, explicit=explicit_fmt)
    path = artifact_path(art_dir, node_id, fmt)
    size = serialize(result, path, fmt)
    _emit(
        "result",
        node_id=node_id,
        artifact={"path": str(path), "format": fmt, "size_bytes": size},
        elapsed=elapsed,
    )


def run_batch(batch):
    cache = {}
    modules = {}
    # node_ids (base or partition-suffixed) that failed or were blocked. A step is
    # skipped (blocked) if any input it depends on is unavailable — this lets
    # independent chains bundled in the same batch finish even when one fails.
    unavailable = set()

    # Artifact directory for writing outputs.
    art_dir = batch.get("artifact_dir")
    if art_dir:
        Path(art_dir).mkdir(parents=True, exist_ok=True)

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


def main():
    if len(sys.argv) < 2:
        print("Usage: python -m barca._worker <batch.json>", file=sys.stderr)
        sys.exit(1)

    with open(sys.argv[1]) as f:
        batch = json.load(f)

    run_batch(batch)


if __name__ == "__main__":
    main()
