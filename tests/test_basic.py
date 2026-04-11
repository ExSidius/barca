"""W1: Basic asset lifecycle — reindex, refresh, cache, reset."""

from barca._engine import refresh, reindex, reset
from barca._store import MetadataStore


def test_reindex_discovers_assets(tmp_project):
    store = MetadataStore(str(tmp_project / ".barca" / "metadata.db"))
    reindex(store, tmp_project)
    assets = store.list_assets()
    names = [a.logical_name for a in assets]
    assert any("hello" in n for n in names)
    assert any("greeting" in n for n in names)


def test_refresh_produces_artifact(tmp_project):
    store = MetadataStore(str(tmp_project / ".barca" / "metadata.db"))
    reindex(store, tmp_project)
    assets = store.list_assets()
    hello_id = next(a.asset_id for a in assets if "hello" in a.logical_name)

    detail = refresh(store, tmp_project, hello_id)
    mat = detail.latest_materialization
    assert mat is not None
    assert mat.status == "success"
    assert mat.artifact_path is not None

    # Check artifact exists on disk
    artifact = tmp_project / mat.artifact_path
    assert artifact.exists()
    import json

    value = json.loads(artifact.read_text())
    assert value == {"message": "hello"}


def test_second_refresh_is_cached(tmp_project):
    store = MetadataStore(str(tmp_project / ".barca" / "metadata.db"))
    reindex(store, tmp_project)
    assets = store.list_assets()
    hello_id = next(a.asset_id for a in assets if "hello" in a.logical_name)

    detail1 = refresh(store, tmp_project, hello_id)
    mat1 = detail1.latest_materialization

    detail2 = refresh(store, tmp_project, hello_id)
    mat2 = detail2.latest_materialization

    # Should reuse the same materialization (cache hit)
    assert mat1.materialization_id == mat2.materialization_id


def test_reset_clears_state(tmp_project):
    store = MetadataStore(str(tmp_project / ".barca" / "metadata.db"))
    reindex(store, tmp_project)
    assets = store.list_assets()
    hello_id = next(a.asset_id for a in assets if "hello" in a.logical_name)
    refresh(store, tmp_project, hello_id)

    assert (tmp_project / ".barca").exists()
    assert (tmp_project / ".barcafiles").exists()

    output = reset(tmp_project)
    assert "removed .barca/" in output
    assert "removed .barcafiles/" in output
    assert not (tmp_project / ".barca").exists()
    assert not (tmp_project / ".barcafiles").exists()


def test_idempotent_reindex(tmp_project):
    store = MetadataStore(str(tmp_project / ".barca" / "metadata.db"))
    reindex(store, tmp_project)
    assets1 = store.list_assets()
    reindex(store, tmp_project)
    assets2 = store.list_assets()
    assert len(assets1) == len(assets2)
    assert [a.definition_hash for a in assets1] == [a.definition_hash for a in assets2]
