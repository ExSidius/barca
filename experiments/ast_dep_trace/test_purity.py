"""Test cases for purity analysis.

Tests that analyze_purity correctly identifies pure vs impure functions
and emits appropriate warnings.
"""

import random
from trace import analyze_purity

from unsafe_decorator import unsafe

# ---------------------------------------------------------------------------
# Pure functions (should pass purity check)
# ---------------------------------------------------------------------------


def pure_math(x, y):
    return x * y + 1


def pure_string(s):
    return s.lower().strip()


def pure_with_local_mutation(items):
    result = []
    for item in items:
        result.append(item * 2)
    return result


def pure_with_conditional(x):
    if x > 0:
        return "positive"
    return "non-positive"


# ---------------------------------------------------------------------------
# Impure functions (should fail purity check)
# ---------------------------------------------------------------------------


def impure_global_mutation():
    global SOME_STATE
    SOME_STATE = 42
    return SOME_STATE


def impure_nonlocal():
    counter = 0

    def increment():
        nonlocal counter
        counter += 1

    increment()
    return counter


def impure_file_io():
    with open("/tmp/test.txt") as f:
        return f.read()


def impure_print(msg):
    print(msg)
    return msg


def impure_exec():
    exec("x = 1")


def impure_eval():
    return eval("1 + 1")


def impure_getattr_dispatch(obj, name):
    return getattr(obj, name)()


def impure_subprocess():
    import subprocess

    subprocess.run(["echo", "hello"])


def impure_random():
    return random.random()


def impure_setattr(obj):
    obj.x = 42


def impure_attr_mutation(obj):
    obj.value = 42


def impure_subscript_mutation(d):
    d["key"] = "value"


def impure_generator():
    yield 1
    yield 2


# ---------------------------------------------------------------------------
# @unsafe functions
# ---------------------------------------------------------------------------


@unsafe
def acknowledged_impure():
    return eval("1 + 1")


@unsafe(cache=True)
def acknowledged_with_cache():
    return open("/etc/hostname").read()


# ---------------------------------------------------------------------------
# Test runner
# ---------------------------------------------------------------------------


def run_tests():
    results = []

    # Pure functions should pass
    for func in [pure_math, pure_string, pure_with_local_mutation, pure_with_conditional]:
        r = analyze_purity(func)
        results.append((f"Pure: {func.__name__}", r.is_pure, f"is_pure={r.is_pure}, reasons={r.impure_reasons}"))

    # Impure functions should fail
    impure_funcs = [
        (impure_global_mutation, "global"),
        (impure_nonlocal, "nonlocal"),
        (impure_file_io, "open"),
        (impure_print, "print"),
        (impure_exec, "exec"),
        (impure_eval, "eval"),
        (impure_getattr_dispatch, "getattr"),
        (impure_random, "random"),
        (impure_setattr, "setattr"),
        (impure_attr_mutation, "attr mutation"),
        (impure_subscript_mutation, "subscript mutation"),
        (impure_generator, "generator"),
    ]

    for func, expected_reason in impure_funcs:
        r = analyze_purity(func)
        results.append((f"Impure: {func.__name__} ({expected_reason})", not r.is_pure, f"is_pure={r.is_pure}, reasons={r.impure_reasons[:2]}"))

    # @unsafe functions
    r_unsafe = analyze_purity(acknowledged_impure)
    results.append(
        ("@unsafe: acknowledged_impure", not r_unsafe.is_pure and len(r_unsafe.warnings) == 0, f"is_pure={r_unsafe.is_pure}, warnings={r_unsafe.warnings}, reasons={r_unsafe.impure_reasons}")
    )

    # getattr should produce a warning
    r_getattr = analyze_purity(impure_getattr_dispatch)
    has_warning = any("getattr" in w for w in r_getattr.warnings)
    results.append(("Warning: getattr produces actionable warning", has_warning, f"warnings={r_getattr.warnings[:1]}"))

    # open should produce extra_deps suggestion
    r_open = analyze_purity(impure_file_io)
    has_file_warning = any("extra_deps" in w for w in r_open.warnings)
    results.append(("Warning: open() suggests extra_deps", has_file_warning, f"warnings={r_open.warnings[:1]}"))

    return results


if __name__ == "__main__":
    for name, passed, detail in run_tests():
        status = "PASS" if passed else "FAIL"
        print(f"  [{status}] {name}")
        print(f"         {detail}")
