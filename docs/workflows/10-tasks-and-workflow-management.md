# Tasks And Workflow Management

This document specifies how Barca models **tasks** — the workflow-management
steps that live alongside asset pipelines but don't fit the asset model.

Even the cleanest data-science setups carry a tail of "do something" work:
deploy a model, notify a channel, run a migration, warm a cache, copy files,
trigger an external system. These *do* something rather than produce cacheable
data, and they should always re-run. Modeling them as assets is awkward (they
have no meaningful cached output); modeling them as side-effect leaves is too
restrictive (they often sit in the middle of a flow and feed other steps).

Barca models them with a first-class `@task` node.

## Summary

`@task` is the task-style counterpart to `@asset`:

| | `@asset` | `@task` |
|---|----------|---------|
| Cached? | Yes | **No** — always re-runs |
| Position in DAG | anywhere | anywhere |
| May depend on | assets, sensors | assets, sensors, tasks |
| May be an input to | assets, sensors, tasks | **tasks only** |
| Run with | `barca get <asset>` | `barca run <task>` |

The one hard rule: **a task may not be an input to an asset or sensor.** A task
always re-runs, so feeding its output into a cacheable node would keep that node
perpetually stale. Assets and sensors may freely be upstream of tasks.

You "**get**" an asset (cache-aware, maximally cached) and "**run**" a task
(always executes). Barca encourages asset-based workflows but lets you sprinkle
tasks in — it constructs and manages the DAG for you; you just don't get the
caching for the task nodes.

## Declaring tasks

```python
from barca import asset, task

@asset()
def model() -> dict:
    return train(...)

# asset → task: a task that consumes an upstream asset.
@task(inputs={"m": model})
def deploy(m: dict) -> dict:
    upload_to_endpoint(m)
    return {"deployment_id": "abc123"}

# task → task: a nested task consuming another task's output.
@task(inputs={"deploy": deploy})
def notify(deploy: dict) -> None:
    send_slack(f"deployed {deploy['deployment_id']}")
```

### Ordering without data: `after=`

Tasks often have execution order but no data to pass (migrate → warm_cache →
notify). Declare ordering-only dependencies with `after=[...]`. No data flows
along these edges — they only force the referenced nodes to run first.

```python
@task()
def migrate(): ...

@task(after=[migrate])
def warm_cache(): ...

@task(after=[warm_cache])
def notify(): ...
```

## Running tasks: `barca run`

```bash
barca run <task> file.py [--burst a,b]
```

- **Default** — every upstream `@asset` in the task's cone is **burst**
  (force-rerun), so the run reflects fresh inputs end to end.
- **`--burst a,b`** — only the named assets are force-rerun; every other asset
  uses the cache normally. Use this to re-run selectively (e.g. re-train one
  model while keeping expensive upstream data cached).
- Tasks and sensors always re-run regardless of the burst set.

Because the DAG is built from your declared dependencies, **targeting any node
scopes the run to exactly that subtree.** Running a top-level "release" task
pulls in everything behind it; running one sub-task runs just that sub-task and
its upstreams.

```bash
barca run notify_team iris_project/assets.py          # whole subtree
barca run smoke_test  nested_project/assets.py         # scoped to one sub-task
barca run release --burst trained_model assets.py      # selective re-run
```

## Worked examples

- [`examples/basic_app`](https://github.com/ExSidius/barca/tree/main/examples/basic_app) —
  a task consuming an asset (`log_summary`) and an `after=` chain
  (`migrate → warm_cache → notify`).
- [`examples/iris_pipeline`](https://github.com/ExSidius/barca/tree/main/examples/iris_pipeline) —
  a realistic mixed pipeline: data assets feed `deploy_model` (asset → task) and
  `notify_team` (task → task).
- [`examples/nested_tasks`](https://github.com/ExSidius/barca/tree/main/examples/nested_tasks) —
  nested task hierarchies, `after=` ordering, assets-feeding-tasks, and run
  scoping.

## Future work

For v1, task composition is expressed through declared dependencies
(`inputs=` / `after=`) and scoped by targeting a node with `barca run`. Runtime
**call-nesting** — a task body literally invoking other `@task` functions, with
Barca statically tracing the calls to build the sub-DAG — is deferred to a later
iteration.
