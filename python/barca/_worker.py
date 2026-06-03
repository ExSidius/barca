"""Barca worker — executes a batch of steps sequentially.

Invoked by Rust: python -m barca._worker <batch.json>

Protocol:
  - Input: batch JSON file with steps and provided_inputs
  - Protocol output: JSON lines on STDERR (Rust reads this)
  - User output: stdout passes through to terminal (print() works normally)
  - No DB access — Rust owns all persistence
"""

import importlib.util
import json
import sys
import time
from pathlib import Path


def load_module(source_file):
    path = Path(source_file).resolve()
    spec = importlib.util.spec_from_file_location(f"_barca_{path.stem}", str(path))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def run_batch(batch):
    cache = {}
    modules = {}

    # Pre-load provided inputs (cross-phase values injected by Rust).
    provided = batch.get("provided_inputs", {})
    cache.update(provided)

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

        # User's print() goes to stdout (visible in terminal).
        # Protocol messages go to stderr (Rust reads this).
        t0 = time.perf_counter()
        result = fn(**kwargs) if kwargs else fn()
        elapsed = time.perf_counter() - t0

        cache[step["node_id"]] = result

        # Emit result as JSON line to stderr (protocol channel).
        print(
            json.dumps(
                {"node_id": step["node_id"], "output": result, "elapsed": elapsed},
                default=str,
            ),
            file=sys.stderr,
            flush=True,
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
