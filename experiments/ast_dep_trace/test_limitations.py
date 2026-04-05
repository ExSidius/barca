"""Test cases L1-L5: unsafe pattern detection and warnings.

These test the detection of fundamentally untraceable patterns and
the @unsafe decorator's warning suppression.
"""

from trace import analyze_purity, compute_dependency_hash, extract_dependencies, clear_caches
from unsafe_decorator import unsafe, is_unsafe


# ---------------------------------------------------------------------------
# L1: Dynamic dispatch warning
# ---------------------------------------------------------------------------

def func_with_getattr(obj, name):
    return getattr(obj, name)()

@unsafe
def func_with_getattr_safe(obj, name):
    return getattr(obj, name)()


# ---------------------------------------------------------------------------
# L2: eval/exec warning
# ---------------------------------------------------------------------------

def func_with_eval():
    return eval("1 + 1")

@unsafe
def func_with_eval_safe():
    return eval("1 + 1")


# ---------------------------------------------------------------------------
# L3: Dynamic import warning
# ---------------------------------------------------------------------------

def func_with_dynamic_import(module_name):
    import importlib
    return importlib.import_module(module_name)

@unsafe
def func_with_dynamic_import_safe(module_name):
    import importlib
    return importlib.import_module(module_name)


# ---------------------------------------------------------------------------
# L4: @unsafe propagation
# ---------------------------------------------------------------------------

@unsafe
def unsafe_source():
    return eval("42")

def downstream_of_unsafe():
    """This depends on an @unsafe function via globals."""
    return unsafe_source() + 1


# ---------------------------------------------------------------------------
# L5: Non-Python data dependency
# ---------------------------------------------------------------------------

def func_reads_file():
    with open("config.yaml") as f:
        return f.read()


# ---------------------------------------------------------------------------
# Test runner
# ---------------------------------------------------------------------------


def run_tests():
    results = []

    # L1: getattr warning
    r1 = analyze_purity(func_with_getattr)
    has_warning = any("getattr" in w for w in r1.warnings)
    results.append(("L1: getattr() emits warning",
                     not r1.is_pure and has_warning,
                     f"is_pure={r1.is_pure}, warnings={r1.warnings[:1]}"))

    # L1b: @unsafe silences warning
    r1b = analyze_purity(func_with_getattr_safe)
    results.append(("L1b: @unsafe silences getattr warning",
                     not r1b.is_pure and len(r1b.warnings) == 0,
                     f"is_pure={r1b.is_pure}, warnings={r1b.warnings}"))

    # L2: eval warning
    r2 = analyze_purity(func_with_eval)
    results.append(("L2: eval() detected as impure",
                     not r2.is_pure,
                     f"is_pure={r2.is_pure}, reasons={r2.impure_reasons[:1]}"))

    # L2b: @unsafe silences
    r2b = analyze_purity(func_with_eval_safe)
    results.append(("L2b: @unsafe silences eval warning",
                     not r2b.is_pure and len(r2b.warnings) == 0,
                     f"is_pure={r2b.is_pure}, warnings={r2b.warnings}"))

    # L3: dynamic import warning
    r3 = analyze_purity(func_with_dynamic_import)
    has_import_warning = any("import_module" in w for w in r3.warnings)
    results.append(("L3: importlib.import_module() emits warning",
                     not r3.is_pure and has_import_warning,
                     f"is_pure={r3.is_pure}, warnings={r3.warnings[:1]}"))

    # L4: @unsafe propagation — downstream sees unsafe_source as dependency
    clear_caches()
    deps4 = extract_dependencies(downstream_of_unsafe)
    # The downstream should detect the @unsafe function
    has_unsafe_dep = any("unsafe_source" in k for k in deps4.dependencies)
    unsafe_dep_is_marked = is_unsafe(unsafe_source)
    results.append(("L4: @unsafe function detected in dependency cone",
                     unsafe_dep_is_marked,
                     f"unsafe_source is @unsafe={unsafe_dep_is_marked}, "
                     f"found in deps={has_unsafe_dep}, "
                     f"deps: {list(deps4.dependencies.keys())}"))

    # L5: open() suggests extra_deps
    r5 = analyze_purity(func_reads_file)
    has_file_warning = any("extra_deps" in w for w in r5.warnings)
    results.append(("L5: open() suggests @asset(extra_deps=...)",
                     not r5.is_pure and has_file_warning,
                     f"is_pure={r5.is_pure}, warnings={r5.warnings[:1]}"))

    return results


if __name__ == "__main__":
    for name, passed, detail in run_tests():
        status = "PASS" if passed else "FAIL"
        print(f"  [{status}] {name}")
        print(f"         {detail}")
