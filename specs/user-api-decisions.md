# User API Decisions

Why the barca user API is shaped the way it is. Each section covers a decision,
the alternatives we considered, and the reasoning.

## Assets vs tasks: two concepts, not one

**Decision:** Assets and tasks are distinct node types with different decorators
(`@asset` vs `@task`), different commands (`get` vs `run`), different caching
behavior, and a hard rule that tasks can't feed assets.

**Alternative considered:** A single `@step` decorator with `cache=True/False`.
Fewer concepts, simpler surface area.

**Why we rejected it:** Assets and tasks serve different user mindsets. When
you're thinking about data, you want guarantees around staleness, speed, and
parallelization — the asset model is opinionated and handles state for you so
you can focus on your work. When you're thinking about tasks, the model is
simpler — you're doing things, maybe in parallel, maybe in sequence. The task
model deliberately doesn't bring the baggage and opinions of the asset model.

A unified `@step` would force users to think about caching on every function
and blur the line between "produce data" and "do something." Keeping them
separate means the system provides the right defaults for each case: assets
cache by default, tasks never cache. Users don't make caching decisions — the
node type makes them.

## Tasks can't be inputs to assets (but this may soften)

**Decision:** DAG validation rejects edges from tasks to assets or sensors.

**Alternative considered:** Allow it, but make the downstream asset uncacheable
(since the task always re-runs, the asset's inputs are never stable).

**Why we enforce it (for now):** The ban is a guardrail that pushes users toward
better patterns. The typical case is "do some side-effect work, then generate
data" — like syncing from GCS to AWS, then building an asset from the AWS data.
But this couples the side-effect to the data pipeline unnecessarily. The better
pattern is: run the sync task independently, then add a sensor to AWS that
detects when new data is available, and have the asset depend on the sensor.
This preserves caching, decouples the workflows, and makes each piece
independently testable.

That said, the ban isn't absolute. There are valid cases where you want to say
"always do X before generating Y." We may soften this to a warning rather than
an error in a future release — making the asset uncacheable when it depends on
a task, and documenting the tradeoff.

## Decorators are no-ops

**Decision:** `@asset()`, `@sensor()`, `@task()` are identity functions. They
return the decorated function unchanged. All metadata is extracted by Rust via
static analysis of the AST.

**Alternative considered:** Dagster's approach — decorators wrap the function
and modify its behavior (adding context objects, I/O managers, etc.).

**Why we rejected it:** Users should be able to import their functions and use
them like any regular Python code. Framework decorators that change function
behavior get in the way of that. They make it hard to work in a Jupyter
notebook, hard to unit test without fixtures, and hard to reason about what
the function actually does.

From an orchestration standpoint, wrapping isn't necessary. The Rust binary
extracts everything it needs from the source text. Keeping decorators as no-ops
reduces Python overhead and keeps the boundary between "user code" and
"orchestrator" clean. The orchestrator observes your code — it doesn't become
part of it.

There's room for lightweight instrumentation in the decorators later (Datadog/
Sentry tracing, for example), but they should stay thin.

## Static analysis, never import

**Decision:** Barca parses Python source files using ruff's AST parser in Rust.
It never imports user modules during planning.

**Alternative considered:** Import the module, inspect the decorated objects,
build the graph from runtime metadata. This is what Dagster, Prefect, and
Airflow do.

**Why we rejected it:** Speed was the primary motivation. Importing Python
modules is slow — 30ms+ per import with dependencies, and a large pipeline
might import hundreds of modules. AST parsing in Rust takes <5ms for 2000
assets. The planning phase must be invisible (sub-100ms), and importing
makes that impossible.

Beyond speed: import side effects are dangerous. A module that connects to a
database on import will do so during planning, which should be a read-only
operation. Static analysis avoids this entirely — the planner reads text, not
code.

**What we give up:** Complex metaprogramming (dynamically generating decorated
functions, conditional decorator application, decorators in loops) isn't
visible to static analysis. In practice, for clean and relatively simple data
pipelines, static analysis works. If users write straightforward decorated
functions, the parser handles it.

## `inputs=` with function references

**Decision:** Dependencies are declared via `inputs={"param": upstream_function}`,
where the value is a Python function reference (not a string).

**Alternatives considered:**
- String-based: `inputs={"data": "raw_data"}` — simpler but no IDE support
- Implicit by parameter name: function param `raw_data` auto-resolves to the
  asset named `raw_data` — magic, hard to debug
- Separate graph file: define the DAG in YAML/TOML/Python — separates the
  graph from the code it describes

**Why function references:** They're clean, require the upstream to actually
exist (ImportError at module load if not), prevent circular definitions, and
are self-explanatory. You get autocomplete, go-to-definition, and rename
refactoring for free from your IDE. The graph definition lives right next to
the code it describes.

String references are supported via `asset_ref("path:name")` for cross-file
cases where you can't import the function directly, but function references
are the primary API.

## `barca get` vs `barca run`: two commands

**Decision:** Assets are materialized with `barca get`, tasks are executed with
`barca run`. Using the wrong command on the wrong node type gives a clear error.

**Alternative considered:** A single `barca execute` command that infers the
behavior from the node type.

**Why two commands:** Intent clarity. `get` communicates that barca will try to
do the least work possible — if the asset is cached, you just get it. `run`
communicates that the thing runs, period. The verb itself tells you the contract:
get is lazy (cache-first), run is eager (always executes).

This also gives natural homes for different defaults. `get` is cache-aware by
default. `run` bursts all upstream assets by default (because when you run a
deploy, you usually want fresh data). These defaults would be awkward flags on
a single command.

## `parallel()` takes ready-to-call functions

**Decision:** `parallel()` accepts `functools.partial` instances — functions with
all parameters pre-bound, ready to call with `()`.

**Alternatives considered:**
- `@parallel_task` decorator marking functions as parallelizable
- `fan_out(fn, items)` that takes a function + argument list
- Tuple syntax: `parallel((fn, args, kwargs), (fn2, args2, kwargs2))`

**Why partials:** The goal is to ask for something very simple — give us a
function that's ready to go. A partial cleanly packages the function reference
and its arguments into one object. Users already know `functools.partial` from
stdlib. It composes naturally with generators and comprehensions:
`parallel(*(partial(f, x) for x in items))` is just Python, no special syntax.

The tuple syntax would require users to learn a barca-specific input format.
A decorator would be too static (you can't dynamically choose what to
parallelize). `parallel_map(fn, items)` exists as sugar for the common case
but `parallel()` with partials is the general-purpose primitive.

## Ordering-only deps use `_` prefix, not `after=`

**Decision:** To declare "run X after Y but don't pass Y's data," use
`inputs={"_y": y}`. The `_` prefix signals ordering-only; the parameter
receives `None`.

**Alternative considered:** An `after=` keyword on the decorator:
`@task(after=[migrate, seed])`.

**Why `_` prefix:** Early prototypes had `after=`, but it added a second
dependency mechanism alongside `inputs=`. Having one way to declare
dependencies (through `inputs=`) and using a naming convention to distinguish
data deps from ordering deps is simpler. It's one concept, not two.

The `_` prefix is a well-known Python convention for "unused." The pattern
`inputs={"_migrate": migrate}` reads naturally: "this depends on migrate but
I'm not using the value." The DAG edge is still present, the ordering is
enforced, but no data passes.

## Sensors return `(updated, data)` tuples

**Decision:** Sensor functions return a 2-tuple. The `updated` boolean tells
the orchestrator whether new data was detected. Only `data` is passed to
downstream consumers.

**Alternative considered:** Sensors just return data, and the orchestrator
compares it to the previous value to detect changes.

**Why explicit `updated`:** Comparison-based change detection is fragile — it
requires serializing and diffing the full output on every check. For sensors
that observe large external datasets, this is expensive and error-prone.
Letting the sensor author decide what constitutes "updated" is both cheaper
and more correct. The sensor knows its domain.

The tuple is stripped transparently — downstream assets receive just the data
component. The `updated` flag is metadata for the orchestrator, invisible to
consumer code.
