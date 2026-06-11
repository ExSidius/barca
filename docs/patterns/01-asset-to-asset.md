# Pattern: Asset-to-Asset

Pure data pipelines. Assets chain via `inputs=`. Each step is cached and only re-runs when its inputs change.

## The right way

```python
from barca import asset

@asset
def raw_data():
    return load_csv("sales.csv")

@asset(inputs={"data": raw_data})
def cleaned(data):
    return data.dropna().reset_index(drop=True)

@asset(inputs={"data": cleaned})
def summary(data):
    return {"total": data["amount"].sum(), "count": len(data)}
```

```bash
barca get pipeline.py
```

Barca builds the DAG `raw_data -> cleaned -> summary`, hashes each step's code and inputs, and skips any step whose hash has not changed since the last run.

## Why this works

- **Hash-based invalidation.** Barca hashes the function source and the hashes of its upstream assets. If nothing changed, the step is skipped entirely.
- **Pure functions.** Each asset is a pure transformation: same inputs produce the same output. This makes caching safe.
- **Artifact-based data passing.** Intermediate results are serialized to disk (json, pickle, or parquet). Workers in different batches never share memory -- they read artifacts from the previous tier.

## Common mistakes

### Calling asset functions directly

```python
# Wrong -- bypasses the cache and the DAG entirely
@asset
def summary():
    data = cleaned()  # direct Python call, barca never sees this dependency
    return {"total": data["amount"].sum()}
```

Barca uses static analysis to discover the graph. If you call `cleaned()` inside the function body instead of declaring it via `inputs=`, barca does not know about the dependency. The step will not wait for `cleaned` to finish, and caching will not work.

### Mutating inputs in place

```python
# Wrong -- mutates the upstream artifact
@asset(inputs={"data": raw_data})
def cleaned(data):
    data.drop(columns=["junk"], inplace=True)  # modifies the original object
    return data
```

Even though barca serializes artifacts between tiers, within a single worker batch multiple steps may share references to the same deserialized object. Mutating in place can corrupt a value that another step in the same batch still needs. Always return a new object.
