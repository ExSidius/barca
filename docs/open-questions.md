# Open Design Questions

Observations from stress-testing the asset model against real-world use cases. These are edges to watch, not necessarily problems to solve now.

## Addressed by the current model

These were initially flagged as concerns but are already handled by existing primitives.

### Incremental processing

Sensors are the incremental processing primitive. A sensor scopes work to "what changed since last check" and downstream assets process only that delta. The separation is clean: the sensor owns change detection, the asset owns transformation.

```python
@sensor(freshness=Schedule("*/5 * * * *"))
def new_files():
    files = detect_new_files_since_last_check()
    return (bool(files), files)

@asset(inputs={"files": new_files}, freshness=Always)
def process(files):
    _, file_list = files
    return [parse(f) for f in file_list]
```

### Dynamic partitions

`partitions_from(upstream)` derives partition values from an upstream asset at plan time. The DAG shape is known statically (the `PartitionSource` edge exists at parse time), but the values are resolved dynamically. This handles the "partition keys not known at definition time" case.

## Edges worth watching

These are real scenarios that the current model handles imperfectly or not at all. They're ordered by how commonly a data team would encounter them.

### Long-running assets with no checkpointing

A 4-hour model training job that fails at hour 3 loses all progress. An asset either succeeds and returns a value, or fails — there's no partial progress or resumable execution. Workflow 9 (execution controls) specs timeouts, retries, and cancellation, which helps, but doesn't solve mid-execution checkpointing.

**Who hits this:** ML teams, large batch ETL.

**Possible directions:**
- Checkpoint callback that the asset can call to save intermediate state
- Resume-from-checkpoint on retry, passing the last checkpoint as an extra input
- This could compose with the existing retry policy (workflow 9) rather than requiring a new primitive

### Accumulating state across sensor ticks

A sensor fires every 5 minutes and returns new files. The downstream asset processes that batch. But what if you need a running total — e.g., "all files seen today"? The asset only sees the current sensor output, not the history.

**Who hits this:** Ingestion pipelines that need session windows or daily aggregation of streaming-like inputs.

**Possible directions:**
- The asset reads its own previous materialization as an input (self-referential edge — currently disallowed as a cycle)
- A `@stateful_asset` variant that receives `(previous_output, new_input)`
- Leave this to the user: the asset function can read its own prior state from a database or file, outside the DAG
- A sensor-level accumulator that maintains state across ticks

### Multi-output functions

One expensive computation that produces several distinct outputs consumed by different downstream assets. Today you'd return a dict and have each downstream pick its key, or duplicate the computation across multiple assets.

**Who hits this:** Feature engineering (one training run produces multiple feature tables), ETL (one API call returns data for multiple entities).

**Possible directions:**
- `@asset(outputs=["features", "labels", "metadata"])` that produces named outputs, each independently cacheable and consumable
- Dagster's `@multi_asset` is prior art here
- Simpler alternative: a single asset returns a dict, and downstream assets declare which key they consume — caching still works because the upstream run_hash is the same

### Conditional execution

"If the sensor detects CSVs, run path A; if Parquet, run path B." The DAG is static — determined at parse time — so conditional edges aren't expressible at the graph level.

**Who hits this:** Pipelines that handle heterogeneous input formats, feature flag-gated processing.

**Possible directions:**
- Handle inside the asset function with if/else (loses graph-level visibility but works)
- A `@router` node type that inspects upstream output and activates/deactivates downstream branches
- Probably fine to leave as "do it in your function" for a long time — conditional DAGs are a complexity magnet

### Effect chains

"Send email, then update CRM, then log to audit table." Effects are leaf-only — you can't chain them or use one effect's confirmation as input to another. The workaround is to model intermediate steps as assets that return confirmation payloads.

**Who hits this:** Post-pipeline notification/integration workflows.

**Possible directions:**
- Probably the right answer is "don't" — if you need procedural effect chains, that's a workflow engine's job, not an orchestrator's
- The asset-returning-confirmation workaround is slightly unnatural but works and preserves caching/provenance

## Explicitly out of scope

These are legitimate needs but belong to different tools. Barca should not try to solve them.

- **Human-in-the-loop approval gates** — "transform data, wait for human approval, then publish." This is workflow orchestration (Temporal, Step Functions).
- **Infrastructure orchestration** — "spin up a Spark cluster, run a job, tear it down." This is infra automation (Terraform, Pulumi, Airflow).
- **General procedural task graphs** — ordered sequences of arbitrary actions with no data flow between them. This is task scheduling (Airflow, Celery).
