---
title: "Pattern: Parallel Tasks"
description: Run multiple independent tasks concurrently within a single task function using parallel().
---

Run multiple independent tasks concurrently within a single task function using the `parallel()` primitive.

## The right way

### 1. Static known calls

When you know the exact set of work at write time, pass explicit partials:

```python
from functools import partial
from barca import task, parallel

@task
def deploy_us(model):
    return push_to_region("us-east-1", model)

@task
def deploy_eu(model):
    return push_to_region("eu-west-1", model)

@task
def deploy_all(model):
    us_result, eu_result = parallel(
        partial(deploy_us, model),
        partial(deploy_eu, model),
    )
    return {"us": us_result, "eu": eu_result}
```

### 2. Dynamic fan-out

When the set of work is determined at runtime, unpack a generator of partials:

```python
from functools import partial
from barca import task, parallel

@task
def deploy(region, model):
    return push_to_region(region, model)

@task
def deploy_all(model):
    regions = ["us-east-1", "eu-west-1", "ap-southeast-1"]
    results = parallel(*(partial(deploy, r, model) for r in regions))
    return dict(zip(regions, results))
```

### 3. parallel_map sugar

For the common case of applying one task across an iterable with shared kwargs:

```python
from barca import task, parallel_map

@task
def deploy(region, model):
    return push_to_region(region, model)

@task
def deploy_all(model):
    regions = ["us-east-1", "eu-west-1", "ap-southeast-1"]
    results = parallel_map(deploy, regions, model=model)
    return dict(zip(regions, results))
```

`parallel_map(fn, items, **kwargs)` is equivalent to `parallel(*(partial(fn, item, **kwargs) for item in items))`.

## How it works

- Each argument to `parallel()` must be a `functools.partial` wrapping a `@task`-decorated function.
- The parent worker sends the work items to Rust via Unix domain socket.
- Rust freezes the parent worker (SIGSTOP), spawns a temp replacement to maintain pool capacity, and adds the child items to the global ready queue.
- Rust assigns children to idle workers, which execute them in parallel.
- When all children complete, Rust kills the temp replacement, resumes the parent (SIGCONT), and sends the results back.
- Results are collected and returned to the parent in argument order.
- Each result is either the return value or a `ParallelError` instance (Promise.allSettled semantics -- no branch failure crashes the parent).

## Error handling

Failed branches do not raise exceptions in the parent. Instead, the corresponding position in the results tuple contains a `ParallelError` object:

```python
from barca import task, parallel, ParallelError
from functools import partial

@task
def deploy(region, model):
    return push_to_region(region, model)

@task
def deploy_all(model):
    regions = ["us-east-1", "eu-west-1", "ap-southeast-1"]
    results = parallel(*(partial(deploy, r, model) for r in regions))

    for region, result in zip(regions, results):
        if isinstance(result, ParallelError):
            log.error(f"Deploy to {region} failed: {result.error}")
        else:
            log.info(f"Deploy to {region} succeeded: {result}")

    return results
```

Key behaviors:

- In v0.2.0, parallel branches use default retry settings (1 attempt, no retry). Per-branch retry configuration via the sub-task's `retries=` policy is planned for a future release.
- `ParallelError.error` contains the stringified exception from the failed branch.
- The parent task continues executing after `parallel()` returns, regardless of branch failures.

## Common mistakes

### 1. Passing non-partial callables

```python
# Wrong -- bare function reference, not a partial
results = parallel(deploy_us, deploy_eu)

# Right
results = parallel(partial(deploy_us, model), partial(deploy_eu, model))
```

All arguments must be `functools.partial` instances so Rust can serialize the function target and arguments for dispatch to worker processes.

### 2. Expecting exceptions to propagate

```python
# Wrong -- this will not catch branch failures
try:
    results = parallel(partial(deploy_us, model), partial(deploy_eu, model))
except Exception:
    handle_failure()

# Right -- check each result individually
results = parallel(partial(deploy_us, model), partial(deploy_eu, model))
for r in results:
    if isinstance(r, ParallelError):
        handle_failure(r)
```

### 3. Using parallel() in an @asset body

```python
# Wrong -- parallel() is only valid inside @task functions
@asset
def my_asset():
    return parallel(partial(fetch_a), partial(fetch_b))

# Right -- use @task for imperative orchestration
@task
def my_task():
    return parallel(partial(fetch_a), partial(fetch_b))
```

Assets define the static DAG and cannot spawn dynamic sub-work. Use `@task` for imperative orchestration patterns.

### 4. Calling parallel() with raw function calls (eager evaluation trap)

```python
# Wrong -- deploy_us(model) executes immediately in the parent process
results = parallel(deploy_us(model), deploy_eu(model))

# Right -- partial defers execution to the child workers
results = parallel(partial(deploy_us, model), partial(deploy_eu, model))
```

Passing `deploy_us(model)` calls the function eagerly in the parent, then passes the return value (not a partial) to `parallel()`. Use `partial()` to defer execution to the spawned worker processes.
