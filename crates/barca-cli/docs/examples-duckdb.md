# Example: DuckDB relations as a DAG

Every step returns a DuckDB relation; barca writes each one as parquet and hands the next step
a relation over that file. Requires `pip install duckdb`.

```
orders ──┐
         ├─► orders_enriched ─► revenue_by_region ─► top_region
customers┘
```

```python
import duckdb
from barca import asset


@asset()
def orders() -> duckdb.DuckDBPyRelation:
    return duckdb.sql("""
        select * from (values
            (1, 10, 120.0), (2, 11, 80.0), (3, 10, 45.5), (4, 12, 300.0), (5, 11, 19.99)
        ) t(order_id, customer_id, amount)
    """)


@asset()
def customers() -> duckdb.DuckDBPyRelation:
    return duckdb.sql("""
        select * from (values
            (10, 'Ada', 'EMEA'), (11, 'Grace', 'AMER'), (12, 'Linus', 'EMEA')
        ) t(customer_id, name, region)
    """)


@asset(inputs={"orders": orders, "customers": customers})
def orders_enriched(
    orders: duckdb.DuckDBPyRelation, customers: duckdb.DuckDBPyRelation
) -> duckdb.DuckDBPyRelation:
    return duckdb.sql("""
        select o.order_id, o.amount, c.name, c.region
        from orders o join customers c using (customer_id)
    """)


@asset(inputs={"enriched": orders_enriched})
def revenue_by_region(enriched: duckdb.DuckDBPyRelation) -> duckdb.DuckDBPyRelation:
    return duckdb.sql("""
        select region, count(*) as orders, cast(round(sum(amount), 2) as double) as revenue
        from enriched group by region order by revenue desc
    """)


@asset(inputs={"rev": revenue_by_region})
def top_region(rev: duckdb.DuckDBPyRelation) -> dict:
    region, orders, revenue = rev.limit(1).fetchone()
    return {"region": region, "orders": orders, "revenue": revenue}
```

```bash
barca list pipeline.py
barca get top_region pipeline.py
```

What to notice:

- The first four assets are written as `.parquet`; `top_region` returns a dict and is `.json`.
- Each parameter name is also the table name inside the SQL: DuckDB resolves `orders`,
  `customers`, `enriched` and `rev` from the Python variables of the same name. This works
  because inputs and `duckdb.sql(...)` share duckdb's default connection; a step that opens
  its own `duckdb.connect()` cannot mix its relations with inputs (see `barca docs types`).
- `revenue` is cast to `double`: DuckDB decimals come back as Python `Decimal`, which is not
  JSON-serializable, so a dict holding one would be pickled instead of stored as JSON.
- The final JSON has `"final_output": {"region": "EMEA", ...}`. To see a parquet step instead,
  `barca get revenue_by_region pipeline.py` returns an `_barca_artifact` pointer with the file
  path; `barca.get("revenue_by_region", "pipeline.py")` returns a DataFrame.

See also: `barca docs types`.
