#!/usr/bin/env python3
"""Run all AST dependency tracing tests."""

import os
import sys
import time

# Ensure the experiment directory is on the path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))


def run_suite(name, module_name):
    """Import and run a test module, return (passed, failed, results)."""
    print(f"\n{'=' * 60}")
    print(f"  {name}")
    print(f"{'=' * 60}")

    try:
        mod = __import__(module_name)
        results = mod.run_tests()
    except Exception as e:
        print(f"  [ERROR] Failed to run: {e}")
        import traceback
        traceback.print_exc()
        return 0, 1, []

    passed = 0
    failed = 0
    for test_name, test_passed, detail in results:
        status = "PASS" if test_passed else "FAIL"
        if test_passed:
            passed += 1
        else:
            failed += 1
        print(f"  [{status}] {test_name}")
        print(f"         {detail}")

    return passed, failed, results


def main():
    t0 = time.perf_counter()

    suites = [
        ("Same-file dependency tracing", "test_same_file"),
        ("Cross-file dependency tracing", "test_cross_file"),
        ("Purity analysis", "test_purity"),
        ("Class hierarchy tracing", "test_classes"),
        ("Unsafe pattern detection", "test_limitations"),
    ]

    total_passed = 0
    total_failed = 0

    for name, module_name in suites:
        p, f, _ = run_suite(name, module_name)
        total_passed += p
        total_failed += f

    elapsed = time.perf_counter() - t0

    print(f"\n{'=' * 60}")
    print(f"  RESULTS: {total_passed} passed, {total_failed} failed "
          f"({elapsed:.3f}s)")
    print(f"{'=' * 60}")

    return 0 if total_failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
