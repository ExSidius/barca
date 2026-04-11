# Decorators

Core decorators for defining assets, sensors, effects, sinks, and related primitives.

## @asset

```python
@asset(
    name: str | None = None,
    inputs: dict[str, AssetRefLike] | None = None,
    partitions: dict[str, PartitionSpecLike] | None = None,
    serializer: SerializerKind | None = None,
    freshness: Freshness = Always,
    timeout_seconds: int = 300,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Declares a cacheable, provenance-tracked asset. The default freshness is `Always` — the asset is kept up to date automatically during `barca run`.

```python
from barca import asset, Always, Manual, Schedule

@asset()                                    # freshness=Always (default)
def my_asset() -> dict:
    return {"x": 1}

@asset(freshness=Manual)                    # only via explicit refresh
def pinned_data() -> dict:
    return {"x": 1}

@asset(freshness=Schedule("0 5 * * *"))     # daily at 05:00
def daily_report() -> dict:
    return {"x": 1}
```

`Manual` freshness blocks downstream `Always` assets from auto-updating — a downstream asset cannot be fresher than its most-upstream `Manual` dependency.

::: barca.asset
    options:
      show_source: false

## AssetWrapper

::: barca.AssetWrapper

## @sink

```python
@sink(
    path: str,
    serializer: SerializerKind,
)
```

Stacked on an `@asset` to write the asset's output to a path when it materialises. Paths are fsspec-compatible (local, `s3://`, `gs://`, etc.). Multiple `@sink` decorators may be stacked on the same asset.

```python
from barca import asset, sink, Always, JsonSerializer

@asset(freshness=Always)
@sink('./output.json', serializer=JsonSerializer)
@sink('s3://my-bucket/output.json', serializer=JsonSerializer)
def banana() -> dict:
    return {'a': 1}
```

Sinks are leaf nodes — no other asset may list a sink as an input. A sink failure does not fail the parent asset, but is surfaced prominently in `assets list` and job logs.

::: barca.sink
    options:
      show_source: false

## @sensor

```python
@sensor(
    name: str | None = None,
    freshness: Manual | Schedule = Manual,
    timeout_seconds: int = 300,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Declares an external-state observer. Sensors must use `Manual` or `Schedule` freshness — `Always` is not valid for sensors (polling frequency must be declared explicitly).

Sensors return `(update_detected: bool, output)` tuples. The full tuple is passed as input to downstream assets.

```python
from barca import sensor, Schedule

@sensor(freshness=Schedule("*/5 * * * *"))
def inbox_files() -> tuple[bool, list[str]]:
    files = list(Path("inbox").glob("*.csv"))
    return len(files) > 0, [str(f) for f in files]
```

Sensors are source nodes only — they have no upstream inputs.

::: barca.sensor
    options:
      show_source: false

## SensorWrapper

::: barca.SensorWrapper

## @effect

```python
@effect(
    name: str | None = None,
    inputs: dict[str, NodeRefLike] | None = None,
    freshness: Freshness = Always,
    timeout_seconds: int = 300,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Declares a standalone side-effect function — sending email, writing to a database, calling an external API. Effects take upstream inputs and produce no meaningful output. Use `@effect` for arbitrary side-effects; use `@sink` for writing asset outputs to file paths.

Effects are leaf nodes — no other asset may list an effect as an input.

```python
from barca import asset, effect, Always

@asset()
def report() -> dict:
    return {"rows": 42}

@effect(inputs={"data": report}, freshness=Always)
def send_email(data: dict) -> None:
    print(f"Sending report: {data}")
```

::: barca.effect
    options:
      show_source: false

## EffectWrapper

::: barca.EffectWrapper

## @unsafe

```python
@unsafe
def my_asset() -> str:
    return global_config["value"]
```

Marks a function as unsafe — it references globals, performs I/O, or otherwise cannot be tracked by AST analysis. `@unsafe` silences purity warnings; caching behaviour is unchanged. Barca makes no correctness guarantee for unsafe assets.

::: barca.unsafe
    options:
      show_source: false

## Schedule

```python
Schedule("0 5 * * *")   # cron expression
```

Constructs a schedule freshness value. Use inside `freshness=` on any decorator.

::: barca.Schedule

## partitions

::: barca.partitions

## Partitions

::: barca.Partitions
