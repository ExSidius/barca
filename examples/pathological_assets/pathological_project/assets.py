"""Pathological asset examples for AST dependency tracing validation.

These assets exercise edge cases in dependency tracing:
- Same-file helper functions
- Global constants
- Transitive dependency chains
- Cross-file imports
- @unsafe decorator (untraceable patterns)
- Impure functions (should emit warnings)
- Closures
"""

from barca import asset, unsafe
from pathological_project.helpers import normalize

# ---------------------------------------------------------------------------
# 1. Pure asset with same-file helper
# ---------------------------------------------------------------------------


def compute_score(x):
    return x * 0.85


@asset()
def scored_data() -> dict:
    """Depends on compute_score — changing it should invalidate this asset."""
    return {"score": compute_score(100)}


# ---------------------------------------------------------------------------
# 2. Asset using global constant
# ---------------------------------------------------------------------------

THRESHOLD = 0.5


@asset()
def threshold_check() -> dict:
    """Depends on THRESHOLD — changing it should invalidate this asset."""
    return {"above": 75 > THRESHOLD, "threshold": THRESHOLD}


# ---------------------------------------------------------------------------
# 3. Transitive chain: step_a -> step_b -> chained_result
# ---------------------------------------------------------------------------


def step_a():
    return 1


def step_b():
    return step_a() + 1


@asset()
def chained_result() -> dict:
    """Depends on step_b, which depends on step_a.
    Changing step_a should invalidate this asset transitively."""
    return {"result": step_b() + 1}


# ---------------------------------------------------------------------------
# 4. Cross-file dependency
# ---------------------------------------------------------------------------


@asset()
def normalized_data() -> dict:
    """Depends on helpers.normalize — cross-file dependency tracing."""
    return {"value": normalize("  Hello World  ")}


# ---------------------------------------------------------------------------
# 5. @unsafe: acknowledging untraceable behavior
# ---------------------------------------------------------------------------


@unsafe
def load_dynamic_config():
    """Uses eval() — fundamentally untraceable. @unsafe silences warnings."""
    return eval("{'key': 'value'}")


@asset()
def config_based() -> dict:
    """A pure asset. Note: load_dynamic_config is @unsafe but this asset
    doesn't call it directly — it's a standalone example."""
    return {"config_key": "static_value"}


# ---------------------------------------------------------------------------
# 6. Impure function (should emit purity warning at reindex)
# ---------------------------------------------------------------------------


@asset()
def impure_asset() -> dict:
    """Uses open() — tracer should warn about external data dependency.
    Falls back to conservative hashing."""
    import json
    import os

    data_path = os.path.join(os.path.dirname(__file__), "sample_data.json")
    if os.path.exists(data_path):
        with open(data_path) as f:
            return json.load(f)
    return {"fallback": True}


# ---------------------------------------------------------------------------
# 7. Closure dependency
# ---------------------------------------------------------------------------


def make_transformer(factor):
    def transform(x):
        return x * factor

    return transform


double = make_transformer(2)


@asset()
def closure_asset() -> dict:
    """Depends on the closure `double` — should trace through make_transformer."""
    return {"doubled": double(21)}
