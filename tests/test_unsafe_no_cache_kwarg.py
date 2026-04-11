"""Tests for @unsafe after the cache= parameter removal.

Design decision D10: @unsafe no longer accepts a cache= kwarg. Unsafe assets
cache by run_hash identically to pure assets. The only thing @unsafe changes
is: dependency cone hashing is skipped (so only the function's own source is
hashed) and purity warnings are silenced.
"""

from __future__ import annotations

import sys
import textwrap

import pytest

from barca._store import MetadataStore


def _cleanup(prefix: str) -> None:
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]
    from barca._trace import clear_caches

    clear_caches()


def test_unsafe_rejects_cache_kwarg():
    """@unsafe(cache=True) must raise TypeError — parameter is removed."""
    from barca import unsafe

    with pytest.raises(TypeError):

        @unsafe(cache=True)
        def bad_fn():
            return 1


def test_unsafe_rejects_cache_false_kwarg():
    """@unsafe(cache=False) is also rejected — the kwarg does not exist at all."""
    from barca import unsafe

    with pytest.raises(TypeError):

        @unsafe(cache=False)
        def bad_fn():
            return 1


def test_unsafe_bare_decoration_works():
    """@unsafe without parens still works."""
    from barca import unsafe

    @unsafe
    def ok():
        return 1

    assert getattr(ok, "__unsafe__", False) is True


def test_unsafe_parens_no_kwargs_works():
    """@unsafe() with empty parens still works."""
    from barca import unsafe

    @unsafe()
    def ok():
        return 1

    assert getattr(ok, "__unsafe__", False) is True


def test_is_unsafe_cacheable_removed():
    """The is_unsafe_cacheable helper should be gone from the public surface."""
    from barca import _unsafe

    assert not hasattr(_unsafe, "is_unsafe_cacheable")


def test_unsafe_asset_caches_like_pure(tmp_path):
    """An unsafe asset with unchanged inputs hits the cache on second run_pass."""
    project_dir = tmp_path / "unsafeproject"
    project_dir.mkdir()

    mod_dir = project_dir / "unsafemod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, unsafe, Always

        @asset(freshness=Always())
        @unsafe
        def impure_but_stable():
            return {"value": 42}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["unsafemod.assets"]
        """)
    )

    _cleanup("unsafemod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        r1 = run_pass(store, project_dir)
        assert r1.executed_assets >= 1

        r2 = run_pass(store, project_dir)
        # Second pass: cache hit, nothing executes.
        assert r2.executed_assets == 0
        assert r2.fresh >= 1
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("unsafemod")
