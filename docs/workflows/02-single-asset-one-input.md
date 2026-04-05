# Single Asset With One Upstream Input

This document specifies the second concrete Barca workflow: one asset depending on one upstream asset.

This is the first workflow that forces the real API shape for dependency declaration, input loading, and cache reuse.

This workflow assumes the Barca core constraints documented in [../core-constraints.md](../core-constraints.md).

## Example

```python
from barca import asset


@asset()
def a() -> str:
    return "banana"


@asset(inputs={"fruit": a})
def b(fruit: str) -> str:
    return fruit.upper()
```

## Why this API shape

The function `b` is still just a normal Python function:

```python
from my_project.assets import b

b("banana")
# "BANANA"
```

Dependency wiring lives entirely in decorator metadata:

```python
@asset(inputs={"fruit": a})
```

That is the key design choice.

Barca should not inject special framework objects into function parameters, and it should not require users to write `b(a())`-style orchestration code.

The decorated function stays regular Python. Barca separately knows where to load `fruit` from when running the asset orchestrated.

## Proposed decorator API

For the MVP, the `@asset` decorator should support this shape:

```python
@asset(
    name: str | None = None,
    inputs: dict[str, AssetRefLike] | None = None,
    serializer: SerializerKind | None = None,
    description: str | None = None,
    tags: dict[str, str] | None = None,
)
```

Where `AssetRefLike` means:

```python
AssetRef | Callable
```

The preferred user-facing sugar is passing the upstream asset function directly:

```python
@asset(inputs={"fruit": a})
def b(fruit: str) -> str:
    return fruit.upper()
```

Barca should canonicalize that immediately into a stable internal asset reference during indexing.

### Canonicalization rule

If a user passes a function object like `a`, Barca should resolve it to a canonical ref such as:

```text
my_project/assets.py:a
```

That canonical ref is what gets stored in Turso and on disk.

Function objects are authoring sugar, not persisted identity.

### Why not persist raw function objects

Passing the function directly is good syntax, but a raw Python function object is not a good long-term identity because:

- canonical refs are easier to validate
- it is not portable across processes
- it is awkward to persist in metadata
- forward references still need another mechanism
- wrapped or non-top-level functions are not stable enough to treat as asset IDs

So the correct model is:

- function-object input declarations for ergonomics
- canonical asset references for storage and execution

### Why keep `asset_ref(...)` at all

Even if function-object sugar is the default ergonomic path, `asset_ref(...)` is still useful for:

- forward references
- cross-module refs that should not require eager imports
- future selectors such as versions, partitions, or tags

For example:

```python
from barca import asset, asset_ref


@asset(inputs={"fruit": asset_ref("my_project/assets.py:a")})
def b(fruit: str) -> str:
    return fruit.upper()
```

This should remain supported, but it does not need to be the first example.

## Things the API should not do

For the MVP, the API should not support:

- dependency injection through function signatures
- mixing function objects and asset refs in one dependency map
- hidden global context during plain Python execution

Mixing function objects and asset refs in one dependency map is not fundamentally wrong, but it is unnecessary complexity for the first implementation. Barca should normalize either form to the same internal reference model.

Those all make notebook and direct-function use worse.

## User-facing execution helpers

This workflow requires a minimal helper API:

```python
from barca import load_inputs, materialize, read_asset

kwargs = load_inputs(b)
# {"fruit": "banana"}

result = b(**kwargs)
# "BANANA"

artifact = materialize(b)
resolved = read_asset("my_project/assets.py:b")
```

The default `load_inputs()` behavior should be:

- return kwargs, not a tuple
- resolve the latest successful upstream materialization
- raise clear errors if an input cannot be resolved

This is a strong fit for regular Python because users can always do:

```python
b(**load_inputs(b))
```

That is exactly the kind of notebook-friendly workflow Barca should optimize for.

## Asset identity and dependency identity

For this workflow:

- `a` has a logical asset ID
- `b` has a logical asset ID
- `b`'s definition metadata stores that parameter `fruit` depends on asset `a`

Example logical IDs:

```text
my_project/assets.py:a
my_project/assets.py:b
```

The dependency mapping stored for `b` should look conceptually like:

```json
{
  "fruit": {
    "asset_ref": "my_project/assets.py:a"
  }
}
```

Even if the user wrote `inputs={"fruit": a}`, the stored representation should still look like this.

## Hash model

This is where the first real split between `definition_hash` and `run_hash` matters.

### `definition_hash`

For `b`, the definition hash should include:

- module source hash
- decorator metadata
- declared serializer
- return annotation
- Python version
- `uv.lock` hash
- Barca protocol version

### `run_hash`

For `b`, the run hash should include:

- `definition_hash`
- resolved upstream materialization IDs
- explicit runtime params, if any

That means:

- changing `b`'s code changes `definition_hash`
- changing `a`'s materialized output changes `b`'s `run_hash`
- unchanged `b` code plus unchanged upstream materialization means cache reuse
- returning to a previously seen combination of `b` definition and upstream materialization IDs should reuse the old `b` materialization

### Non-monotonic code history

Freshness should not be tied to “latest run.”

Consider:

```python
@asset()
def a(x: int) -> int:
    return x
```

then later:

```python
@asset()
def a(x: int) -> int:
    return x**2
```

then later back to:

```python
@asset()
def a(x: int) -> int:
    return x
```

If Barca already has:

- the old `a` materialization for the original definition
- the corresponding `b` materialization whose `run_hash` used that original `a` materialization

then `b` should be fresh again immediately when `a` returns to that original definition and provenance.

No recomputation should be required.

## Indexing flow

When Barca indexes these modules:

1. Import the module.
2. Discover `a` and `b` as decorated assets.
3. Inspect each function and module source.
4. Validate the dependency map on `b`.
5. Confirm that `fruit` exists as a parameter on `b`.
6. Confirm that the referenced upstream asset exists.
7. Validate that the asset graph is acyclic.
8. Compute and upsert asset definitions for both `a` and `b`.

Validation for `b` should fail if:

- the decorator references a parameter that is not in the function signature
- the referenced asset cannot be found
- the same parameter is declared twice
- the resolved upstream output is obviously incompatible with the downstream annotation when that can be checked
- a function object is provided that is not a top-level Barca asset
- the dependency graph contains a cycle

## Materialization flow

Materializing `b` should work like this:

1. Resolve `b` by logical asset ID.
2. Load `b`'s current asset definition.
3. Recompute the current `definition_hash` from the live importable module as a preflight consistency check.
4. Refuse execution if the current imported definition does not match the indexed definition.
5. Read `b`'s input map.
6. Resolve the upstream asset for `fruit`, which is `a`.
7. Ensure `a` has a usable materialization:
   - reuse if current
   - compute if missing or stale
8. Build input kwargs:
   - `{"fruit": <value from a>}`
9. Compute `b`'s `run_hash` using:
   - `b`'s `definition_hash`
   - `a`'s chosen materialization ID
10. Reuse an existing successful materialization for `b` if the `run_hash` matches.
11. Otherwise claim the run and execute `b` in the Python worker via `uv run`.
12. Import the real module and function from the user codebase.
13. Call `b(**kwargs)`.
14. Serialize and publish the artifact.
15. Record provenance from `b`'s materialization to `a`'s materialization.

The crucial point is that “usable materialization” means “a successful materialization whose provenance matches the current plan,” not “the most recently produced one.”

## `load_inputs()` behavior

For this workflow, `load_inputs(b)` should:

1. Resolve `b`'s decorator metadata.
2. Resolve each declared input asset.
3. Read the selected upstream artifact values.
4. Deserialize them into plain Python values.
5. Return a kwargs dict keyed by parameter name.

Example:

```python
load_inputs(b)
# {"fruit": "banana"}
```

This helper is not an afterthought. It is one of the main reasons the API stays usable outside the orchestrator runtime.

## Filesystem layout

Assuming both assets live in `my_project/assets.py`, the filesystem might look like:

```text
.barcafiles/
  my-project-assets-py-a/
    <definition-hash-a>/
      code.txt
      metadata.json
      value.json
  my-project-assets-py-b/
    <definition-hash-b>/
      code.txt
      metadata.json
      value.json
```

For this workflow, `metadata.json` for `b` must include upstream provenance references.

Suggested `metadata.json` shape for `b`:

```json
{
  "asset_name": "b",
  "module_path": "my_project.assets",
  "file_path": "my_project/assets.py",
  "function_name": "b",
  "definition_hash": "<hash-b>",
  "run_hash": "<hash-b-run>",
  "serializer_kind": "json",
  "return_type": "str",
  "inputs": {
    "fruit": {
      "asset_ref": "my_project/assets.py:a",
      "materialization_id": "<materialization-id-a>"
    }
  }
}
```

## Turso records

This workflow needs one more metadata concept beyond the first example.

### `asset_inputs`

- `definition_id`
- `parameter_name`
- `upstream_asset_id`
- `selector_json`

### `materialization_inputs`

- `materialization_id`
- `parameter_name`
- `upstream_materialization_id`

This split matters:

- `asset_inputs` describes the declared dependency
- `materialization_inputs` describes the concrete resolved dependency for one run

This is what allows Barca to reuse old downstream results when upstream code returns to a previously seen state.

## Type behavior

For this example:

- `a` returns `str`
- `b` accepts `fruit: str`
- `b` returns `str`

That should round-trip through JSON serialization cleanly.

For the MVP, Barca should validate:

- declared parameter names
- supported output type of the upstream asset
- supported output type of the downstream asset

Barca should not try to solve full Python type-checking at runtime.

It is enough to fail clearly when the serializer contract is impossible or obviously inconsistent.

## Critical holes and tradeoffs

### Function-object sugar is nice but not universal

`inputs={"fruit": a}` is the nicest normal case, but it does not cover every dependency declaration case.

Later, Barca can consider ergonomic sugar such as:

- forward refs
- relative refs
- selectors
- aliases

So the first implementation should support both:

- `inputs={"fruit": a}`
- `inputs={"fruit": asset_ref("my_project/assets.py:a")}`

with the function-object form shown as the preferred happy path.

### Automatic upstream materialization can surprise users

If `materialize(b)` silently materializes `a`, that is convenient but also implicit.

That is still the right default, but the CLI and UI should show the full planned execution graph before running.

### Recency should not override exact provenance matches

If the newest upstream materialization does not match the currently planned provenance but an older one does, Barca should choose the older exact match.

The cache key is the provenance identity, not the wall-clock order.

### `load_inputs()` needs a selection policy

“Latest successful upstream materialization” is fine for the MVP, but later users will want:

- latest overall
- latest matching current code context
- specific version
- explicit timestamp or tag selectors

The API should leave room for that:

```python
load_inputs(b, selector="latest")
```

### Definition-level directory names may become awkward

The folder layout is understandable for manual browsing, but once multiple materializations exist under one definition, the storage model may need another nested level or a stronger convention.

For this exact workflow, one materialization per `run_hash` is enough to describe the semantics. The filesystem layout may later evolve to make `run_hash` more explicit than it is here.

## Recommended implementation stance

For the MVP:

- prefer direct function-object dependency declarations
- support `asset_ref(...)` as the canonical escape hatch
- store dependency declarations on the decorator
- return kwargs from `load_inputs()`
- keep function execution plain and direct
- compute `run_hash` from exact upstream materialization IDs
- let Turso hold the provenance graph
- reject cycles during indexing and planning
- require `uv` for indexing and execution

That is the minimum API that is both usable and honest.

## Acceptance criteria

- A user can define `a` and `b` exactly as shown.
- Barca indexes both assets and stores the dependency mapping.
- `load_inputs(b)` returns `{"fruit": "banana"}`.
- `b(**load_inputs(b))` works in normal Python.
- `materialize(b)` computes `a` first if needed.
- Re-running `materialize(b)` reuses cached materializations when neither `b` nor `a` changed.
- Changing `a` invalidates `b` through `run_hash`, even if `b`'s own definition did not change.
- Changing `b` invalidates only `b`, not `a`.
- Changing `a`, then changing it back to a previously seen definition, allows Barca to reuse the old matching `b` materialization without recomputation.
