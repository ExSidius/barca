# Single Asset, No Inputs

This document specifies the first concrete Barca workflow: a single decorated asset with no inputs.

It is intentionally narrow. If this flow is not clean, the rest of the system will not be clean either.

This workflow assumes the Barca core constraints documented in [../core-constraints.md](../core-constraints.md).

## Example

```python
from barca import asset


@asset()
def a() -> str:
    return "banana"
```

## User-facing behavior

The function remains a normal Python function:

```python
from my_project.assets import a

a()
# "banana"
```

Barca adds three capabilities around it:

1. Index the asset definition into the metadata store.
2. Materialize the asset into a persisted output.
3. Reuse the persisted output when the definition and runtime inputs have not changed.

The default freshness for `@asset()` is `Always` — the asset is kept up to date automatically during `barca run`.

## Asset identity vs materialization identity

We need two different identities.

### Logical asset identity

This is the stable, human-facing asset identity:

- explicit asset `name`, if provided
- module path
- file path
- function name

For this example, the logical asset ID is effectively:

```text
my_project/assets.py:a
```

This is what users refer to in CLI, UI, and API.

For continuity across code changes, the MVP should use:

- explicit asset `name` if present
- otherwise repo-relative file path + function name

If two currently indexed assets collide on that continuity key, Barca should fail indexing with a clear duplicate error.

### Materialization identity

This is the hash that decides whether we can reuse an existing output.

For the first version, this should not be only the raw function body hash. That is too weak.

Instead, define a `definition_hash` from:

- normalized function source
- decorator metadata
- declared output format / serializer
- Python version
- `uv.lock` hash
- Barca serializer protocol version

For this no-input example, `run_hash == definition_hash` because there are no upstream inputs or runtime params.

Later, when inputs exist:

```text
run_hash = hash(definition_hash + upstream_materialization_ids + explicit_params)
```

## Indexing flow

When Barca indexes the codebase:

1. Discover Python modules from configured source roots.
2. Import modules in an inspection mode.
3. Find functions decorated with `@asset`.
4. Extract:
   - module path
   - file path
   - function name
   - normalized function source
   - decorator metadata
   - return annotation
5. Validate that the return type is supported or explicitly configured.
6. Compute `definition_hash`.
7. Upsert the asset definition into Turso.

Even though this example has no dependencies, the indexing pass should still run graph validation so the same pipeline works for larger DAGs.

## How source inspection should work

Barca should primarily inspect assets dynamically, not just by parsing files statically.

The basic flow should be:

1. Import the module with `importlib`.
2. Resolve the decorated function object.
3. Use `inspect.getsource()` on the function object.
4. Use `inspect.getmodule()` and `inspect.getsource()` on the containing module.
5. Normalize the captured source before hashing and writing `code.txt`.

This is the right default because Barca is supposed to understand the function users actually wrote, not an invented framework wrapper around it.

For the `PREFIX` example, the important point is that we should not only save the function text. We should save enough inspected source context to invalidate correctly when module-level dependencies change.

For the MVP, that means:

- store the function source in `code.txt`
- hash the full module source for `definition_hash`
- store both the function source and module path metadata in Turso

This keeps the human-facing artifact readable while making cache invalidation safer.

## Why dynamic inspection is not enough by itself

This should be the primary mechanism, but not the only mechanism.

There are several edge cases:

- `inspect.getsource()` depends on Python being able to locate source files
- decorators can obscure the original function unless Barca preserves `__wrapped__`
- interactive definitions and notebooks are harder to make reproducible
- imported helpers and module constants still affect semantics even if they are not inside the function text

Because of that, the first implementation should treat dynamic inspection as the main source of truth and file/module source as the validation boundary.

In practice:

- use the live imported function object to discover the asset
- use the module source file to compute the module-level hash
- require importable source-backed modules for indexed assets in v1

That last constraint is reasonable for the MVP and avoids pretending notebook-defined assets are fully reproducible.

## Turso records

For this example, the minimum metadata records are:

### `assets`

- `asset_id`
- `logical_name`
- `module_path`
- `file_path`
- `function_name`
- `asset_slug`

### `asset_definitions`

- `definition_id`
- `asset_id`
- `definition_hash`
- `continuity_key`
- `source_text`
- `decorator_metadata_json`
- `return_type`
- `serializer_kind`
- `python_version`
- `uv_lock_hash`
- `status`
- `created_at`

### `materializations`

- `materialization_id`
- `asset_id`
- `definition_id`
- `run_hash`
- `status`
- `artifact_path`
- `artifact_format`
- `artifact_checksum`
- `created_at`

The important point is that both tables are append-only history.

When code changes:

- Barca inserts a new `asset_definitions` row
- old definitions remain
- old materializations remain
- prior materializations become stale or superseded relative to the newest definition

## Filesystem layout

The filesystem layout should be readable by humans, but Turso remains the source of truth.

For this example:

```text
.barcafiles/
  my-project-assets-py-a/
    <definition-hash-1>/
      code.txt
      metadata.json
      value.json
    <definition-hash-2>/
      code.txt
      metadata.json
      value.json
```

The slug directory should come from:

- relative file path
- filename
- function name

Example slug:

```text
my-project-assets-py-a
```

Each `definition-hash` directory is an immutable versioned location for one definition snapshot.

Old directories are not deleted when the asset changes.

## File contents

### `code.txt`

Contains the normalized function source captured from the live imported function object.

For this example:

```python
@asset()
def a() -> str:
    return "banana"
```

### `metadata.json`

Contains the minimal structured metadata needed to understand and validate the asset without querying the database.

Suggested fields:

```json
{
  "asset_name": "a",
  "module_path": "my_project.assets",
  "file_path": "my_project/assets.py",
  "function_name": "a",
  "definition_hash": "<hash>",
  "run_hash": "<hash>",
  "serializer_kind": "json",
  "python_version": "3.12",
  "return_type": "str",
  "inputs": [],
  "barca_version": "0.1.0"
}
```

### Output file

For this example, the return value is a string, so the output should be stored as JSON:

```json
"banana"
```

The filename should be format-driven:

- `value.parquet`
- `value.json`
- `value.yaml`
- `value.pkl`

For the MVP, JSON, Parquet, and pickle are enough. YAML can be deferred unless there is a real need for it.

## Execution flow

Materializing `a` should look like this:

1. Resolve the asset by logical name.
2. Load the latest indexed definition.
3. Recompute the current `definition_hash` from the live importable module as a preflight consistency check.
4. Refuse execution if the current imported definition does not match the indexed definition.
5. Compute `run_hash`.
6. Check Turso for a successful materialization with the same `run_hash`.
7. If found, return the existing artifact path.
8. If not found, claim the run.
9. Launch `uv run` with the Barca Python worker.
10. Import the target module and execute the real function from its original code location.
11. Validate the returned value against the supported output kinds.
12. Serialize the value to the correct format in a temp directory.
13. Atomically move the temp output into:
    - `.barcafiles/<asset-slug>/<definition-hash>/`
14. Insert the materialization record into Turso.
15. Release the claim and emit status updates to UI/TUI.

If the current code produced a new `definition_hash`, Barca should:

- index the new definition
- keep the old definition and its materializations
- mark older materializations as stale relative to the current definition in UI/query results

## Return-type rules for this example

This example returns `str`.

That raises an important design choice: do we support arbitrary JSON-serializable Python values, or only a smaller typed subset?

For the MVP, Barca should support:

- `pandas.DataFrame`
- `polars.DataFrame`
- JSON-serializable values
- explicit pickle values

That means `str` is allowed because it is JSON-serializable.

The Python SDK should make this explicit through validation and docs, not by silently guessing.

## Why the proposed shape is good

- The function is still normal Python.
- The output directory is inspectable by hand.
- The hash directory gives immutable versioned materializations.
- Turso handles indexing and lookup without turning artifacts into database blobs.
- historical definitions remain visible rather than being overwritten

## Holes in the current proposal

### Hashing only the full function body is not enough

If the function uses a helper or module constant, a function-body hash can incorrectly reuse stale outputs.

Example:

```python
PREFIX = "yellow"


@asset()
def a() -> str:
    return f"{PREFIX} banana"
```

Changing `PREFIX` must invalidate the asset even if the function body text stays the same.

The first implementation should therefore hash at least the full module source, not just the function body.

The clean split is:

- `code.txt`: the inspected function source, for humans
- `definition_hash`: derived from module-level source plus asset metadata and runtime envelope, for correctness

### Folder path should not be the source of truth

Using `filepath + filename + function name` as the primary identity breaks on renames and moves.

That path is good for readability, but Turso should be the real mapping from logical asset to current definitions and materializations.

### Approximate history should not silently rewrite identity

It is useful to preserve approximate history for assets that move or get renamed.

But source similarity should only be used as a hint, not as automatic identity, because copied or lightly edited functions can otherwise merge incorrectly.

For the MVP:

- continuity is keyed by explicit asset name or file path + function name
- possible rename/move matching can be surfaced later as advisory metadata
- duplicate live continuity keys should fail loudly

### `definition-hash` may not be enough once inputs exist

For input-free assets, `definition_hash` is enough.

For assets with dependencies, the reusable output must be keyed by `run_hash`, not only `definition_hash`.

Otherwise downstream assets will incorrectly reuse stale input combinations.

### Writing directly into the final directory is risky

Writes must go through a temp location and then be atomically moved into place.

Otherwise interrupted runs can leave partially written artifacts that look valid.

### Stored source should not be executed directly

`code.txt` is a provenance artifact, not an execution artifact.

The worker should always import and execute the real user module under `uv run`.

That keeps imports, helper resolution, and tracebacks accurate.

### YAML should probably not be in the MVP

YAML is user-friendly but weaker as a canonical storage format than JSON.

Unless there is a strong use case, MVP should likely support:

- JSON
- Parquet
- pickle

## Implementation notes

### Rust side

- indexing service discovers and validates assets
- metadata service reads/writes Turso
- executor launches `uv run` workers
- API server exposes run status and artifact metadata
- Datastar UI subscribes to run events
- TUI reads the same backend state

### Python side

- `@asset` decorator registers asset metadata
- worker loads the target function by module path and function name
- serializer layer maps supported return values to output formats
- validation layer gives clear errors for unsupported outputs

## Acceptance criteria for this example

- A user can define the `a()` asset exactly as above.
- Barca can index it without materializing it.
- Barca can materialize it into `.barcafiles/.../value.json`.
- Re-running without relevant code or environment changes reuses the prior materialization.
- Changing the module source invalidates the cached result and creates a new definition directory.
- The function remains directly callable outside Barca.
