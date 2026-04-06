"""W7: Notebook workflow — load_inputs, materialize, read_asset, list_versions.

Core scenario: one expensive dataset asset, two downstream models that consume
it.  The notebook helpers let you materialize the dataset once and iterate on
models without recomputation.
"""

import sys
import textwrap

import pytest

from barca._engine import reindex, trigger_sensor
from barca._notebook import list_versions, load_inputs, materialize, read_asset
from barca._store import MetadataStore


def _cleanup(prefix):
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]


# ------------------------------------------------------------------
# Fixture: ML pipeline with shared expensive upstream
# ------------------------------------------------------------------


@pytest.fixture
def ml_project(tmp_path):
    project_dir = tmp_path / "mlproject"
    project_dir.mkdir()

    mod_dir = project_dir / "mlmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import asset

        @asset()
        def load_dataset() -> dict:
            \"\"\"Expensive: simulates loading a large dataset.\"\"\"
            return {"features": [[1,2],[3,4],[5,6]], "labels": [0,1,0]}

        @asset(inputs={"data": load_dataset})
        def linear_model(data: dict) -> dict:
            n = len(data["labels"])
            return {"model": "linear", "accuracy": 0.75, "n_samples": n}

        @asset(inputs={"data": load_dataset})
        def tree_model(data: dict) -> dict:
            n = len(data["labels"])
            return {"model": "tree", "accuracy": 0.82, "n_samples": n}
    """)
    )

    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["mlmod.pipeline"]
    """)
    )

    _cleanup("mlmod")
    sys.path.insert(0, str(project_dir))
    yield project_dir
    sys.path.remove(str(project_dir))
    _cleanup("mlmod")
    from barca._trace import clear_caches

    clear_caches()


# ------------------------------------------------------------------
# Fixture: sensor pipeline (reused from test_sensor pattern)
# ------------------------------------------------------------------


@pytest.fixture
def sensor_project(tmp_path):
    project_dir = tmp_path / "sensorproject"
    project_dir.mkdir()

    mod_dir = project_dir / "smod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "pipeline.py").write_text(
        textwrap.dedent("""\
        from barca import sensor, asset, effect

        @sensor(schedule="always")
        def check_file():
            return (True, {"path": "/tmp/data.csv", "rows": 42})

        @asset(inputs={"data": check_file}, schedule="always")
        def process(data):
            return {"processed": data["rows"] * 2}

        @effect(inputs={"result": process}, schedule="always")
        def notify(result):
            pass
    """)
    )

    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["smod.pipeline"]
    """)
    )

    _cleanup("smod")
    sys.path.insert(0, str(project_dir))
    yield project_dir
    sys.path.remove(str(project_dir))
    _cleanup("smod")
    from barca._trace import clear_caches

    clear_caches()


# ------------------------------------------------------------------
# Tests: notebook-style iteration with shared expensive input
# ------------------------------------------------------------------


def test_materialize_dataset_once_reuse_for_two_models(ml_project):
    """Materialize the dataset once, then iterate on both models without
    recomputing the upstream."""
    import mlmod.pipeline as m

    # 1. Materialize the expensive dataset once
    dataset = materialize(m.load_dataset)
    assert dataset == {"features": [[1, 2], [3, 4], [5, 6]], "labels": [0, 1, 0]}

    # 2. Load inputs for the linear model — dataset is reused, not recomputed
    kwargs = load_inputs(m.linear_model)
    assert "data" in kwargs
    assert kwargs["data"] == dataset

    # 3. Call model as plain Python
    result_a = m.linear_model(**kwargs)
    assert result_a == {"model": "linear", "accuracy": 0.75, "n_samples": 3}

    # 4. Same dataset for tree model — zero recomputation
    kwargs = load_inputs(m.tree_model)
    assert kwargs["data"] == dataset

    result_b = m.tree_model(**kwargs)
    assert result_b == {"model": "tree", "accuracy": 0.82, "n_samples": 3}

    # 5. Verify dataset was only materialized once
    store = MetadataStore(str(ml_project / ".barca" / "metadata.db"))
    assets = store.list_assets()
    ds_id = next(a.asset_id for a in assets if a.function_name == "load_dataset")
    mats = store.list_materializations(ds_id)
    success_mats = [m for m in mats if m.status == "success"]
    assert len(success_mats) == 1


def test_read_asset_returns_cached_value(ml_project):
    """read_asset returns the materialized value without re-executing."""
    import mlmod.pipeline as m

    materialize(m.load_dataset)
    value = read_asset(m.load_dataset)
    assert value == {"features": [[1, 2], [3, 4], [5, 6]], "labels": [0, 1, 0]}


def test_materialize_cascades_upstream(ml_project):
    """Materializing a downstream asset auto-materializes its upstream."""
    import mlmod.pipeline as m

    result = materialize(m.linear_model)
    assert result == {"model": "linear", "accuracy": 0.75, "n_samples": 3}

    # Upstream dataset should also be materialized
    value = read_asset(m.load_dataset)
    assert value["labels"] == [0, 1, 0]


def test_load_inputs_no_inputs_returns_empty(ml_project):
    """An asset with no declared inputs returns an empty dict."""
    import mlmod.pipeline as m

    kwargs = load_inputs(m.load_dataset)
    assert kwargs == {}


def test_round_trip_call(ml_project):
    """asset_fn(**load_inputs(asset_fn)) works as ordinary Python."""
    import mlmod.pipeline as m

    materialize(m.load_dataset)
    result = m.tree_model(**load_inputs(m.tree_model))
    assert result["model"] == "tree"
    assert result["n_samples"] == 3


# ------------------------------------------------------------------
# Tests: sensor upstream
# ------------------------------------------------------------------


def test_load_inputs_sensor_upstream(sensor_project):
    """load_inputs resolves sensor observations for downstream assets."""
    import smod.pipeline as m

    # Trigger the sensor so it has an observation
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    assets = reindex(store, sensor_project)
    sensor_id = next(a.asset_id for a in assets if a.function_name == "check_file")
    trigger_sensor(store, sensor_project, sensor_id)

    kwargs = load_inputs(m.process)
    assert "data" in kwargs
    assert kwargs["data"] == {"path": "/tmp/data.csv", "rows": 42}


def test_read_asset_sensor(sensor_project):
    """read_asset returns the latest sensor observation output."""
    import smod.pipeline as m

    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    assets = reindex(store, sensor_project)
    sensor_id = next(a.asset_id for a in assets if a.function_name == "check_file")
    trigger_sensor(store, sensor_project, sensor_id)

    value = read_asset(m.check_file)
    assert value == {"path": "/tmp/data.csv", "rows": 42}


def test_load_inputs_effect(sensor_project):
    """load_inputs works for effects — they have inputs too."""
    import smod.pipeline as m

    from barca._reconciler import reconcile as _reconcile

    # Reconcile runs the full pipeline: sensor → asset → effect
    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    _reconcile(store, sensor_project)

    kwargs = load_inputs(m.notify)
    assert "result" in kwargs
    assert kwargs["result"]["processed"] == 84


# ------------------------------------------------------------------
# Tests: error cases
# ------------------------------------------------------------------


def test_read_asset_not_materialized_raises(ml_project):
    """read_asset raises ValueError when no materialization exists."""
    import mlmod.pipeline as m

    with pytest.raises(ValueError, match="no successful materialization"):
        read_asset(m.load_dataset)


def test_read_asset_effect_raises(sensor_project):
    """read_asset raises ValueError for effects."""
    import smod.pipeline as m

    with pytest.raises(ValueError, match="Cannot read an effect"):
        read_asset(m.notify)


def test_materialize_sensor_raises(sensor_project):
    """materialize raises ValueError for sensors."""
    import smod.pipeline as m

    with pytest.raises(ValueError, match="Cannot materialize a sensor"):
        materialize(m.check_file)


def test_load_inputs_upstream_not_materialized_raises(ml_project):
    """load_inputs raises ValueError when upstream has no data."""
    import mlmod.pipeline as m

    with pytest.raises(ValueError, match="no successful materialization"):
        load_inputs(m.linear_model)


# ------------------------------------------------------------------
# Tests: list_versions
# ------------------------------------------------------------------


def test_list_versions_asset(ml_project):
    """list_versions returns materialization history."""
    import mlmod.pipeline as m

    materialize(m.load_dataset)
    versions = list_versions(m.load_dataset)
    assert len(versions) >= 1
    assert versions[0]["status"] == "success"


def test_list_versions_sensor(sensor_project):
    """list_versions returns observations in reverse chronological order."""
    import smod.pipeline as m

    store = MetadataStore(str(sensor_project / ".barca" / "metadata.db"))
    assets = reindex(store, sensor_project)
    sensor_id = next(a.asset_id for a in assets if a.function_name == "check_file")
    trigger_sensor(store, sensor_project, sensor_id)
    trigger_sensor(store, sensor_project, sensor_id)

    versions = list_versions(m.check_file)
    assert len(versions) >= 2
    assert versions[0]["created_at"] >= versions[1]["created_at"]


def test_list_versions_empty(ml_project):
    """list_versions returns empty list when no history exists."""
    import mlmod.pipeline as m

    versions = list_versions(m.load_dataset)
    assert versions == []
