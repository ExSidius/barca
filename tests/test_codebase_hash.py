"""W5: Codebase hash and dependency cone hash tests."""

import importlib
import sys
import textwrap

from barca._engine import reindex
from barca._hashing import compute_codebase_hash
from barca._store import MetadataStore
from barca._trace import clear_caches


def _cleanup_modules(prefix: str):
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]


def test_helper_change_invalidates_hash(tmp_project):
    """Changing a helper function changes the dependency cone hash of assets that use it."""
    mod_dir = tmp_project / "mymod"
    (mod_dir / "helpers.py").write_text(textwrap.dedent("""\
        def compute(x):
            return x * 2
    """))
    (mod_dir / "assets.py").write_text(textwrap.dedent("""\
        from barca import asset
        from mymod.helpers import compute

        @asset()
        def computed() -> dict:
            return {"result": compute(21)}
    """))

    _cleanup_modules("mymod")
    clear_caches()

    store = MetadataStore(str(tmp_project / ".barca" / "metadata.db"))
    assets1 = reindex(store, tmp_project)
    hash1 = {a.logical_name: a.definition_hash for a in assets1}

    # Change the helper
    (mod_dir / "helpers.py").write_text(textwrap.dedent("""\
        def compute(x):
            return x * 3
    """))

    _cleanup_modules("mymod")
    clear_caches()

    assets2 = reindex(store, tmp_project)
    hash2 = {a.logical_name: a.definition_hash for a in assets2}

    computed_name = next(k for k in hash1 if "computed" in k)
    assert hash1[computed_name] != hash2[computed_name], "definition hash should change when helper changes"


def test_no_change_stable(tmp_project):
    """Reindexing without changes produces stable hashes."""
    store = MetadataStore(str(tmp_project / ".barca" / "metadata.db"))
    assets1 = reindex(store, tmp_project)
    assets2 = reindex(store, tmp_project)

    for a1, a2 in zip(assets1, assets2):
        assert a1.definition_hash == a2.definition_hash


def test_codebase_hash_changes_with_new_file(tmp_project):
    """Adding a .py file changes the codebase hash."""
    hash1 = compute_codebase_hash(tmp_project)

    (tmp_project / "mymod" / "extra.py").write_text("x = 1\n")
    hash2 = compute_codebase_hash(tmp_project)

    assert hash1 != hash2
