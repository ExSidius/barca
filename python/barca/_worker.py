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
from pathlib import Path

from barca._artifacts import artifact_path, deserialize, detect_format, serialize


_PROTOCOL_VERSION = 2


def _emit(msg_type, **fields):
    """Emit a protocol message on stderr: BARCA:<version>:<json>"""
    payload = json.dumps({"type": msg_type, **fields})
    print(f"BARCA:{_PROTOCOL_VERSION}:{payload}", file=sys.stderr, flush=True)


def load_module(source_file):
    path = Path(source_file).resolve()
    # Add the file's directory to sys.path so cross-file imports work.
    module_dir = str(path.parent)
    if module_dir not in sys.path:
        sys.path.insert(0, module_dir)
    mod_name = f"_barca_{path.stem}"
    spec = importlib.util.spec_from_file_location(mod_name, str(path))
    assert spec is not None, f"Could not load module spec for {path}"
    mod = importlib.util.module_from_spec(spec)
    # Register in sys.modules so pickle can find classes defined in user code.
    sys.modules[mod_name] = mod
    assert spec.loader is not None, f"No loader for {path}"
    spec.loader.exec_module(mod)
    return mod


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


def run_batch(batch):
    cache = {}
    modules = {}

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
        source = str(Path(step["source_file"]).resolve())
        if source not in modules:
            modules[source] = load_module(source)

        fn = getattr(modules[source], step["function_name"])

        # Resolve inputs: check local cache (includes provided_inputs).
        kwargs = {}
        for param_name, upstream_id in step.get("inputs", {}).items():
            if upstream_id in cache:
                kwargs[param_name] = cache[upstream_id]
            else:
                raise RuntimeError(
                    f"Input '{param_name}' (from '{upstream_id}') not found in cache. "
                    f"Available: {list(cache.keys())}"
                )

        # Inject partition values as kwargs (e.g., ticker="AAPL").
        if "partition" in step:
            kwargs.update(step["partition"])

        # User's print() goes to stdout (visible in terminal).
        # Protocol messages go to stderr (Rust reads this).
        t0 = time.perf_counter()
        result = fn(**kwargs) if kwargs else fn()
        elapsed = time.perf_counter() - t0

        # Sensors return (updated: bool, data) tuples — unpack for downstream.
        if step.get("kind") == "sensor" and isinstance(result, tuple) and len(result) == 2:
            _updated, result = result

        cache[step["node_id"]] = result

        # Serialize to artifact file.
        explicit_fmt = step.get("serializer")
        fmt = detect_format(result, explicit=explicit_fmt)
        path = artifact_path(art_dir, step["node_id"], fmt)
        size = serialize(result, path, fmt)

        _emit(
            "result",
            node_id=step["node_id"],
            artifact={"path": str(path), "format": fmt, "size_bytes": size},
            elapsed=elapsed,
        )


def main():
    if len(sys.argv) < 2:
        print("Usage: python -m barca._worker <batch.json>", file=sys.stderr)
        sys.exit(1)

    with open(sys.argv[1]) as f:
        batch = json.load(f)

    run_batch(batch)


if __name__ == "__main__":
    main()
