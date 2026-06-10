# Getting Started

## Prerequisites

- Python >= 3.12
- [uv](https://docs.astral.sh/uv/) package manager

## Install

```bash
uv init --app my-project
cd my-project
uv add barca
```

This installs the `@asset()` decorator and the `barca` CLI into your project's virtualenv.

## Hello world

Create `pipeline.py`:

```python
from barca import asset

@asset()
def hello() -> dict:
    return {"message": "Hello from barca!"}
```

Run it:

```bash
uv run barca get pipeline.py
```

You'll see structured JSON on stdout and timing on stderr:

```
[barca] 1 nodes, 0 edges, 1 phases, 1 streams | plan: 0.8ms | exec: 37ms | total: 38ms
```

The `@asset()` decorator itself does nothing at runtime -- it's an identity function. Your code runs exactly the same with or without barca installed.

## Add a dependency

Assets depend on other assets via `inputs=`. Barca resolves the DAG and executes them in the right order.

```python
from barca import asset

@asset()
def raw_data() -> list[dict]:
    return [{"x": 1}, {"x": 2}, {"x": 3}]

@asset(inputs={"data": raw_data})
def summary(data: list[dict]) -> dict:
    return {"count": len(data), "sum": sum(d["x"] for d in data)}
```

```bash
uv run barca get pipeline.py
```

Barca sees that `summary` depends on `raw_data`, creates two phases, executes `raw_data` first, then passes its output to `summary` as the `data` kwarg.

## See the plan

You can see what barca will do without executing anything:

```bash
uv run barca plan pipeline.py
```

```json
{
  "total_steps": 2,
  "phases": [
    {
      "reason": "Independent",
      "streams": [{"stream_id": 0, "steps": ["raw_data"]}]
    },
    {
      "reason": "Dependent",
      "streams": [{"stream_id": 1, "steps": ["summary"]}]
    }
  ]
}
```

## Next steps

- [Guide](guide.md) -- full tutorial covering tasks, sensors, partitions, parallel dispatch, freshness markers, and multi-file pipelines
- [Patterns](patterns/02-asset-to-task.md) -- common patterns and anti-patterns
- [CLI Reference](cli.md) -- all CLI commands
