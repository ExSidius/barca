# Iris ML Pipeline Example

A 4-asset sequential ML pipeline using scikit-learn's iris dataset.

```
raw_data → train_test_split → trained_model → evaluation
```

## Setup (from a branch checkout)

```bash
# 1. Build the Rust CLI (from repo root)
cargo build -p barca-cli

# 2. Install Python dependencies (from this directory)
cd examples/iris_pipeline
uv sync

# 3. Run the pipeline
cargo run -p barca-cli -- reindex
cargo run -p barca-cli -- assets refresh 1   # refresh evaluation (cascades all deps)
```

Or use the built binary directly:

```bash
../../target/debug/barca reindex
../../target/debug/barca assets refresh 1
```

## What it does

| Asset | Description |
|-------|-------------|
| `raw_data` | Loads the iris dataset (150 samples, 4 features, 3 classes) |
| `train_test_split` | 80/20 train/test split with `random_state=42` |
| `trained_model` | Random forest classifier (50 estimators) |
| `evaluation` | Accuracy, feature importances, classification report |

## Expected output

```
$ cargo run -p barca-cli -- assets refresh 1
Waiting for materialization of asset #1 (4 jobs)...
Asset #1
  Name:     iris_project/assets.py:evaluation
  Function: evaluation
  Last job: #4 (success)
```

The evaluation artifact (`.barcafiles/.../value.json`) contains:

```json
{
  "test_accuracy": 1.0,
  "train_accuracy": 1.0,
  "feature_importances": {
    "petal length (cm)": 0.4458,
    "petal width (cm)": 0.414,
    "sepal length (cm)": 0.1116,
    "sepal width (cm)": 0.0286
  },
  "classification_report": { ... }
}
```

Second run is instant (cached):

```
$ cargo run -p barca-cli -- assets refresh 1
Asset #1
  Last job: #5 (success)    # reuses cached materialization
```
