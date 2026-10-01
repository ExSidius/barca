# Example: partitions with fan-in

One asset fans out over three keys; a second asset collects every partition into a list.

```python
from barca import asset, collect, partitions


@asset(partitions={"region": partitions(["emea", "amer", "apac"])})
def sales(region: str) -> dict:
    base = {"emea": 120, "amer": 200, "apac": 80}[region]
    return {"region": region, "revenue": base}


@asset(inputs={"all_sales": collect(sales)})
def summary(all_sales: list[dict]) -> dict:
    return {"regions": len(all_sales), "total": sum(s["revenue"] for s in all_sales)}
```

```bash
barca plan pipeline.py
barca get summary pipeline.py
barca get summary pipeline.py
```

What to notice:

- `barca plan` shows one `sales` step per region, then `summary` in its own fan-in phase.
- The first `get` runs 4 steps. The second runs 0: every partition and the fan-in are served
  from cache. Add a region to the list and re-run: only the new partition and `summary` run.
- Each partition has its own artifact under `.barca/artifacts/`, for example
  `pipeline.py--sales_region_emea/`.

See also: `barca docs partitions`.
