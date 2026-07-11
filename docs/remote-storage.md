# Remote storage

Barca can store artifacts — the serialized outputs of every asset — in a
remote object store instead of the local `.barca/artifacts/` directory, and
`@sink` destinations can point at remote URIs directly. Amazon S3, Google
Cloud Storage, and Azure ADLS Gen2 are all first-class; Cloudflare R2 rides
on the S3 backend (it speaks the S3 API).

## Install the backend

Remote backends are optional extras — the core install stays dependency-free:

| Extra | Backend | URI schemes |
|---|---|---|
| `barca[s3]` | Amazon S3 (s3fs) | `s3://`, `s3a://` |
| `barca[r2]` | Cloudflare R2 (s3fs) — S3-compatible | `s3://` + R2 endpoint |
| `barca[gcs]` | Google Cloud Storage (gcsfs + google-cloud-storage) | `gs://`, `gcs://` |
| `barca[azure]` | Azure ADLS Gen2 / Blob (adlfs) | `abfs://`, `abfss://` |
| `barca[remote]` | all of the above | |

```bash
uv add 'barca[s3]'
```

Every backend is held to the **same shared-state contract** — conditional
create, cross-machine cache hit, concurrent-writer conflict → replay — by a
backend conformance suite that runs on every PR against local emulators
(MinIO for S3/R2, fake-gcs-server for GCS, Azurite for Azure). See
[Releases](releases.md) for the guarantees each backend makes.

## Remote mode: shared state + artifacts

Point `[remote].uri` in `barca.toml` (or `BARCA_REMOTE_URI`) at an object
store prefix and barca shares **both** artifacts and materialization state
across machines:

```toml
# barca.toml
[remote]
uri = "abfss://pipelines@myaccount.dfs.core.windows.net/barca/my-project"
```

- Artifacts are written **content-addressed** to
  `{uri}/{env}/artifacts/{node}/{run_hash}{ext}` — immutable objects, so a
  cache hit on one machine is valid on every machine.
- The metadata DB (the turso/SQLite file that records materializations and
  run history) lives as a single blob at `{uri}/{env}/state/metadata.db`.
  Each run **pulls** it first — so cache checks see every machine's
  materializations — and **pushes** it back at the end with an
  etag/generation-conditional upload. If another machine pushed first, barca
  re-pulls and replays this run's rows, so nothing is lost (bounded by
  `push_retries`).
- Before upload the WAL is checkpointed into the main file, so the blob is
  always a complete standalone SQLite database — you can download it and
  open it with stock `sqlite3`.
- A run that pulls successfully but crashes mid-way uploads nothing; its
  local rows are discarded by the next pull and those steps recompute.

The result: a run on VM-B hits artifacts materialized by VM-A with zero
re-execution. See [Configuration](config.md) for the full schema,
environment separation (`--env`), and env-var overrides.

`barca serve` does not support shared state yet — set `state = "off"` for
served projects.

### Artifacts-only mode (0.4.0 behavior)

Set `BARCA_ARTIFACT_URI` to a URI prefix and every materialized asset is
written there instead of `.barca/artifacts/`, while metadata stays local:

```bash
export BARCA_ARTIFACT_URI=abfss://artifacts@myaccount.dfs.core.windows.net/prod
barca get pipeline.py
```

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

- **S3 (s3fs)**: the standard boto chain — `AWS_ACCESS_KEY_ID`, profiles,
  instance metadata.
- **GCS**: `google.auth` application default credentials. Artifact I/O uses
  gcsfs; the shared-state path uses the `google-cloud-storage` SDK directly
  (gcsfs cannot express a generation precondition on overwrite) — both read
  the same ADC chain.
- **Azure (adlfs)**: `DefaultAzureCredential` — env vars
  (`AZURE_CLIENT_ID`/`AZURE_CLIENT_SECRET`/`AZURE_TENANT_ID`), managed
  identity, Azure CLI login, etc. `AZURE_STORAGE_ACCOUNT_NAME` /
  `AZURE_STORAGE_ACCOUNT_KEY` and connection strings also work.

For anything the default chains can't express, `BARCA_STORAGE_OPTIONS`
takes a JSON object keyed by fsspec protocol, splatted into the filesystem
constructor (equivalently, `[remote.storage_options.<protocol>]` in
`barca.toml`):

```bash
export BARCA_STORAGE_OPTIONS='{"abfs": {"account_name": "myaccount", "anon": false}}'
```

### Cloudflare R2

R2 is S3-compatible, so it uses the `s3://` schemes with the S3 backend
(`barca[r2]` or `barca[s3]`) pointed at your account's R2 endpoint. Set the
endpoint in `storage_options` under the `s3` protocol; credentials are your
R2 access key / secret via the usual boto env vars:

```toml
# barca.toml
[remote]
uri = "s3://my-bucket/barca/my-project"

[remote.storage_options.s3]
client_kwargs = { endpoint_url = "https://<account-id>.r2.cloudflarestorage.com" }
```

```bash
export AWS_ACCESS_KEY_ID=<r2-access-key-id>
export AWS_SECRET_ACCESS_KEY=<r2-secret-access-key>
```

R2 supports the same `If-Match` conditional writes barca's shared state relies
on. As with S3, the state blob must stay under the 48 MiB single-request limit
(the coordinator errors clearly if it grows past that).

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
