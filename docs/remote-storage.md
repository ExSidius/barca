# Remote storage

Barca can store artifacts — the serialized outputs of every asset — in a
remote object store instead of the local `.barca/artifacts/` directory, and
`@sink` destinations can point at remote URIs directly. Azure ADLS Gen2 is
the first-class backend; S3 and GCS plug in through the same mechanism.

## Install the backend

Remote backends are optional extras — the core install stays dependency-free:

| Extra | Backend | URI schemes |
|---|---|---|
| `barca[azure]` | Azure ADLS Gen2 / Blob (adlfs) | `abfs://`, `abfss://` |
| `barca[s3]` | Amazon S3 (s3fs) | `s3://`, `s3a://` |
| `barca[gcs]` | Google Cloud Storage (gcsfs) | `gs://`, `gcs://` |
| `barca[remote]` | all of the above | |

```bash
uv add 'barca[azure]'
```

## Remote artifact store

Set `BARCA_ARTIFACT_URI` to a URI prefix and every materialized asset is
written there instead of `.barca/artifacts/`:

```bash
export BARCA_ARTIFACT_URI=abfss://artifacts@myaccount.dfs.core.windows.net/prod
barca get pipeline.py
```

Artifact paths recorded in the metadata DB and passed between workers are
then full URIs (e.g. `abfss://.../prod/pipeline.py--load_data.parquet`).
Downstream steps download their inputs to a local staging file on demand.

## Remote sinks

`@sink` paths accept the same URIs, independent of where the artifact store
lives:

```python
from barca import asset, sink

@asset
@sink('abfss://exports@myaccount.dfs.core.windows.net/daily/report.parquet')
def report():
    return build_dataframe()
```

A sink failure (missing extra, bad credentials, unreachable account) never
fails the parent asset — it is reported as `[barca] SINK FAILED: ...` and
recorded in the run's metadata.

## Staged writes

Serialized payloads are never buffered fully in memory — important when
assets are multi-hundred-MB DataFrames or pickled models:

1. The serializer (json/pickle/parquet) streams to a temp file — in the
   destination directory for local writes, in `.barca/staging/` for remote
   ones (deliberately on project disk, not `/tmp`, which is often RAM-backed
   tmpfs).
2. Local: the temp file is atomically renamed into place (`os.replace`).
   Remote: the temp file is uploaded with a chunked `put_file`; object
   stores commit the object only when the upload completes.
3. On any failure the temp file is removed — the destination never holds a
   partial artifact. Stale temp files from crashed workers are swept at
   worker startup.

Remote reads are symmetric: inputs are downloaded to `.barca/staging/`,
deserialized, and the temp file removed.

## Credentials

Barca passes no credentials — each backend uses its native default chain:

- **Azure (adlfs)**: `DefaultAzureCredential` — env vars
  (`AZURE_CLIENT_ID`/`AZURE_CLIENT_SECRET`/`AZURE_TENANT_ID`), managed
  identity, Azure CLI login, etc. `AZURE_STORAGE_ACCOUNT_NAME` /
  `AZURE_STORAGE_ACCOUNT_KEY` and connection strings also work.
- **S3 (s3fs)**: the standard boto chain — `AWS_ACCESS_KEY_ID`, profiles,
  instance metadata.
- **GCS (gcsfs)**: `google.auth` application default credentials.

For anything the default chains can't express, `BARCA_STORAGE_OPTIONS`
takes a JSON object keyed by fsspec protocol, splatted into the filesystem
constructor:

```bash
export BARCA_STORAGE_OPTIONS='{"abfs": {"account_name": "myaccount", "anon": false}}'
```

## v1 limitations

Two coordinator features read artifact files directly from local disk and
require a local artifact store (they are unaffected by remote *sinks*):

- **Dynamic partitions** (`partitions_from=...`) — the partition source
  artifact is read by the Rust coordinator. With a remote store the run
  fails with an explicit error.
- **`parallel()` return values** — child results are read back from JSON
  artifacts to resume the parent. With a remote store the parent receives
  `null` results and a warning is printed.

Both are candidates for a later release. Also note: artifacts are keyed by
node id, not content, so re-runs overwrite the same remote names; switching
`BARCA_ARTIFACT_URI` between runs does not invalidate cache rows that point
at the previous store.
