# Partitions

Partitions split one asset into independent steps, one per key; the keys run in parallel.
Each key is cached on its own: it has its own run hash, so a re-run serves unchanged keys from
cache and executes only keys with no successful materialization (a new key, or a key whose
function or upstream changed). A `collect` fan-in re-runs when its set of inputs changes.

```python
from barca import asset, partitions, partitions_from, collect


@asset(partitions={"region": partitions(["emea", "amer", "apac"])})
def sales(region: str) -> dict:
    return {"region": region, "revenue": len(region) * 100}


@asset(partitions={"region": partitions_from(sales)})   # same keys as `sales`
def margin(region: str, sales: dict) -> dict:
    return {"region": region, "margin": sales["revenue"] * 0.2}


@asset(inputs={"all_sales": collect(sales)})            # fan-in: every partition as a list
def summary(all_sales: list[dict]) -> dict:
    return {"total": sum(s["revenue"] for s in all_sales)}
```

- `partitions([...])` declares keys. A literal list is read statically; any other expression
  (a list comprehension, a function call) is evaluated by the Python runtime at plan time.
- The partition key is passed to the function as the parameter named in `partitions={...}`.
- `partitions_from(upstream)` reuses an upstream asset's partition keys.
- `collect(upstream)` inside `inputs=` aggregates all partitions of `upstream` into one list.
- An unpartitioned asset in `inputs=` is passed whole to every key, for example
  `@asset(inputs={"m": multiplier}, partitions={"k": partitions(["a", "b"])})` calls the
  function with `k` and `m`. It runs once, before any key, and its run hash is part of every
  key's run hash: changing it, or `--refresh multiplier`, re-runs every key (and, with the
  default cascade, everything downstream of them).
- Artifacts are stored per key, for example
  `.barca/artifacts/pipeline.py--sales_region_emea/<run_hash>.json`. `barca plan` lists one step
  per key, all under the same node id (`pipeline.py:sales`).
- A fan-in (`collect`) runs in its own phase after every partition has finished.

See also: `barca docs sinks` (one file per partition), `barca docs examples/partitions`.
