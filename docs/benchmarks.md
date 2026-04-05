# Pipeline Benchmarks

Aspirational benchmark suite for stress-testing barca's orchestration. Ordered by complexity.

## Current benchmarks

### `iris_pipeline` (implemented)
- **Topology**: 4-node linear chain
- **Pattern**: `raw_data → train_test_split → trained_model → evaluation`
- **Tests**: Sequential deps, caching, far-off dependency invalidation
- **Compute**: scikit-learn (load, split, fit, evaluate)

### `spaceflights` — Full diamond, adapted from Kedro (implemented)
```
raw_shuttles ──→ prep_shuttles ──┐
raw_companies ─→ prep_companies ─├→ master_table → split → train → evaluate
raw_reviews ───→ prep_reviews ──┘
```
- **Depth**: 6 levels, width 3
- **Pattern**: 3 independent sources, each preprocessed, merged into master table, then linear ML chain
- **Tests**: Mixed fan-out/fan-in, deep chain after merge, all caching behaviors
- **Compute**: pandas joins + sklearn RandomForestRegressor
- **Source**: Adapted from [kedro-org/kedro-starters/spaceflights](https://github.com/kedro-org/kedro-starters)
- **Benchmarked across**: Barca, Dagster, Prefect (see `benchmarks/`)

## Planned benchmarks

### `linear_chain` — Deep sequential pipeline
```
ingest → clean → feature_eng → split → train → evaluate → report
```
- **Depth**: 7 levels
- **Pattern**: Pure linear chain, no fan-out
- **Tests**: Ordering correctness at depth, re-queue wait behavior
- **Compute**: pandas transforms + sklearn
- **Source data**: Kaggle House Prices or Titanic

### `diamond` — Fan-out / fan-in
```
raw_data → numerical_features ──┐
                                 ├→ combined_features → model → evaluation
raw_data → categorical_features ┘
```
- **Depth**: 4 levels
- **Pattern**: Classic diamond (A → B, A → C, B+C → D)
- **Tests**: Parallel fan-out, correct merge, upstream-runs-once guarantee
- **Compute**: sklearn ColumnTransformer equivalent in pandas

### `ensemble` — Wide fan-out + merge
```
features → model_xgboost ───┐
features → model_rf ─────────├→ ensemble_blend → evaluation
features → model_lr ─────────┘
```
- **Depth**: 4 levels, width 3
- **Pattern**: One asset feeds 3 independent models, results merged
- **Tests**: Parallel model training, correct blend
- **Compute**: sklearn RandomForest, LogisticRegression + simple average blend

### `partitioned_timeseries` — Mixed partitioned + non-partitioned
```
raw_events (daily partitioned)
  → daily_aggregates (daily partitioned)
    → monthly_rollup (non-partitioned)
      → forecast_model (non-partitioned)
```
- **Depth**: 4 levels
- **Pattern**: Partitioned assets feeding non-partitioned downstream
- **Tests**: Partition fan-out, cross-partition merge, mixed partition types
- **Compute**: pandas groupby + simple forecasting

### `full_ml_pipeline` — All patterns combined
```
raw_train ─→ clean ─→ num_features ──────────────────┐
                   └→ cat_features → encoded ─────────┤
                                                       ├→ combined → split → xgb ────┐
raw_config ─→ hyperparams ────────────────────────────┘                → rf ─────┼→ blend → eval → report
                                                                        → lr ─────┘
raw_train (partitioned by fold) → cv_scores (partitioned) → cv_summary ─┘
```
- **Depth**: 8+ levels, width 5+
- **Pattern**: Diamond + fan-out + partitioned + deep chain
- **Tests**: Everything — the "does it all work together" benchmark
- **Compute**: Full sklearn pipeline

## Reference sources

| Source | Patterns | Link |
|--------|----------|------|
| Kedro spaceflights | Diamond + deep chain | `kedro-org/kedro-starters` |
| Dagster project_fully_featured | Partitioned + IO managers | `dagster-io/dagster/examples/` |
| Dagster Hacker News | Fan-out + real API data | `dagster-io/dagster/examples/` |
| MLflow multistep_workflow | Explicit multi-step DAG | `mlflow/mlflow/examples/` |
| dbt jaffle_shop | Staging → intermediate → marts | `dbt-labs/jaffle_shop` |
| TPC-DI specification | Industry-standard ETL benchmark | `tpc.org/tpcdi` |
