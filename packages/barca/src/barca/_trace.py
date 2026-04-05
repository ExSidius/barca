"""AST-level dependency tracing for Python functions.

Extracts the transitive dependency cone of a function — every project-local
function, variable, and import it references — and computes a hash over that
cone. This enables per-function staleness detection instead of whole-codebase
invalidation.

Three public APIs:
    extract_dependencies(func) -> dict[str, str]
    analyze_purity(func) -> PurityResult
    compute_dependency_hash(func) -> str
"""

import ast
import hashlib
import inspect
import os
import sysconfig
import textwrap
from dataclasses import dataclass, field
from functools import lru_cache
from pathlib import Path

from barca._unsafe import is_unsafe

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

# Standard library paths (computed once)
_STDLIB_PATHS: set[str] = set()
for _key in ("stdlib", "platstdlib", "purelib", "platlib"):
    _p = sysconfig.get_paths().get(_key)
    if _p:
        _STDLIB_PATHS.add(os.path.realpath(_p))


# Known impure function names (module.func or bare func)
_IMPURE_CALLS = {
    "open", "print", "exec", "eval", "input",
    "setattr", "delattr",
}

_IMPURE_ATTR_CALLS = {
    ("os", "environ"),
    ("os", "system"),
    ("os", "popen"),
    ("subprocess", "run"),
    ("subprocess", "call"),
    ("subprocess", "Popen"),
    ("random", "random"),
    ("random", "randint"),
    ("random", "choice"),
    ("random", "uniform"),
    ("random", "shuffle"),
    ("time", "time"),
    ("datetime", "now"),
    ("importlib", "import_module"),
}

_NONDETERMINISTIC_MODULES = {"random", "time", "datetime"}

_DYNAMIC_DISPATCH_CALLS = {"getattr", "globals"}

# ---------------------------------------------------------------------------
# Data structures
# ---------------------------------------------------------------------------


@dataclass
class PurityResult:
    is_pure: bool
    warnings: list[str] = field(default_factory=list)
    impure_reasons: list[str] = field(default_factory=list)


@dataclass
class TraceResult:
    """Result of extract_dependencies."""
    dependencies: dict[str, str]  # qualified_name -> source_or_repr
    warnings: list[str] = field(default_factory=list)


# ---------------------------------------------------------------------------
# Module AST cache (parse each file at most once)
# ---------------------------------------------------------------------------


@lru_cache(maxsize=256)
def _parse_module_ast(filepath: str) -> ast.Module:
    """Parse a Python file and cache the AST."""
    with open(filepath) as f:
        return ast.parse(f.read(), filename=filepath)


@lru_cache(maxsize=256)
def _read_source(filepath: str) -> str:
    with open(filepath) as f:
        return f.read()


def clear_caches():
    """Clear all module-level caches. Call between test runs if needed."""
    _parse_module_ast.cache_clear()
    _read_source.cache_clear()
    _dep_cache.clear()


# ---------------------------------------------------------------------------
# Project boundary detection
# ---------------------------------------------------------------------------


def _is_project_local(filepath: str | None, project_root: str) -> bool:
    """Determine if a file is part of the user's project (not stdlib/third-party)."""
    if filepath is None:
        return False
    try:
        real = os.path.realpath(filepath)
    except (OSError, ValueError):
        return False
    # Must be under project root
    if not real.startswith(project_root):
        return False
    # Must not be in site-packages
    if "site-packages" in real:
        return False
    # Must not be in stdlib paths
    for sp in _STDLIB_PATHS:
        if real.startswith(sp):
            return False
    return True


def _get_file_safe(obj) -> str | None:
    """Get the source file of an object, or None."""
    try:
        return inspect.getfile(obj)
    except (TypeError, OSError):
        return None


def _get_source_safe(obj) -> str | None:
    """Get dedented source code of an object, or None."""
    try:
        return textwrap.dedent(inspect.getsource(obj))
    except (TypeError, OSError):
        return None


# ---------------------------------------------------------------------------
# AST name extraction (single-pass)
# ---------------------------------------------------------------------------


def _collect_referenced_names(source: str) -> tuple[set[str], list[ast.Import | ast.ImportFrom]]:
    """Walk a function's AST and collect all referenced names and imports.

    Returns (names, imports) where names is the set of Name nodes in Load
    context and imports is the list of import statements.
    """
    try:
        tree = ast.parse(textwrap.dedent(source))
    except SyntaxError:
        return set(), []

    names: set[str] = set()
    imports: list[ast.Import | ast.ImportFrom] = []

    for node in ast.walk(tree):
        # Names in Load context (variable/function references)
        if isinstance(node, ast.Name) and isinstance(node.ctx, ast.Load):
            names.add(node.id)

        # Attribute access: obj.attr — track the root object name
        elif isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
            names.add(node.value.id)

        # Import statements
        elif isinstance(node, (ast.Import, ast.ImportFrom)):
            imports.append(node)

    return names, imports


# ---------------------------------------------------------------------------
# Dependency extraction
# ---------------------------------------------------------------------------


def _make_dep_key(filepath: str, name: str) -> str:
    """Create a unique key for a dependency: 'filepath:name'."""
    return f"{filepath}:{name}"


def _process_single_func(func, project_root, visited, result):
    """Process one function: extract its direct references (no recursion).

    Returns a list of (callable_obj,) to process next (worklist items).
    """
    func_source = _get_source_safe(func)
    if func_source is None:
        return []

    func_file = _get_file_safe(func)
    func_name = getattr(func, "__name__", repr(func))
    func_globals = getattr(func, "__globals__", {})

    # Single-pass: collect all referenced names and imports
    names, imports = _collect_referenced_names(func_source)

    worklist = []

    # Process referenced names
    for name in names:
        obj = func_globals.get(name)
        if obj is None:
            continue

        # Module reference (import mod; mod.func())
        if inspect.ismodule(obj):
            obj_file = _get_file_safe(obj)
            if obj_file and _is_project_local(obj_file, project_root):
                dep_key = _make_dep_key(obj_file, name)
                if dep_key not in visited:
                    visited.add(dep_key)
                    mod_source = _get_source_safe(obj)
                    if mod_source:
                        result.dependencies[dep_key] = mod_source
            continue

        # Callable (function, class)
        if callable(obj):
            obj_file = _get_file_safe(obj)
            if obj_file and _is_project_local(obj_file, project_root):
                obj_name = getattr(obj, "__name__", name)
                dep_key = _make_dep_key(obj_file, obj_name)
                if dep_key not in visited:
                    visited.add(dep_key)
                    obj_source = _get_source_safe(obj)
                    if obj_source:
                        result.dependencies[dep_key] = obj_source
                        worklist.append(obj)
        else:
            # Non-callable: global variable
            dep_key = _make_dep_key(func_file or "<unknown>", f"global:{name}")
            if dep_key not in visited:
                visited.add(dep_key)
                result.dependencies[dep_key] = repr(obj)

    # Process imports
    for imp in imports:
        if isinstance(imp, ast.ImportFrom):
            if imp.module and imp.module.startswith("__future__"):
                continue
            # Star imports
            if imp.names and any(alias.name == "*" for alias in imp.names):
                mod_name = imp.module or ""
                mod_obj = func_globals.get(mod_name)
                if mod_obj is None:
                    try:
                        import importlib
                        mod_obj = importlib.import_module(mod_name)
                    except (ImportError, ModuleNotFoundError):
                        result.warnings.append(f"Cannot resolve star import: from {mod_name} import *")
                        continue
                if mod_obj:
                    mod_file = _get_file_safe(mod_obj)
                    if mod_file and _is_project_local(mod_file, project_root):
                        dep_key = _make_dep_key(mod_file, f"star:{mod_name}")
                        if dep_key not in visited:
                            visited.add(dep_key)
                            mod_source = _read_source(mod_file) if os.path.isfile(mod_file) else None
                            if mod_source:
                                result.dependencies[dep_key] = mod_source
                                result.warnings.append(
                                    f"Star import 'from {mod_name} import *' — "
                                    f"hashing entire module (consider explicit imports)"
                                )
                continue

            # Named imports
            for alias in imp.names:
                imported_name = alias.asname or alias.name
                obj = func_globals.get(imported_name)
                if obj is None:
                    continue
                obj_file = _get_file_safe(obj)
                if obj_file and _is_project_local(obj_file, project_root):
                    obj_name = getattr(obj, "__name__", imported_name)
                    dep_key = _make_dep_key(obj_file, obj_name) if callable(obj) else \
                              _make_dep_key(obj_file, f"imported:{imported_name}")
                    if dep_key not in visited:
                        visited.add(dep_key)
                        if callable(obj):
                            obj_source = _get_source_safe(obj)
                            if obj_source:
                                result.dependencies[dep_key] = obj_source
                                worklist.append(obj)
                        else:
                            result.dependencies[dep_key] = repr(obj)

    # Closure variables
    try:
        closure_vars = inspect.getclosurevars(func)
        for name, val in closure_vars.nonlocals.items():
            dep_key = _make_dep_key(func_file or "<unknown>", f"closure:{name}")
            if dep_key not in visited:
                visited.add(dep_key)
                if callable(val):
                    src = _get_source_safe(val)
                    if src:
                        result.dependencies[dep_key] = src
                else:
                    result.dependencies[dep_key] = repr(val)
    except TypeError:
        pass

    return worklist


# Memoized results keyed by (func_id, project_root)
_dep_cache: dict[tuple[int, str], TraceResult] = {}


def extract_dependencies(func, project_root: str | None = None) -> TraceResult:
    """Extract all project-local dependencies of a function, transitively.

    Uses an iterative worklist (not recursion) to handle deep chains.
    Results are memoized per function.
    """
    if project_root is None:
        func_file = _get_file_safe(func)
        project_root = os.path.dirname(os.path.realpath(func_file)) if func_file else os.getcwd()
    project_root = os.path.realpath(project_root)

    # For @unsafe functions, don't trace dependencies
    if is_unsafe(func):
        return TraceResult(dependencies={})

    # Check cache
    cache_key = (id(func), project_root)
    if cache_key in _dep_cache:
        return _dep_cache[cache_key]

    result = TraceResult(dependencies={})
    visited: set[str] = set()

    # Mark the root function as visited
    func_file = _get_file_safe(func)
    func_name = getattr(func, "__name__", repr(func))
    root_key = _make_dep_key(func_file or "<unknown>", func_name)
    visited.add(root_key)

    # Iterative worklist: process functions breadth-first
    worklist = [func]
    while worklist:
        current = worklist.pop()
        # Check if this dependency was already fully traced (from a prior call)
        dep_cache_key = (id(current), project_root)
        if dep_cache_key in _dep_cache:
            cached = _dep_cache[dep_cache_key]
            result.dependencies.update(cached.dependencies)
            result.warnings.extend(cached.warnings)
            continue
        new_items = _process_single_func(current, project_root, visited, result)
        worklist.extend(new_items)

    _dep_cache[cache_key] = result
    return result


# ---------------------------------------------------------------------------
# Purity analysis
# ---------------------------------------------------------------------------


def analyze_purity(func) -> PurityResult:
    """Analyze whether a function is pure (no side effects, deterministic).

    Returns PurityResult with is_pure, warnings, and impure_reasons.
    """
    if is_unsafe(func):
        return PurityResult(
            is_pure=False,
            impure_reasons=["Function is decorated with @unsafe"],
        )

    source = _get_source_safe(func)
    if source is None:
        return PurityResult(
            is_pure=False,
            impure_reasons=["Cannot retrieve function source"],
        )

    try:
        tree = ast.parse(textwrap.dedent(source))
    except SyntaxError:
        return PurityResult(
            is_pure=False,
            impure_reasons=["Cannot parse function source"],
        )

    result = PurityResult(is_pure=True)
    func_name = getattr(func, "__name__", "<unknown>")
    func_file = _get_file_safe(func) or "<unknown>"

    for node in ast.walk(tree):
        # global / nonlocal statements
        if isinstance(node, ast.Global):
            result.is_pure = False
            names = ", ".join(node.names)
            result.impure_reasons.append(f"'global {names}' — mutates module state")

        elif isinstance(node, ast.Nonlocal):
            result.is_pure = False
            names = ", ".join(node.names)
            result.impure_reasons.append(f"'nonlocal {names}' — mutates enclosing scope")

        # yield / yield from (generators are stateful)
        elif isinstance(node, (ast.Yield, ast.YieldFrom)):
            result.is_pure = False
            result.impure_reasons.append("yield — function is a stateful generator")

        # Attribute/subscript assignment (obj.x = val, d[k] = val)
        elif isinstance(node, (ast.Assign, ast.AugAssign)):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            for target in targets:
                if isinstance(target, ast.Attribute):
                    result.is_pure = False
                    result.impure_reasons.append(
                        "Attribute assignment — likely mutating external object"
                    )
                elif isinstance(target, ast.Subscript):
                    result.is_pure = False
                    result.impure_reasons.append(
                        "Subscript assignment — likely mutating external container"
                    )

        # Function calls
        elif isinstance(node, ast.Call):
            # Direct calls: open(), exec(), eval(), etc.
            if isinstance(node.func, ast.Name):
                call_name = node.func.id
                if call_name in _IMPURE_CALLS:
                    result.is_pure = False
                    result.impure_reasons.append(f"Call to '{call_name}()' — side effect")
                    if call_name == "open":
                        result.warnings.append(
                            f"WARNING: {func_name} ({func_file}) calls open() — "
                            f"external data dependency. Consider @asset(extra_deps=[...])"
                        )
                if call_name in _DYNAMIC_DISPATCH_CALLS:
                    result.is_pure = False
                    result.impure_reasons.append(
                        f"Call to '{call_name}()' — dynamic dispatch, cannot trace target"
                    )
                    result.warnings.append(
                        f"WARNING: {func_name} ({func_file}) uses {call_name}() — "
                        f"dependency tracing cannot guarantee correctness. Consider:\n"
                        f"  1. Extract dynamic code into a helper decorated with @unsafe\n"
                        f"  2. Or add @unsafe to this function to silence this warning"
                    )

            # Attribute calls: os.environ, subprocess.run(), etc.
            elif isinstance(node.func, ast.Attribute):
                if isinstance(node.func.value, ast.Name):
                    mod = node.func.value.id
                    attr = node.func.attr
                    if (mod, attr) in _IMPURE_ATTR_CALLS:
                        result.is_pure = False
                        result.impure_reasons.append(
                            f"Call to '{mod}.{attr}()' — side effect or non-deterministic"
                        )
                        if mod == "importlib" and attr == "import_module":
                            result.warnings.append(
                                f"WARNING: {func_name} ({func_file}) uses "
                                f"importlib.import_module() — dynamic import, "
                                f"cannot trace target module"
                            )
                    elif mod in _NONDETERMINISTIC_MODULES:
                        result.is_pure = False
                        result.impure_reasons.append(
                            f"Call to '{mod}.{attr}()' — potentially non-deterministic"
                        )

        # Attribute access (not call) on known impure modules
        elif isinstance(node, ast.Attribute):
            if (isinstance(node.value, ast.Name)
                    and node.value.id == "os" and node.attr == "environ"
                    and isinstance(node.ctx, ast.Load)):
                result.is_pure = False
                result.impure_reasons.append("Access to os.environ — external state dependency")

    return result


# ---------------------------------------------------------------------------
# Dependency hashing
# ---------------------------------------------------------------------------


def compute_dependency_hash(func, project_root: str | None = None) -> str:
    """Compute a SHA-256 hash over a function and its transitive dependency cone.

    For @unsafe functions: hash source only, return with a special prefix.
    For pure functions: hash source + all transitive dependencies.
    For impure non-@unsafe functions: hash source + full module source (conservative).
    """
    func_source = _get_source_safe(func) or ""

    if is_unsafe(func):
        payload = f"UNSAFE:{func_source}"
        return hashlib.sha256(payload.encode()).hexdigest()

    trace = extract_dependencies(func, project_root)

    # Build deterministic payload: function source + sorted dependencies
    parts = [func_source]
    for key in sorted(trace.dependencies.keys()):
        parts.append(f"\n--- {key} ---\n{trace.dependencies[key]}")

    payload = "\n".join(parts)
    return hashlib.sha256(payload.encode()).hexdigest()
