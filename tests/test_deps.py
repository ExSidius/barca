"""W2: Asset dependencies — upstream resolution, artifact passing."""

import importlib
import json
import sys

from barca._engine import reindex, refresh
from barca._store import MetadataStore
from barca._trace import clear_caches


def test_reindex_stores_input_linkage(dep_project):
    store = MetadataStore(str(dep_project / ".barca" / "metadata.db"))
    assets = reindex(store, dep_project)

    upper_id = next(a.asset_id for a in assets if "uppercased" in a.logical_name)
    detail = store.asset_detail(upper_id)
    inputs = store.get_asset_inputs(detail.asset.definition_id)

    assert len(inputs) == 1
    assert inputs[0].parameter_name == "fruit"
    assert "fruit" in inputs[0].upstream_asset_ref


def test_refresh_downstream_triggers_upstream(dep_project):
    store = MetadataStore(str(dep_project / ".barca" / "metadata.db"))
    assets = reindex(store, dep_project)

    upper_id = next(a.asset_id for a in assets if "uppercased" in a.logical_name)
    detail = refresh(store, dep_project, upper_id)

    # Both assets should be materialized
    for a in assets:
        mat = store.latest_successful_materialization(a.asset_id)
        assert mat is not None, f"asset {a.logical_name} should be materialized"


def test_downstream_receives_upstream_artifact(dep_project):
    store = MetadataStore(str(dep_project / ".barca" / "metadata.db"))
    assets = reindex(store, dep_project)

    upper_id = next(a.asset_id for a in assets if "uppercased" in a.logical_name)
    detail = refresh(store, dep_project, upper_id)

    mat = detail.latest_materialization
    assert mat is not None
    value = json.loads((dep_project / mat.artifact_path).read_text())
    assert value == "BANANA"


def test_upstream_change_invalidates_downstream_run_hash(dep_project):
    store = MetadataStore(str(dep_project / ".barca" / "metadata.db"))
    assets = reindex(store, dep_project)
    upper_id = next(a.asset_id for a in assets if "uppercased" in a.logical_name)

    detail1 = refresh(store, dep_project, upper_id)
    mat1 = detail1.latest_materialization

    # Change the upstream asset source
    assets_file = dep_project / "depmod" / "assets.py"
    src = assets_file.read_text()
    new_src = src.replace('"banana"', '"apple"')
    assets_file.write_text(new_src)

    # Reload modules
    _cleanup_and_reload("depmod")
    clear_caches()

    assets = reindex(store, dep_project)
    detail2 = refresh(store, dep_project, upper_id)
    mat2 = detail2.latest_materialization

    # The fruit asset should have a new definition hash (source changed)
    # So the uppercased asset should have a new run_hash and thus new materialization
    assert mat2.run_hash != mat1.run_hash, "run_hash should change when upstream source changes"

    # New value should be "APPLE"
    value = json.loads((dep_project / mat2.artifact_path).read_text())
    assert value == "APPLE"


def test_upstream_already_fresh_is_reused(dep_project):
    store = MetadataStore(str(dep_project / ".barca" / "metadata.db"))
    assets = reindex(store, dep_project)

    fruit_id = next(a.asset_id for a in assets if "fruit" in a.logical_name and "uppercased" not in a.logical_name)
    upper_id = next(a.asset_id for a in assets if "uppercased" in a.logical_name)

    # Refresh fruit first
    refresh(store, dep_project, fruit_id)
    fruit_mat = store.latest_successful_materialization(fruit_id)

    # Now refresh uppercased — fruit should not be re-materialized
    refresh(store, dep_project, upper_id)
    fruit_mat_after = store.latest_successful_materialization(fruit_id)

    assert fruit_mat.materialization_id == fruit_mat_after.materialization_id


def _cleanup_and_reload(prefix: str):
    """Remove all modules starting with prefix from sys.modules."""
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]
