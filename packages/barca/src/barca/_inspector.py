"""Pure-Python asset inspection — replaces the PyO3 inspect_modules."""

from __future__ import annotations

import importlib
import inspect
import sys
import textwrap
from pathlib import Path

from barca._models import InspectedAsset
from barca._trace import analyze_purity, compute_dependency_hash


def inspect_modules(
    module_names: list[str],
    project_root: str | None = None,
) -> list[InspectedAsset]:
    """Import modules, find @asset functions, extract metadata + dependency hashes."""
    version_info = sys.version_info
    python_version = f"{version_info.major}.{version_info.minor}.{version_info.micro}"

    assets: list[InspectedAsset] = []

    for module_name in module_names:
        try:
            module = importlib.import_module(module_name)
        except Exception as e:
            print(f"barca: skipping module '{module_name}': {e}", file=sys.stderr)
            continue

        # Get normalized module source
        try:
            module_source = textwrap.dedent(inspect.getsource(module)).strip() + "\n"
        except (TypeError, OSError):
            module_source = ""

        for _name, obj in inspect.getmembers(module):
            kind = getattr(obj, "__barca_kind__", None)
            if kind not in ("asset", "sensor", "effect"):
                continue

            # Skip class objects: accessing a @property on the class returns the
            # descriptor itself (not the computed value). We only want instances.
            if inspect.isclass(obj):
                continue

            metadata = getattr(obj, "__barca_metadata__", {})
            if not isinstance(metadata, dict):
                # Defensive: skip anything where metadata can't be serialised
                continue

            original = getattr(obj, "__barca_original__", obj)

            # Get function source
            try:
                function_source = textwrap.dedent(inspect.getsource(original)).strip() + "\n"
            except (TypeError, OSError):
                function_source = ""

            function_name = getattr(original, "__name__", "")

            # Get file path
            try:
                source_file = inspect.getsourcefile(original)
                file_path = str(Path(source_file).resolve()) if source_file else ""
            except (TypeError, OSError):
                file_path = ""

            # Get return type
            try:
                sig = inspect.signature(original)
                ra = sig.return_annotation
                if ra is inspect.Signature.empty:
                    return_type = None
                elif isinstance(ra, type):
                    return_type = ra.__name__
                else:
                    return_type = str(ra)
            except (TypeError, ValueError):
                return_type = None

            # Compute per-function dependency cone hash
            dep_hash: str | None = None
            purity_warnings: list[str] = []
            try:
                dep_hash = compute_dependency_hash(original, project_root)
            except Exception as e:
                print(f"barca: dependency tracing failed for {function_name}: {e}", file=sys.stderr)

            try:
                purity_result = analyze_purity(original)
                purity_warnings = purity_result.warnings
            except Exception as e:
                print(f"barca: purity analysis failed for {function_name}: {e}", file=sys.stderr)

            mod_path = getattr(module, "__name__", module_name)

            assets.append(
                InspectedAsset(
                    kind=kind,
                    module_path=mod_path,
                    file_path=file_path,
                    function_name=function_name,
                    function_source=function_source,
                    module_source=module_source,
                    decorator_metadata=metadata,
                    return_type=return_type,
                    python_version=python_version,
                    dependency_cone_hash=dep_hash,
                    purity_warnings=purity_warnings,
                )
            )

    return assets
