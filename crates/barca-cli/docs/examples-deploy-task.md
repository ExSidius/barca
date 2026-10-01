# Example: an asset feeding a task

A cached asset produces a model; a task deploys it. The task always runs, the asset only when
stale.

```python
from barca import asset, task


@asset()
def model() -> dict:
    return {"version": 3, "metrics": {"auc": 0.91}}


@task(inputs={"m": model})
def deploy(m: dict) -> None:
    print(f"deploying model v{m['version']} (auc {m['metrics']['auc']})")
```

```bash
barca run deploy pipeline.py
barca run deploy pipeline.py
barca run deploy pipeline.py --refresh model
```

What to notice:

- First run: 2 steps (`model`, then `deploy`). Second run: 1 step, because `model` is served
  from cache and only the task re-runs.
- `--refresh model` forces `model` to re-materialize, so that run is 2 steps again.
  `--refresh-all` (alias `--no-cache`) refreshes every upstream asset.
- The task's `print` goes to stderr; stdout stays a single JSON line.
- `barca get deploy pipeline.py` exits 1 and tells you to use `barca run`.

See also: `barca docs tasks`, `barca docs cache`.
