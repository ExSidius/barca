"""Pure-Python asset inspection.

Walks the imported modules, finds @asset/@sensor/@effect decorated
functions, and extracts everything needed for hashing + indexing.

For every @asset that has stacked @sink decorators, the inspector emits
one additional ``InspectedAsset`` per sink. Sinks are modelled as child
nodes keyed by ``{parent_continuity_key}::sink::{path}``.
"""

from __future__ import annotations

import importlib
import inspect
import sys
import textwrap
from pathlib import Path

from barca._models import InspectedAsset
from barca._trace import analyze_purity, compute_dependency_hash
from barca._unsafe import is_unsafe


def inspect_modules(
    module_names: list[str],
    project_root: str | None = None,
) -> list[InspectedAsset]:
    """Import modules, find decorated functions, extract metadata + hashes."""
    version_info = sys.version_info
    python_version = f"{version_info.major}.{version_info.minor}.{version_info.micro}"

    assets: list[InspectedAsset] = []
    seen_asset_keys: set[tuple[str, str]] = set()  # (file_path, function_name)

    for module_name in module_names:
        try:
            module = importlib.import_module(module_name)
        except Exception as e:  # pragma: no cover
            print(f"barca: skipping module '{module_name}': {e}", file=sys.stderr)
            continue

        try:
            module_source = textwrap.dedent(inspect.getsource(module)).strip() + "\n"
        except (TypeError, OSError):
            module_source = ""

        for _name, obj in inspect.getmembers(module):
            kind = getattr(obj, "__barca_kind__", None)
            if kind not in ("asset", "sensor", "effect"):
                continue
            if inspect.isclass(obj):
                continue

            metadata = getattr(obj, "__barca_metadata__", {})
            if not isinstance(metadata, dict):
                continue

            original = getattr(obj, "__barca_original__", obj)
            function_name = getattr(original, "__name__", "")

            try:
                source_file = inspect.getsourcefile(original)
                file_path = str(Path(source_file).resolve()) if source_file else ""
            except (TypeError, OSError):
                file_path = ""

            dedupe_key = (file_path, function_name)
            if dedupe_key in seen_asset_keys:
                continue
            seen_asset_keys.add(dedupe_key)

            try:
                function_source = textwrap.dedent(inspect.getsource(original)).strip() + "\n"
            except (TypeError, OSError):
                function_source = ""

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

            dep_hash: str | None = None
            purity_warnings: list[str] = []
            if not is_unsafe(original):
                try:
                    dep_hash = compute_dependency_hash(original, project_root)
                except Exception as e:  # pragma: no cover
                    print(
                        f"barca: dependency tracing failed for {function_name}: {e}",
                        file=sys.stderr,
                    )
                try:
                    purity_result = analyze_purity(original)
                    purity_warnings = purity_result.warnings
                except Exception as e:  # pragma: no cover
                    print(
                        f"barca: purity analysis failed for {function_name}: {e}",
                        file=sys.stderr,
                    )
            else:
                # Unsafe assets: hash only the function's own source, no cone
                from barca._hashing import sha256_hex

                dep_hash = sha256_hex(function_source.encode())
                purity_warnings = []

            mod_path = getattr(module, "__name__", module_name)

            parent_asset = InspectedAsset(
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
            assets.append(parent_asset)

            # If this is an @asset with stacked @sink decorators, emit one
            # InspectedAsset per sink. Sinks inherit the parent's source and
            # dep cone — their identity comes from (parent, path).
            if kind == "asset":
                sinks = getattr(obj, "__barca_sinks__", None) or []
                for spec in sinks:
                    sink_function_name = f"{function_name}__sink__{spec.path}"
                    sink_metadata = {
                        "kind": "sink",
                        "parent_function_name": function_name,
                        "path": spec.path,
                        "serializer": spec.serializer,
                        "freshness": metadata.get("freshness", "always"),
                    }
                    assets.append(
                        InspectedAsset(
                            kind="sink",
                            module_path=mod_path,
                            file_path=file_path,
                            function_name=sink_function_name,
                            function_source=function_source,
                            module_source=module_source,
                            decorator_metadata=sink_metadata,
                            return_type=return_type,
                            python_version=python_version,
                            dependency_cone_hash=dep_hash,
                            purity_warnings=[],
                            parent_function_name=function_name,
                            sink_path=spec.path,
                            sink_serializer=spec.serializer,
                        )
                    )

    return assets
