# Barca

**The invisible asset orchestrator.** Rust plans it. Python runs it. You just write functions.

Barca is a hybrid Rust+Python asset orchestrator that discovers functions decorated with `@asset()`, `@sensor()`, and `@task()`, builds a dependency graph via static analysis (no imports), generates a phased execution plan, and dispatches work to Python workers -- all in under 40ms of framework overhead.

```python
from barca import asset

@asset()
def raw_data() -> list[dict]:
    return [{"x": 1}, {"x": 2}, {"x": 3}]

@asset(inputs={"data": raw_data})
def summary(data: list[dict]) -> dict:
    return {"count": len(data), "total": sum(d["x"] for d in data)}
```

```bash
barca get pipeline.py
```

## Install

```bash
pip install barca
```

## How it works

1. **Rust binary** parses Python source using ruff's AST (pure static analysis, no import)
2. Builds a petgraph DAG from decorator metadata
3. Generates a phased execution plan (phases run sequentially, streams within a phase run in parallel)
4. Spawns Python workers per phase, passing inputs via temp JSON files
5. Collects outputs from worker stdout (JSON lines protocol)
6. Persists results to `.barca/metadata.db` (Turso/libSQL)

## Node kinds

| Kind | Decorator | Cached | Can be input |
|------|-----------|--------|-------------|
| **asset** | `@asset()` | Yes | Yes |
| **sensor** | `@sensor()` | No (always re-runs) | Yes |
| **task** | `@task()` | No (always re-runs) | No (leaf node) |

## Docs

- [Guide](guide.md) -- step-by-step tutorial from first asset to full pipeline
- [Architecture](architecture.md) -- system design and internals
- [Benchmarks](benchmarks.md) -- performance comparison vs. Dagster and Prefect
- [API Reference](api/decorators.md) -- decorator and helper docs
