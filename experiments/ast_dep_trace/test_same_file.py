"""Test cases for same-file dependency tracing.

Cases 1, 2, 3, 5, 10, 11, 12: function calls, globals, transitive chains,
closures, decorators, comprehensions.
"""

import types
from trace import clear_caches, compute_dependency_hash, extract_dependencies

# ---------------------------------------------------------------------------
# Test fixtures — we define functions in module scope so they have __globals__
# ---------------------------------------------------------------------------


# Case 1: Same-file function call
def helper_v1():
    return 1


def asset_calls_helper():
    return helper_v1()


# Case 2: Global variable
THRESHOLD = 0.5


def asset_uses_global(x):
    return x > THRESHOLD


# Case 3: Unrelated function (negative test)
def clean(d):
    return d.strip()


def process(d):
    return clean(d).upper()


def unrelated():
    return 42


# Case 5: Transitive chain
def chain_a():
    return 1


def chain_b():
    return chain_a() + 1


def chain_c():
    return chain_b() + 1


# Case 10: Nested function / closure
def make_multiplier(factor):
    def multiply(x):
        return x * factor

    return multiply


multiplier_10 = make_multiplier(10)


def asset_uses_closure():
    return multiplier_10(5)


# Case 11: Decorated dependency
def my_cache(func):
    """Simple decorator."""
    import functools

    @functools.wraps(func)
    def wrapper(*args):
        return func(*args)

    wrapper.__wrapped__ = func
    return wrapper


@my_cache
def cached_helper(x):
    return x * 2


def asset_calls_decorated():
    return cached_helper(21)


# Case 12: Comprehension and f-string calls
def transform(x):
    return x * 2


def validate(x):
    return x > 0


def asset_with_comprehension(items):
    return [transform(x) for x in items if validate(x)]


def compute_label(x):
    return f"value={x}"


def asset_with_fstring(x):
    return f"Result: {compute_label(x)}"


# ---------------------------------------------------------------------------
# Test runner
# ---------------------------------------------------------------------------


def run_tests():
    results = []

    # Case 1: Same-file function call
    clear_caches()
    compute_dependency_hash(asset_calls_helper)
    deps1 = extract_dependencies(asset_calls_helper)
    has_helper = any("helper_v1" in k for k in deps1.dependencies)
    results.append(("Case 1: same-file function call", has_helper, f"Found helper_v1 in deps: {has_helper}, deps: {list(deps1.dependencies.keys())}"))

    # Case 1b: Changing helper changes hash
    # Simulate by creating a modified version
    clear_caches()
    original_hash = compute_dependency_hash(asset_calls_helper)

    # Modify helper_v1 by creating new function with different body
    exec_globals = dict(asset_calls_helper.__globals__)
    exec("def helper_v1(): return 999", exec_globals)
    modified_func = types.FunctionType(
        asset_calls_helper.__code__,
        exec_globals,
        asset_calls_helper.__name__,
    )
    clear_caches()
    modified_hash = compute_dependency_hash(modified_func)
    hash_changed = original_hash != modified_hash
    results.append(("Case 1b: helper change -> hash changes", hash_changed, f"original={original_hash[:16]}... modified={modified_hash[:16]}..."))

    # Case 2: Global variable
    clear_caches()
    deps2 = extract_dependencies(asset_uses_global)
    has_threshold = any("THRESHOLD" in k for k in deps2.dependencies)
    results.append(("Case 2: global variable reference", has_threshold, f"Found THRESHOLD in deps: {has_threshold}, deps: {list(deps2.dependencies.keys())}"))

    # Case 2b: Changing global changes hash
    clear_caches()
    h2_orig = compute_dependency_hash(asset_uses_global)
    exec_globals2 = dict(asset_uses_global.__globals__)
    exec_globals2["THRESHOLD"] = 0.9
    modified_func2 = types.FunctionType(
        asset_uses_global.__code__,
        exec_globals2,
        asset_uses_global.__name__,
    )
    clear_caches()
    h2_mod = compute_dependency_hash(modified_func2)
    results.append(("Case 2b: global change -> hash changes", h2_orig != h2_mod, f"original={h2_orig[:16]}... modified={h2_mod[:16]}..."))

    # Case 3: Unrelated function (negative test)
    clear_caches()
    compute_dependency_hash(process)
    deps3 = extract_dependencies(process)
    has_clean = any("clean" in k for k in deps3.dependencies)
    has_unrelated = any("unrelated" in k for k in deps3.dependencies)
    results.append(("Case 3: unrelated function NOT in deps", has_clean and not has_unrelated, f"clean={has_clean}, unrelated={has_unrelated}, deps: {list(deps3.dependencies.keys())}"))

    # Case 5: Transitive chain
    clear_caches()
    deps5 = extract_dependencies(chain_c)
    has_a = any("chain_a" in k for k in deps5.dependencies)
    has_b = any("chain_b" in k for k in deps5.dependencies)
    results.append(("Case 5: transitive chain a->b->c", has_a and has_b, f"chain_a={has_a}, chain_b={has_b}, deps: {list(deps5.dependencies.keys())}"))

    # Case 5b: Changing a changes hash of c
    clear_caches()
    h5_orig = compute_dependency_hash(chain_c)
    exec_globals5 = dict(chain_c.__globals__)
    exec("def chain_a(): return 999", exec_globals5)
    # Also need to update chain_b's globals so it sees new chain_a
    mod_b = types.FunctionType(chain_b.__code__, exec_globals5, "chain_b")
    exec_globals5["chain_b"] = mod_b
    mod_c = types.FunctionType(chain_c.__code__, exec_globals5, "chain_c")
    clear_caches()
    h5_mod = compute_dependency_hash(mod_c)
    results.append(("Case 5b: change a -> hash of c changes", h5_orig != h5_mod, f"original={h5_orig[:16]}... modified={h5_mod[:16]}..."))

    # Case 10: Closure
    clear_caches()
    deps10 = extract_dependencies(asset_uses_closure)
    # multiplier_10 is a closure wrapping multiply(); trace should find
    # either 'multiplier_10', 'multiply', or 'closure:factor'
    has_closure_dep = any("multiply" in k or "multiplier" in k or "closure:" in k for k in deps10.dependencies)
    results.append(("Case 10: closure dependency", has_closure_dep, f"Found closure deps: {has_closure_dep}, deps: {list(deps10.dependencies.keys())}"))

    # Case 12: Comprehension calls
    clear_caches()
    deps12 = extract_dependencies(asset_with_comprehension)
    has_transform = any("transform" in k for k in deps12.dependencies)
    has_validate = any("validate" in k for k in deps12.dependencies)
    results.append(("Case 12: comprehension function calls", has_transform and has_validate, f"transform={has_transform}, validate={has_validate}, deps: {list(deps12.dependencies.keys())}"))

    # Case 12b: f-string calls
    clear_caches()
    deps12b = extract_dependencies(asset_with_fstring)
    has_compute = any("compute_label" in k for k in deps12b.dependencies)
    results.append(("Case 12b: f-string function calls", has_compute, f"Found compute_label: {has_compute}, deps: {list(deps12b.dependencies.keys())}"))

    return results


if __name__ == "__main__":
    for name, passed, detail in run_tests():
        status = "PASS" if passed else "FAIL"
        print(f"  [{status}] {name}")
        print(f"         {detail}")
