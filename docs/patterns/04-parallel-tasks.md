# Pattern: Parallel Tasks

> Coming in a future release.

Run multiple independent tasks concurrently within a single function using the `parallel()` primitive.

## The planned API

```python
from barca import task, parallel

@task
def task_a():
    return fetch_from_api("service-a")

@task
def task_b():
    return fetch_from_api("service-b")

@task
def task_c():
    return fetch_from_api("service-c")

@task
def aggregate():
    a, b, c = parallel(task_a(), task_b(), task_c())
    return merge(a, b, c)
```

`parallel()` will accept any number of task or asset calls and return their results in argument order. Under the hood, barca will schedule them into the same execution tier so they run in separate worker processes simultaneously.

## Current behavior

Without `parallel()`, barca determines concurrency from the DAG structure. Steps that share the same tier (i.e., have no dependencies on each other) already run in parallel across worker processes.

```python
# These two assets have no dependency relationship, so barca
# already runs them in parallel in separate workers.
@asset
def users():
    return load("users.csv")

@asset
def products():
    return load("products.csv")

# This asset depends on both, so it runs in the next tier.
@asset(inputs={"u": users, "p": products})
def report(u, p):
    return join(u, p)
```

The `parallel()` primitive will add the ability to express concurrency explicitly inside a function body, rather than relying solely on DAG-level tier assignment.

## What to use today

Structure your DAG so that independent steps have no edges between them. Barca will automatically schedule them in the same tier and dispatch them to separate workers. This gives you parallelism without any special syntax.
