# Pattern: Ordering-Only Dependencies

When you need one step to run after another but do not need the upstream step's data. Use the `_` prefix convention to skip artifact deserialization.

## The right way

```python
from barca import task

@task
def migrate_db():
    run_migrations()

@task(inputs={"_migrate": migrate_db})
def seed_data():
    insert_seed_records()
```

The `_migrate` parameter name starts with `_`, which tells barca to establish the DAG edge (ensuring `migrate_db` finishes before `seed_data` starts) but skip deserializing the upstream artifact. The function body never references `_migrate`.

## Why this works

- **No wasted I/O.** Without the `_` prefix, barca would serialize `migrate_db`'s return value to disk, then deserialize it into `seed_data`'s worker process -- all for a value you never use. The `_` prefix eliminates both the write and the read.
- **Intent is explicit.** Anyone reading the code immediately sees that the dependency is for ordering, not data flow. The parameter exists in the signature to satisfy static analysis, but the `_` prefix signals "don't bother loading this."
- **DAG is still correct.** The edge is still present in the execution plan. Barca will still schedule `seed_data` in a later tier than `migrate_db`.

## Common mistakes

### Using a normal parameter name and ignoring it

```python
# Works but wasteful
@task(inputs={"migrate": migrate_db})
def seed_data(migrate):  # never used, but barca still deserializes it
    insert_seed_records()
```

This is functionally correct but forces barca to serialize and deserialize an artifact you never read. On large return values this can add meaningful latency. Use the `_` prefix to opt out.

### Trying to use `after=`

```python
# Wrong -- after= was removed
@task(after=[migrate_db])
def seed_data():
    insert_seed_records()
```

Early prototypes of barca had an `after=` keyword for ordering-only edges. This was removed in favor of the `_` prefix convention on `inputs=`, which keeps a single mechanism for all dependency types. If you see `after=` in old examples, replace it with `inputs={"_name": upstream}`.
