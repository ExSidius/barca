# Notebook Workflow

**Status: Implemented** — See `packages/barca-core/src/barca/_notebook.py` and `tests/test_notebook.py`.

This document specifies how Barca assets should be used from a notebook.

This is a core workflow, not an afterthought.

The Barca API should feel like normal Python in a notebook, with orchestration helpers layered on top rather than replacing ordinary function calls.

This workflow assumes the Barca core constraints documented in [../core-constraints.md](../core-constraints.md).

## Summary

The key notebook helper is:

```python
load_inputs(asset_fn) -> dict[str, object]
```

That means users should be able to do:

```python
from my_project.assets import b
from barca import load_inputs

kwargs = load_inputs(b)
result = b(**kwargs)
```

The important point is that `b` is still just a Python function.

Barca helps load the right inputs, but it does not change how the function is called.

## Example

Given:

```python
from barca import asset


@asset()
def a() -> str:
    return "banana"


@asset(inputs={"fruit": a})
def b(fruit: str) -> str:
    return fruit.upper()
```

the notebook workflow should be:

```python
from my_project.assets import a, b
from barca import load_inputs, materialize, read_asset


# direct plain-Python usage still works
a()
b("banana")


# orchestration-assisted notebook usage
kwargs = load_inputs(b)
# {"fruit": "banana"}

result = b(**kwargs)
# "BANANA"


# optional persistence / orchestration helpers
materialize(b)
read_asset(b)
```

## Why this matters

A lot of orchestrators make notebook use awkward because the function signature is no longer the real function interface.

Barca should avoid that.

The notebook contract should be:

- asset functions stay ordinary callables
- Barca provides helper functions for loading inputs and reading prior outputs
- users can move fluidly between notebook experimentation and orchestrated execution

## `load_inputs()` contract

For the MVP:

```python
load_inputs(asset_fn) -> dict[str, object]
```

The return value must always be a dictionary keyed by the asset function’s parameter names.

For the example above:

```python
load_inputs(b)
# {"fruit": "banana"}
```

This should be true even if there is only one input.

Barca should not return:

- positional tuples
- framework wrapper objects
- special lazy proxy values

Those all make notebook usage worse.

## What `load_inputs()` should do

Given an asset function:

1. inspect the asset decorator metadata
2. resolve each declared input asset or sensor
3. select the current upstream value
4. deserialize it into a plain Python object
5. return a kwargs dict

That kwargs dict should be directly callable:

```python
asset_fn(**load_inputs(asset_fn))
```

## Selection policy

For the MVP, the default selection policy should be:

- use the latest successful upstream materialization or observation that matches current provenance rules

That keeps the default simple and predictable.

Later, Barca can extend this to support selectors such as:

- latest overall
- latest matching current context
- specific materialization ID
- specific partition
- specific historical timestamp or tag

But the notebook workflow should start with one obvious default.

## Partitioned assets in notebooks

For partitioned assets, notebook usage should still be explicit and kwargs-based.

Example:

```python
from my_project.assets import normalize_prices
from barca import load_inputs

kwargs = load_inputs(normalize_prices, partition={"ticker": "AAPL"})
result = normalize_prices(**kwargs, ticker="AAPL")
```

The same principle holds:

- Barca loads dependency inputs
- the notebook user still calls the function as plain Python

If Barca later adds a helper like:

```python
load_call(normalize_prices, partition={"ticker": "AAPL"})
```

that can be an ergonomic addition, but `load_inputs()` should remain kwargs-only and dependency-focused.

## Sensors in notebooks

Sensors should also participate cleanly in notebook workflows.

Example:

```python
from my_project.sensors import inbox_files
from barca import read_asset

observation = read_asset(inbox_files)
```

For assets that depend on sensors:

```python
kwargs = load_inputs(parse_inbox)
```

should return a normal dictionary of deserialized values sourced from the latest selected sensor observation.

## Effects in notebooks

Effects should remain ordinary functions too.

Example:

```python
from my_project.effects import publish_rows
from barca import load_inputs

kwargs = load_inputs(publish_rows)
publish_rows(**kwargs)
```

This is useful for testing or manual operator workflows in notebooks.

The same rule still holds:

- Barca loads declared inputs
- the user still calls the real function directly

## Why Barca should not inject hidden runtime context

Notebook usability gets much worse if asset functions depend on hidden runtime state.

For the MVP, Barca should not require:

- ambient execution contexts
- special session objects
- framework-specific argument wrappers
- magic parameter injection

The notebook experience should be understandable from ordinary Python alone.

## Recommended helper surface

For notebook usage, the minimal useful helper set is:

```python
load_inputs(asset_or_effect_fn) -> dict[str, object]
read_asset(asset_or_sensor_fn, ...)
materialize(asset_fn, ...)
list_versions(asset_or_sensor_fn)
```

This is enough to make notebooks pleasant without overbuilding the Python API.

## UI and notebook consistency

The notebook model should match the orchestrator model exactly.

That means:

- the same dependency metadata used by the reconciler powers `load_inputs()`
- the same selected upstream materializations shown in the UI are what `load_inputs()` resolves
- notebook execution and orchestrated execution should differ mainly in who calls the function, not in what function is being called

## Acceptance criteria

- `load_inputs(asset_fn)` returns a dictionary.
- The dictionary keys match the function parameter names declared in decorator inputs.
- `asset_fn(**load_inputs(asset_fn))` works as ordinary Python.
- The same workflow works for assets with asset inputs and sensor inputs.
- Effects can also use `load_inputs(...)` for manual notebook invocation.
- Notebook usage does not require special runtime context injection.
