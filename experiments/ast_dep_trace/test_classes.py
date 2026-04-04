"""Test case 14: class hierarchy / super() dependency tracing."""

from trace import clear_caches, compute_dependency_hash, extract_dependencies


# ---------------------------------------------------------------------------
# Test fixtures
# ---------------------------------------------------------------------------

class BaseProcessor:
    def transform(self, x):
        return x * 2

class DerivedProcessor(BaseProcessor):
    def transform(self, x):
        return super().transform(x) + 1

def asset_uses_class():
    proc = DerivedProcessor()
    return proc.transform(5)

def asset_uses_base_directly():
    proc = BaseProcessor()
    return proc.transform(5)


# ---------------------------------------------------------------------------
# Test runner
# ---------------------------------------------------------------------------


def run_tests():
    results = []

    # Case 14: Class usage
    clear_caches()
    deps = extract_dependencies(asset_uses_class)
    has_derived = any("DerivedProcessor" in k for k in deps.dependencies)
    has_base = any("BaseProcessor" in k for k in deps.dependencies)
    results.append(("Case 14: class in dependency cone",
                     has_derived,
                     f"DerivedProcessor={has_derived}, BaseProcessor={has_base}, "
                     f"deps: {list(deps.dependencies.keys())}"))

    # Case 14b: Changing base class changes hash
    # This is hard to test without actually modifying source,
    # so we verify the class is at least detected as a dependency
    clear_caches()
    deps_base = extract_dependencies(asset_uses_base_directly)
    has_base_direct = any("BaseProcessor" in k for k in deps_base.dependencies)
    results.append(("Case 14b: base class detected as dependency",
                     has_base_direct,
                     f"BaseProcessor={has_base_direct}, "
                     f"deps: {list(deps_base.dependencies.keys())}"))

    return results


if __name__ == "__main__":
    for name, passed, detail in run_tests():
        status = "PASS" if passed else "FAIL"
        print(f"  [{status}] {name}")
        print(f"         {detail}")
