---
title: Philosophy
description: Barca is to Airflow, Dagster, and Prefect what DuckDB is to Snowflake.
---

Barca is to Airflow, Dagster, and Prefect what DuckDB is to Snowflake.

## The problem

Every mainstream data orchestrator asks you to become its tenant. You adopt its project structure, learn its DSL, deploy its server, and rewrite your functions to fit its opinions. The orchestrator becomes the center of gravity — your code orbits it.

This is backwards. The orchestrator exists to serve the code, not the other way around.

Most teams don't need a platform. They need the *capabilities* of a platform — dependency resolution, caching, staleness tracking, parallel execution — without the operational overhead of running one.

## The position

Barca is an embedded orchestrator. It adds DAG semantics to plain Python functions the way DuckDB adds OLAP semantics to plain files. No server. No config. No project scaffolding. No framework to learn.

You write normal Python:

```python
@asset()
def raw_data() -> list[dict]:
    return load_from_source()

@asset(inputs={"data": raw_data})
def cleaned(data: list[dict]) -> list[dict]:
    return [clean(row) for row in data]
```

These are real functions. They run standalone with `python my_pipeline.py`. The decorators are identity functions — pure no-ops. Barca reads your source statically, builds the DAG from the decorator signatures, and handles the rest: what to run, in what order, what to cache, what to skip.

Your code never knows the orchestrator is there.

## Design principles

**Invisible by default.** If the orchestrator adds perceptible latency, requires manual steps, or forces you to think about it during normal development, it's failing. The planning phase exists in Rust specifically so that overhead drops below human perception — not because Rust is fashionable, but because invisibility is a hard performance requirement.

**Static analysis, never import.** Barca parses your Python source as text via AST. It never imports your modules during planning. This makes the orchestrator a genuine observer: it reasons *about* your code without *becoming* part of your code. No import side effects, no environment coupling, no accidental execution.

**Your code is the source of truth.** Barca doesn't generate code, doesn't require config files, and doesn't own your project structure. Your Python functions are the pipeline definition. Barca discovers them, infers the graph, and gets out of the way.

**Single install, zero config.** `uv add barca` gives you everything — the CLI, the decorators, the runtime. No `barca init`, no YAML, no manifest. Barca scans your project for decorated functions and builds the graph automatically.

**Scales down to a single script.** The minimum viable Barca project is one Python file with one decorated function. There is no setup cost. If you later need partitions, scheduling, sensors, or a server — those capabilities are there, but they're opt-in, not required.

**Hyper-performant.** Performance isn't a feature — it's a prerequisite for invisibility. Planning happens in microseconds. Execution uses free-threaded Python for true parallelism. Caching is content-addressed so identical work is never repeated. The framework should be the fastest part of your pipeline, never the bottleneck.

**Flexible and extensible.** Barca has strong opinions about how little it should impose, not about how you should write your code. Sensors bring external state in. Effects push state out. Partitions fan work out. Schedules drive reconciliation. Each primitive composes with the others, and `@unsafe` exists as an explicit escape hatch when static analysis can't follow your code.

## Why asset-based

Everything in Barca is an asset, a sensor, or an effect. This is a deliberate constraint, not a limitation.

The asset model gives you strong guarantees the way Rust's ownership model gives you memory safety — it feels rigid until you realize it eliminates entire classes of bugs. When every node in the graph is a function that takes data and returns data:

- **Caching is automatic.** If the inputs haven't changed, the output hasn't changed. No manual cache invalidation, no stale data.
- **Staleness propagates.** Change an upstream asset's code and every downstream consumer knows it's stale — without you tracking dependencies manually.
- **Provenance is free.** Every output is traceable to the exact code version, upstream versions, and partition key that produced it.
- **The graph is statically analyzable.** Barca can reason about your entire pipeline without running any of it.

The three node types cover the full surface area of data work:

- **Assets** produce and cache values. They're the core abstraction — pure functions from data to data.
- **Sensors** observe external state and bring it into the graph. They return `(update_detected, output)` tuples, making them the incremental processing primitive — the sensor scopes work to "what changed," and downstream assets process only that delta.
- **Effects** push state out of the graph — write to a database, send an email, call an API. They're leaf nodes that never cache and can't be consumed by other nodes.

Partitions extend the model to parallel and dynamic workloads. `partitions()` declares a static fan-out. `partitions_from()` derives partition values from an upstream asset at plan time, enabling dynamic partition universes that change as data changes. `collect()` fans partitions back in for aggregation.

### What the asset model doesn't try to do

Barca is not a general-purpose workflow engine. It doesn't do procedural task chains, human-in-the-loop approval gates, or infrastructure orchestration. Those are legitimate needs served by tools like Temporal, Airflow, or Step Functions. Barca's scope is data pipelines — where the fundamental operation is "function takes data, returns data." DuckDB doesn't try to be Postgres. Barca doesn't try to be Temporal.

## What this means in practice

| | Platform orchestrators | Barca |
|---|---|---|
| Install | Server + workers + config | `uv add barca` |
| Startup overhead | Seconds to minutes | Imperceptible |
| Learning curve | Framework-specific patterns | Write normal Python |
| Minimum project | Team with infra support | One file, one function |
| Code ownership | Framework owns the structure | You own everything |
| Runs without the orchestrator | No | Yes — decorators are no-ops |
