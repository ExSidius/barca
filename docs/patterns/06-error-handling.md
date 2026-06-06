# Error Handling

Retries are declared per-task (or per-asset) via decorator kwargs. Rust owns the retry loop -- you do not implement retries in Python.

## Declaring retries

```python
from barca import task, asset

@task(retries=3, retry_backoff=1.0)
def deploy_model(model: dict) -> None:
    upload_to_endpoint(model)
```

- `retries` -- total number of attempts. `retries=3` means up to 3 attempts (1 initial + 2 retries). Default is 1 (no retry).
- `retry_backoff` -- base backoff in seconds. Delay before attempt N is `retry_backoff * N`. Default is 0.

With `retries=3, retry_backoff=1.0`:

| Attempt | Delay before |
|---------|-------------|
| 1       | 0s (immediate) |
| 2       | 1.0s |
| 3       | 2.0s |

Retries work on assets too:

```python
@asset(retries=2, retry_backoff=0.5)
def flaky_api_data() -> dict:
    return call_unreliable_api()
```

## How the retry loop works

Rust's scheduler owns retries entirely:

1. The worker process runs the function.
2. If the function raises an exception, the worker reports failure back to Rust.
3. Rust waits for the backoff delay, then spawns a new worker attempt.
4. If all attempts are exhausted, the step is marked as failed.

Each attempt is a fresh worker invocation. There is no shared state between attempts.

## Failure propagation

Retries do not cascade. Each node in the DAG owns its own retry boundary.

```python
@task(retries=3, retry_backoff=1.0)
def deploy(model: dict) -> dict:
    return upload(model)  # retried up to 3 times

@task(inputs={"d": deploy}, retries=2)
def notify(d: dict) -> None:
    send_slack(f"deployed {d}")  # retried up to 2 times, independently
```

If `deploy` exhausts its 3 attempts and fails:

- `deploy` is marked failed.
- `notify` is **not attempted** -- its input dependency failed.
- `notify`'s own retries are irrelevant because it never started.

If `deploy` succeeds but `notify` fails:

- `notify` retries up to 2 times using its own retry policy.
- `deploy` is **not re-run** -- it already succeeded.

Parent retries never re-run child tasks, and child retries never trigger parent retries.

## Common mistakes

### Implementing retry loops in Python

```python
# Wrong -- Rust already handles this.
@task()
def deploy(model: dict) -> None:
    for attempt in range(3):
        try:
            upload(model)
            return
        except Exception:
            time.sleep(attempt * 1.0)
    raise RuntimeError("gave up")
```

This defeats Rust's retry tracking. The scheduler sees one attempt that either succeeds or fails after internal looping. Use `retries=3, retry_backoff=1.0` on the decorator instead.

### Expecting parent retries to cover sub-task failures

```python
@task(retries=5)
def release(model: dict) -> None:
    deploy(model)  # if deploy is a @task, calling it directly bypasses barca
    notify()
```

Calling `@task` functions directly is just a regular Python call -- barca does not manage it. If `deploy` is a separate `@task`, wire it through `inputs=` so the scheduler can retry it independently.

### Catching all exceptions to prevent retries

```python
# Wrong -- swallowing errors prevents Rust from retrying.
@task(retries=3)
def deploy(model: dict) -> None:
    try:
        upload(model)
    except Exception:
        log.error("failed")
        return  # silently succeeds -- no retry happens
```

If you want retries to work, let the exception propagate. Catch only exceptions you genuinely want to handle without retrying.
