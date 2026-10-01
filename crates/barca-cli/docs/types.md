# Types and output formats

How barca stores each step's output and how it hands values to downstream steps.

## Output: the returned value picks the format

| Step returns | Stored as |
|---|---|
| pandas `DataFrame`, polars `DataFrame`/`LazyFrame`, pyarrow `Table`, duckdb relation | parquet (`.parquet`) |
| JSON-serializable value (dict, list, str, int, float, bool, None) | json (`.json`) |
| anything else | pickle (`.pkl`, protocol 5) |

- Override with `@asset(serializer="json" | "pickle" | "parquet")`.
- The **return annotation does not choose the format**; the value (or `serializer=`) does.
- Asking for parquet on a value that cannot be written as parquet falls back to pickle and
  prints a warning on stderr.
- Lazy values are materialized when the step ends: a polars `LazyFrame` is collected and a
  duckdb relation is executed straight into the parquet file. Nothing lazy crosses a step.
- pandas parquet needs `pyarrow` (`pip install "barca[parquet]"`).

## Input: parameter annotations pick the reader

For parquet artifacts, the annotation on the *consuming* parameter selects the reader:

| Annotation | Downstream receives |
|---|---|
| none | pandas `DataFrame` (default) |
| `pd.DataFrame` / `pandas.DataFrame` | pandas `DataFrame` |
| `pl.DataFrame` / `polars.DataFrame` / `pl.LazyFrame` | polars `DataFrame` |
| `pyarrow.Table` | pyarrow `Table` |
| `duckdb.DuckDBPyRelation` | duckdb relation over the parquet file (lazy read) |

The same upstream parquet can be read differently by different consumers. Annotations are
parsed statically, so use the conventional names above (`pd`, `pl`, `pyarrow`, `duckdb`).
json and pickle artifacts ignore annotations.

```python
import duckdb
import polars as pl
from barca import asset


@asset()
def orders() -> duckdb.DuckDBPyRelation:      # written as parquet
    return duckdb.sql("select 1 as id, 9.5::double as amount")


@asset(inputs={"orders": orders})
def total(orders: duckdb.DuckDBPyRelation) -> dict:   # read as a duckdb relation
    return {"total": orders.sum("amount").fetchone()[0]}


@asset(inputs={"orders": orders})
def as_polars(orders: pl.DataFrame) -> pl.DataFrame:  # same file, read with polars
    return orders.with_columns(doubled=pl.col("amount") * 2)
```

## Reading results back

`barca get` prints one JSON object on stdout. `final_output` is the value itself for json
artifacts; for parquet and pickle it is a pointer:

```json
{"final_output": {"_barca_artifact": {"path": ".barca/artifacts/...parquet", "format": "parquet", "size_bytes": 862}}}
```

To see the data, either call the Python API, which deserializes for you
(`import barca; barca.get("orders", "pipeline.py")` returns a pandas `DataFrame` for parquet),
or read the `path` yourself (`duckdb.sql("select * from '<path>'")`).

## Gotchas

- DuckDB `DECIMAL` values (including literals like `9.5` and the result of `sum()` over them)
  arrive in Python as `decimal.Decimal`, which is not JSON-serializable. A dict containing one
  is stored as pickle, not JSON. Cast to `double` in SQL or convert with `float(...)`.
- Pickle fails for objects that cannot be pickled (open connections, duckdb relations,
  generators). Return data, not handles.
- A duckdb relation is executed when the step ends, so any connection it uses must still be
  alive when the step returns.
- **DuckDB connections.** Barca loads duckdb inputs with `duckdb.read_parquet`, so they live on
  duckdb's process-wide *default* connection, the same one the module-level `duckdb.sql(...)`
  and `duckdb.read_parquet(...)` use inside your step. Stay on those and inputs, joins and SQL
  over input names all work together. Relations from different connections cannot be
  combined: if your step creates its own `duckdb.connect()`, mixing its relations with an
  input fails with `Cannot combine LEFT and RIGHT relations of different connections!` (or
  `... not suitable for replacement scan`), and `con.register("x", input)` fails the same way.
  If you need your own connection, copy the input across with
  `con.register("x", input.arrow())` (the data is loaded into memory). A relation does not
  expose its file path, so you cannot re-read the parquet file yourself.
- Default-connection state (temp views, `SET` options, attached databases) can outlive a
  step, because a worker process runs several steps. Prefer stateless SQL, and clean up
  anything you create (`drop view if exists ...`).
- Everything is materialized between steps. To cache several results from one computation,
  define several assets or return the one you want cached.

See also: `barca docs cache`, `barca docs examples/duckdb`.
