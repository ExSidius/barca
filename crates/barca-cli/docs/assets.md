# Assets, inputs and freshness

An asset is a function whose output barca caches and tracks.

```python
from barca import asset, Always, Manual, Schedule


@asset()                                   # freshness=Always (default)
def raw() -> dict:
    return {"x": 1}


@asset(inputs={"data": raw}, retries=3, retry_backoff=2.0, timeout_seconds=600)
def clean(data: dict) -> dict:
    return {"x": data["x"] + 1}


@asset(freshness=Manual)                   # only recomputed on explicit refresh
def pinned() -> dict:
    return {"x": 0}


@asset(freshness=Schedule("0 5 * * *"))    # fires daily at 05:00 under `barca serve`
def daily() -> dict:
    return {"x": 2}
```

## Options on `@asset`

| Option | Meaning |
|---|---|
| `inputs={"param": upstream}` | Wire upstream nodes to parameters. The param name must match a function parameter. |
| `name="..."` | Explicit node name (the default is the function name). |
| `freshness=` | `Always` (default), `Manual`, or `Schedule("<cron>")`. |
| `serializer=` | Force `"json"`, `"pickle"` or `"parquet"`. See `barca docs types`. |
| `partitions=` | Fan one asset out over keys. See `barca docs partitions`. |
| `timeout_seconds=` | Per-attempt time limit (default 300). |
| `retries=` | Total attempts on failure; 1 means no retry. |
| `retry_backoff=` | Base delay in seconds; the delay grows linearly with the attempt number. |
| `description=`, `tags=` | Metadata. |

## How a node is identified

A node id is `<file>:<function>` (for example `pipeline.py:clean`), or the explicit `name=`.
Targets on the command line can use the bare function name (`barca get clean pipeline.py`).
Use `asset_ref("other/file.py:raw")` inside `inputs=` to reference a node in another file
without importing it.

## Naming

An asset is an ordinary Python function, so its name is an ordinary Python name. If a file
imports a module (`import carry_forward_registry`) and also defines an asset with the same
name, the `def` rebinds the name and shadows the module for the rest of the file. Give the
import an alias (`import carry_forward_registry as cf_registry`) or rename the asset. Barca does
not warn about this; Python simply uses the later definition.

## Static analysis

Planning never imports your code. The decorators, `inputs=` and `freshness=` must be written
literally enough for barca to read them from the source. Dynamic decorator construction
(building `inputs` in a loop, calling a decorator through a variable) is not visible to the
planner. Mark code barca cannot reason about with `@unsafe` (silences purity warnings only).

## Sensors

`@sensor` observes external state and returns `(update_detected: bool, value)`. Sensors have no
inputs and must use `Manual` or `Schedule(...)` freshness, never `Always`.

See also: `barca docs tasks`, `barca docs cache`, `barca docs scheduling`.
