"""Barca execution runner — invoked by the Rust binary.

Usage: python -m barca._runner <plan.json> [db_path]

Executes the plan, caches results in-process (LRU), and persists
materializations to the local Turso/libSQL database.
"""

import importlib.util
import json
import sys
import time
from collections import OrderedDict, defaultdict
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

import turso


class LRUCache:
    """Simple LRU cache for in-process result passing."""

    def __init__(self, maxsize=1024):
        self._cache = OrderedDict()
        self._maxsize = maxsize

    def get(self, key):
        if key in self._cache:
            self._cache.move_to_end(key)
            return self._cache[key]
        return None

    def put(self, key, value):
        if key in self._cache:
            self._cache.move_to_end(key)
        else:
            if len(self._cache) >= self._maxsize:
                self._cache.popitem(last=False)
        self._cache[key] = value

    def __contains__(self, key):
        return key in self._cache


def load_module(source_file):
    path = Path(source_file).resolve()
    spec = importlib.util.spec_from_file_location(f"_barca_{path.stem}", str(path))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def execute_step(step, cache, modules):
    source = str(Path(step["source_file"]).resolve())
    if source not in modules:
        modules[source] = load_module(source)

    fn = getattr(modules[source], step["function_name"])

    kwargs = {}
    for param_name, upstream_id in step.get("inputs", {}).items():
        value = cache.get(upstream_id)
        if value is None:
            raise RuntimeError(
                f"Input '{param_name}' (from '{upstream_id}') not found in cache"
            )
        kwargs[param_name] = value

    return fn(**kwargs) if kwargs else fn()


def save_materialization(db_conn, node_id, output, elapsed):
    """Persist a materialization result to the database."""
    try:
        output_json = json.dumps(output, default=str)
    except (TypeError, ValueError):
        output_json = json.dumps({"__repr__": repr(output)[:500]})

    db_conn.execute(
        "INSERT INTO materializations (node_id, output_json, elapsed_seconds, status) "
        "VALUES (?, ?, ?, 'success')",
        (node_id, output_json, elapsed),
    )
    db_conn.commit()


def run_plan(plan_data, db_path=None):
    plan = plan_data["plan"]
    cache = LRUCache(maxsize=4096)
    modules = {}

    # Open DB connection if path provided (pyturso — drop-in for sqlite3).
    db_conn = None
    if db_path:
        db_conn = turso.connect(db_path)

    tiers = defaultdict(list)
    for step in plan["steps"]:
        tiers[step["tier"]].append(step)

    t0 = time.perf_counter()

    for tier_num in sorted(tiers.keys()):
        steps = tiers[tier_num]
        if len(steps) == 1:
            step = steps[0]
            step_t0 = time.perf_counter()
            result = execute_step(step, cache, modules)
            step_elapsed = time.perf_counter() - step_t0

            cache.put(step["node_id"], result)
            if db_conn:
                save_materialization(db_conn, step["node_id"], result, step_elapsed)
        else:
            with ThreadPoolExecutor() as pool:
                futures = {
                    pool.submit(execute_step, step, cache, modules): step
                    for step in steps
                }
                for future in as_completed(futures):
                    step = futures[future]
                    step_t0 = time.perf_counter()
                    result = future.result()
                    step_elapsed = time.perf_counter() - step_t0

                    cache.put(step["node_id"], result)
                    if db_conn:
                        save_materialization(
                            db_conn, step["node_id"], result, step_elapsed
                        )

    elapsed = time.perf_counter() - t0

    if db_conn:
        db_conn.close()

    return cache, elapsed


def main():
    if len(sys.argv) < 2:
        print("Usage: python -m barca._runner <plan.json> [db_path]", file=sys.stderr)
        sys.exit(1)

    plan_path = sys.argv[1]
    db_path = sys.argv[2] if len(sys.argv) > 2 else None

    with open(plan_path) as f:
        plan_data = json.load(f)

    cache, elapsed = run_plan(plan_data, db_path)
    plan = plan_data["plan"]

    result = {
        "elapsed_seconds": round(elapsed, 6),
        "steps_executed": len(plan["steps"]),
        "tiers": plan["total_tiers"],
    }

    last_step = plan["steps"][-1] if plan["steps"] else None
    if last_step and last_step["node_id"] in cache:
        result["final_output"] = cache.get(last_step["node_id"])

    print(json.dumps(result, indent=2, default=str))


if __name__ == "__main__":
    main()
