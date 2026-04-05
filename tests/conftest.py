"""Shared fixtures for barca tests."""

from __future__ import annotations

import sys
import textwrap
from pathlib import Path

import pytest

from barca._store import MetadataStore


def _cleanup_modules(prefix: str):
    """Remove all modules starting with prefix from sys.modules."""
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]


@pytest.fixture
def store(tmp_path):
    """Fresh MetadataStore in a temp directory."""
    db_path = tmp_path / ".barca" / "metadata.db"
    return MetadataStore(str(db_path))


@pytest.fixture
def tmp_project(tmp_path):
    """Create a minimal barca project with one asset module."""
    project_dir = tmp_path / "myproject"
    project_dir.mkdir()

    mod_dir = project_dir / "mymod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(textwrap.dedent("""\
        from barca import asset

        @asset()
        def hello() -> dict:
            return {"message": "hello"}

        @asset()
        def greeting() -> str:
            return "Hello from Barca!"
    """))

    (project_dir / "barca.toml").write_text(textwrap.dedent("""\
        [project]
        modules = ["mymod.assets"]
    """))

    _cleanup_modules("mymod")
    sys.path.insert(0, str(project_dir))
    yield project_dir
    sys.path.remove(str(project_dir))
    _cleanup_modules("mymod")
    from barca._trace import clear_caches
    clear_caches()


@pytest.fixture
def dep_project(tmp_path):
    """Create a project with upstream/downstream dependency."""
    project_dir = tmp_path / "depproject"
    project_dir.mkdir()

    mod_dir = project_dir / "depmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(textwrap.dedent("""\
        from barca import asset

        @asset()
        def fruit() -> str:
            return "banana"

        @asset(inputs={"fruit": fruit})
        def uppercased(fruit: str) -> str:
            return fruit.upper()
    """))

    (project_dir / "barca.toml").write_text(textwrap.dedent("""\
        [project]
        modules = ["depmod.assets"]
    """))

    _cleanup_modules("depmod")
    sys.path.insert(0, str(project_dir))
    yield project_dir
    sys.path.remove(str(project_dir))
    _cleanup_modules("depmod")
    from barca._trace import clear_caches
    clear_caches()


@pytest.fixture
def partition_project(tmp_path):
    """Create a project with partitioned assets."""
    project_dir = tmp_path / "partproject"
    project_dir.mkdir()

    mod_dir = project_dir / "partmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(textwrap.dedent("""\
        from barca import asset, partitions

        @asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])})
        def fetch_prices(ticker: str) -> dict:
            return {"ticker": ticker, "price": len(ticker) * 100}
    """))

    (project_dir / "barca.toml").write_text(textwrap.dedent("""\
        [project]
        modules = ["partmod.assets"]
    """))

    _cleanup_modules("partmod")
    sys.path.insert(0, str(project_dir))
    yield project_dir
    sys.path.remove(str(project_dir))
    _cleanup_modules("partmod")
    from barca._trace import clear_caches
    clear_caches()
