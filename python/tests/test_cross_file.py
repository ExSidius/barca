"""Tests for cross-file cone analysis: subdirectory imports and __init__.py re-exports."""

import shutil
import textwrap
from pathlib import Path

import pytest

import barca


@pytest.fixture(autouse=True)
def clean_barca_dir():
    barca_dir = Path(".barca")
    if barca_dir.exists():
        shutil.rmtree(barca_dir)
    yield
    if barca_dir.exists():
        shutil.rmtree(barca_dir)


def write_file(base, relpath, code):
    p = base / relpath
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(textwrap.dedent(code))
    return str(p)


class TestSubdirectoryImports:
    """from subpkg.utils import helper should be tracked for cache invalidation."""

    def test_subdir_import_detected(self, tmp_path):
        """Changing a helper in a subdirectory should invalidate the asset."""
        write_file(
            tmp_path,
            "utils/math.py",
            """
            def double(x):
                return x * 2
        """,
        )
        asset_file = write_file(
            tmp_path,
            "pipeline.py",
            """
            from barca import asset
            from utils.math import double

            @asset()
            def result():
                return {"value": double(21)}
        """,
        )

        # First run
        r1 = barca.api._exec(["get", "result", asset_file])
        assert r1["steps_executed"] == 1

        # Second run — cached
        r2 = barca.api._exec(["get", "result", asset_file])
        assert r2["steps_executed"] == 0

        # Change the helper in the subdirectory
        write_file(
            tmp_path,
            "utils/math.py",
            """
            def double(x):
                return x * 99
        """,
        )

        # Third run — should detect the change and re-execute
        r3 = barca.api._exec(["get", "result", asset_file])
        assert r3["steps_executed"] == 1

    def test_nested_subdir_import(self, tmp_path):
        """from pkg.sub.module import func should work."""
        write_file(
            tmp_path,
            "pkg/sub/helpers.py",
            """
            def compute(x):
                return x + 1
        """,
        )
        asset_file = write_file(
            tmp_path,
            "pipeline.py",
            """
            from barca import asset
            from pkg.sub.helpers import compute

            @asset()
            def result():
                return {"value": compute(10)}
        """,
        )

        r1 = barca.api._exec(["get", "result", asset_file])
        assert r1["steps_executed"] == 1

        r2 = barca.api._exec(["get", "result", asset_file])
        assert r2["steps_executed"] == 0

        # Change nested helper
        write_file(
            tmp_path,
            "pkg/sub/helpers.py",
            """
            def compute(x):
                return x + 999
        """,
        )

        r3 = barca.api._exec(["get", "result", asset_file])
        assert r3["steps_executed"] == 1


class TestRelativeImports:
    """from .submodule import func (relative imports in __init__.py)."""

    def test_relative_import_in_init(self, tmp_path):
        """__init__.py using `from .core import transform` should track core.py changes."""
        write_file(
            tmp_path,
            "mylib/__init__.py",
            """
            from .core import transform
        """,
        )
        write_file(
            tmp_path,
            "mylib/core.py",
            """
            def transform(x):
                return x * 2
        """,
        )
        asset_file = write_file(
            tmp_path,
            "pipeline.py",
            """
            from barca import asset
            from mylib import transform

            @asset()
            def result():
                return {"value": transform(21)}
        """,
        )

        r1 = barca.api._exec(["get", "result", asset_file])
        assert r1["steps_executed"] == 1

        r2 = barca.api._exec(["get", "result", asset_file])
        assert r2["steps_executed"] == 0

        # Change the implementation behind the relative import
        write_file(
            tmp_path,
            "mylib/core.py",
            """
            def transform(x):
                return x * 99
        """,
        )

        r3 = barca.api._exec(["get", "result", asset_file])
        assert r3["steps_executed"] == 1

    def test_relative_import_in_regular_submodule(self, tmp_path):
        """from .sibling import x inside a regular (non-__init__) submodule.

        Regression test for issue #63: `mylib/core.py` is a *regular* submodule,
        so `from .util import transform` must resolve to `mylib.util`, not
        `mylib.core.util`.
        """
        write_file(
            tmp_path,
            "mylib/core.py",
            """
            from .util import transform
        """,
        )
        write_file(
            tmp_path,
            "mylib/util.py",
            """
            def transform(x):
                return x * 2
        """,
        )
        asset_file = write_file(
            tmp_path,
            "pipeline.py",
            """
            from barca import asset
            from mylib.core import transform

            @asset()
            def result():
                return {"value": transform(21)}
        """,
        )

        r1 = barca.api._exec(["get", "result", asset_file])
        assert r1["steps_executed"] == 1

        r2 = barca.api._exec(["get", "result", asset_file])
        assert r2["steps_executed"] == 0

        # Change the implementation behind the relative import
        write_file(
            tmp_path,
            "mylib/util.py",
            """
            def transform(x):
                return x * 99
        """,
        )

        r3 = barca.api._exec(["get", "result", asset_file])
        assert r3["steps_executed"] == 1

    def test_relative_import_level_2(self, tmp_path):
        """from ..sibling import x (level=2) climbs to the grandparent package.

        Regression test for issue #63: `import.level` must not be treated as a
        boolean — `from ..util import x` (level=2) inside `pkg/sub/core.py`
        must resolve to `pkg.util`, not `pkg.sub.util` (the level=1 answer).
        """
        write_file(
            tmp_path,
            "pkg/sub/core.py",
            """
            from ..util import transform
        """,
        )
        write_file(
            tmp_path,
            "pkg/util.py",
            """
            def transform(x):
                return x * 2
        """,
        )
        # A decoy at the level=1 (wrong) location — must be ignored.
        write_file(
            tmp_path,
            "pkg/sub/util.py",
            """
            def transform(x):
                return -1
        """,
        )
        asset_file = write_file(
            tmp_path,
            "pipeline.py",
            """
            from barca import asset
            from pkg.sub.core import transform

            @asset()
            def result():
                return {"value": transform(21)}
        """,
        )

        r1 = barca.api._exec(["get", "result", asset_file])
        assert r1["steps_executed"] == 1

        r2 = barca.api._exec(["get", "result", asset_file])
        assert r2["steps_executed"] == 0

        # Change the implementation behind the level=2 relative import
        write_file(
            tmp_path,
            "pkg/util.py",
            """
            def transform(x):
                return x * 99
        """,
        )

        r3 = barca.api._exec(["get", "result", asset_file])
        assert r3["steps_executed"] == 1


class TestInitReExports:
    """from mypackage import thing where thing is in __init__.py."""

    def test_init_reexport_detected(self, tmp_path):
        """Changing a function re-exported through __init__.py should invalidate."""
        write_file(
            tmp_path,
            "mylib/__init__.py",
            """
            from mylib.core import transform
        """,
        )
        write_file(
            tmp_path,
            "mylib/core.py",
            """
            def transform(x):
                return x * 2
        """,
        )
        asset_file = write_file(
            tmp_path,
            "pipeline.py",
            """
            from barca import asset
            from mylib import transform

            @asset()
            def result():
                return {"value": transform(21)}
        """,
        )

        r1 = barca.api._exec(["get", "result", asset_file])
        assert r1["steps_executed"] == 1

        r2 = barca.api._exec(["get", "result", asset_file])
        assert r2["steps_executed"] == 0

        # Change the underlying implementation
        write_file(
            tmp_path,
            "mylib/core.py",
            """
            def transform(x):
                return x * 99
        """,
        )

        r3 = barca.api._exec(["get", "result", asset_file])
        assert r3["steps_executed"] == 1
