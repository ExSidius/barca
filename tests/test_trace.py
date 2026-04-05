"""AST dependency tracing tests."""

import importlib
import sys
import textwrap
from pathlib import Path

import pytest

from barca._trace import (
    analyze_purity,
    clear_caches,
    compute_dependency_hash,
    extract_dependencies,
)


def _cleanup_modules(prefix: str):
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]


@pytest.fixture
def trace_project(tmp_path):
    """Project with tracing test assets."""
    project_dir = tmp_path / "traceproj"
    project_dir.mkdir()

    mod_dir = project_dir / "tracemod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")

    (mod_dir / "helpers.py").write_text(textwrap.dedent("""\
        def normalize(text):
            return text.strip().lower()

        def format_output(value, prefix="result"):
            return f"{prefix}: {value}"
    """))

    (mod_dir / "assets.py").write_text(textwrap.dedent("""\
        from barca import asset, unsafe
        from tracemod.helpers import normalize

        THRESHOLD = 42

        def compute_score(x):
            return x * THRESHOLD

        @asset()
        def same_file_dep() -> dict:
            return {"score": compute_score(10)}

        @asset()
        def cross_file_dep() -> str:
            return normalize("  HELLO  ")

        @unsafe
        def load_dynamic():
            return eval("1 + 2")

        @asset()
        def uses_unsafe() -> dict:
            return {"value": load_dynamic()}
    """))

    _cleanup_modules("tracemod")
    clear_caches()
    sys.path.insert(0, str(project_dir))
    yield project_dir
    sys.path.remove(str(project_dir))
    _cleanup_modules("tracemod")
    clear_caches()


def test_same_file_dep_traced(trace_project):
    clear_caches()
    _cleanup_modules("tracemod")
    import tracemod.assets
    from tracemod.assets import same_file_dep

    original = getattr(same_file_dep, "__barca_original__", same_file_dep)
    result = extract_dependencies(original, str(trace_project))
    dep_keys = list(result.dependencies.keys())
    assert any("compute_score" in k for k in dep_keys), f"should trace compute_score, got {dep_keys}"


def test_cross_file_dep_traced(trace_project):
    clear_caches()
    _cleanup_modules("tracemod")
    import tracemod.helpers
    import tracemod.assets
    from tracemod.assets import cross_file_dep

    original = getattr(cross_file_dep, "__barca_original__", cross_file_dep)
    result = extract_dependencies(original, str(trace_project))
    dep_keys = list(result.dependencies.keys())
    assert any("normalize" in k for k in dep_keys), f"should trace normalize, got {dep_keys}"


def test_unsafe_skips_tracing(trace_project):
    clear_caches()
    _cleanup_modules("tracemod")
    import tracemod.assets
    from tracemod.assets import load_dynamic

    result = extract_dependencies(load_dynamic, str(trace_project))
    assert len(result.dependencies) == 0, "@unsafe should skip dependency tracing"


def test_purity_analysis(trace_project):
    clear_caches()
    _cleanup_modules("tracemod")
    import tracemod.assets
    from tracemod.assets import same_file_dep, load_dynamic

    original = getattr(same_file_dep, "__barca_original__", same_file_dep)
    result = analyze_purity(original)
    assert result.is_pure, "same_file_dep should be pure"

    result2 = analyze_purity(load_dynamic)
    assert not result2.is_pure, "load_dynamic should be impure (uses eval)"


def test_dependency_hash_changes_with_helper(trace_project):
    clear_caches()
    _cleanup_modules("tracemod")
    import tracemod.helpers
    import tracemod.assets
    from tracemod.assets import cross_file_dep

    original = getattr(cross_file_dep, "__barca_original__", cross_file_dep)
    hash1 = compute_dependency_hash(original, str(trace_project))

    # Change the helper
    helpers_file = trace_project / "tracemod" / "helpers.py"
    helpers_file.write_text(textwrap.dedent("""\
        def normalize(text):
            return text.strip().upper()

        def format_output(value, prefix="result"):
            return f"{prefix}: {value}"
    """))

    clear_caches()
    _cleanup_modules("tracemod")
    import tracemod.helpers as th
    importlib.reload(th)
    import tracemod.assets as ta
    importlib.reload(ta)
    from tracemod.assets import cross_file_dep as cross2
    original2 = getattr(cross2, "__barca_original__", cross2)

    hash2 = compute_dependency_hash(original2, str(trace_project))
    assert hash1 != hash2, "dependency hash should change when helper changes"
