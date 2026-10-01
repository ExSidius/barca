---
title: Conditional Execution
description: Barca has no built-in conditional primitive — conditionals are just Python inside the function body.
---

Barca does not have a built-in conditional primitive. Conditionals are just Python.

## The right way

Write your branching logic inside the task or asset body:

```python
from barca import asset, task

@asset()
def validation_report() -> dict:
    results = run_validation_suite()
    return {"passed": results.all_ok, "details": results.summary}

@task(inputs={"result": validation_report})
def act_on_validation(result: dict) -> None:
    if result["passed"]:
        deploy_to_prod()
    else:
        notify_failure(result["details"])
```

The DAG structure stays fixed. The runtime behavior varies based on the data flowing through it.

This works the same way for assets:

```python
@asset(inputs={"raw": raw_data})
def cleaned(raw: dict) -> dict:
    if raw["format"] == "v2":
        return parse_v2(raw)
    else:
        return parse_legacy(raw)
```

## Why this is the right model

Barca handles execution order, caching, and data passing. Control flow is your code. There is no reason to push conditionals into the framework layer because:

- Python already has `if`/`else`, `match`, and exception handling.
- The DAG is built statically from decorators. Dynamic DAG shapes would break caching and make plans non-deterministic.
- Keeping conditionals in function bodies means you can test them with plain `pytest` -- no orchestrator needed.

## Common mistakes

### Trying to express conditionals in decorators

```python
# Wrong -- there is no `when=` parameter.
@task(when=lambda: env == "prod")
def deploy(): ...
```

Barca decorators declare identity, inputs, and execution policy. They do not express runtime control flow. Put the condition inside the function body.

### Splitting branches into separate DAG paths

```python
# Unnecessary complexity.
@task(inputs={"r": validation_report})
def deploy_if_passed(r: dict) -> None:
    if not r["passed"]:
        return  # no-op
    deploy_to_prod()

@task(inputs={"r": validation_report})
def notify_if_failed(r: dict) -> None:
    if r["passed"]:
        return  # no-op
    notify_failure(r["details"])
```

This runs both tasks every time, with one silently doing nothing. A single task with an `if`/`else` is simpler and clearer.
