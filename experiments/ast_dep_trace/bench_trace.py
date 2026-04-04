#!/usr/bin/env python3
"""Performance benchmark for AST dependency tracing.

Measures tracing speed on real Barca pipelines and synthetic scale tests.
"""

import os
import sys
import time
import tempfile
import importlib
import importlib.util
import textwrap

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from trace import extract_dependencies, compute_dependency_hash, analyze_purity, clear_caches

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def bench_module(name, module_path, project_root):
    """Benchmark tracing all @asset-decorated functions in a module."""
    # Add parent to sys.path so module is importable
    parent = os.path.dirname(module_path)
    if parent not in sys.path:
        sys.path.insert(0, parent)

    module_name = os.path.splitext(os.path.basename(module_path))[0]
    spec = importlib.util.spec_from_file_location(module_name, module_path)
    mod = importlib.util.module_from_spec(spec)
    try:
        spec.loader.exec_module(mod)
    except (ImportError, ModuleNotFoundError) as e:
        print(f"    SKIPPED: {e}")
        return 0, 0

    # Find all functions (not just @asset — we want all for benchmarking)
    funcs = [
        (fname, obj) for fname, obj in vars(mod).items()
        if callable(obj) and not fname.startswith("_")
        and hasattr(obj, "__module__")
    ]

    print(f"\n  {name}: {len(funcs)} functions in {os.path.basename(module_path)}")

    # Benchmark: extract_dependencies for all functions
    clear_caches()
    t0 = time.perf_counter()
    for fname, func in funcs:
        extract_dependencies(func, project_root)
    t_deps = time.perf_counter() - t0

    # Benchmark: compute_dependency_hash for all functions
    clear_caches()
    t0 = time.perf_counter()
    for fname, func in funcs:
        compute_dependency_hash(func, project_root)
    t_hash = time.perf_counter() - t0

    # Benchmark: analyze_purity for all functions
    t0 = time.perf_counter()
    for fname, func in funcs:
        analyze_purity(func)
    t_purity = time.perf_counter() - t0

    # Benchmark: cached re-run (should be near-instant)
    t0 = time.perf_counter()
    for fname, func in funcs:
        compute_dependency_hash(func, project_root)
    t_cached = time.perf_counter() - t0

    per_func = t_hash / len(funcs) if funcs else 0
    print(f"    extract_dependencies: {t_deps*1000:.1f}ms total, "
          f"{t_deps/len(funcs)*1000:.2f}ms/func")
    print(f"    compute_dep_hash:     {t_hash*1000:.1f}ms total, "
          f"{per_func*1000:.2f}ms/func")
    print(f"    analyze_purity:       {t_purity*1000:.1f}ms total, "
          f"{t_purity/len(funcs)*1000:.2f}ms/func")
    print(f"    cached re-hash:       {t_cached*1000:.1f}ms total "
          f"(speedup: {t_hash/t_cached:.1f}x)" if t_cached > 0 else
          f"    cached re-hash:       {t_cached*1000:.1f}ms total")

    return len(funcs), t_hash


def bench_synthetic(n_files, funcs_per_file):
    """Generate synthetic project with cross-file dependencies and benchmark."""
    total_funcs = n_files * funcs_per_file
    print(f"\n  Synthetic: {total_funcs} functions across {n_files} files")

    with tempfile.TemporaryDirectory() as tmpdir:
        # Generate files
        for f_idx in range(n_files):
            lines = []
            for fn_idx in range(funcs_per_file):
                global_idx = f_idx * funcs_per_file + fn_idx
                if global_idx == 0:
                    lines.append(f"def func_{global_idx:04d}():\n    return {global_idx}\n")
                else:
                    # Reference a function from previous in same file or cross-file
                    dep_idx = max(0, global_idx - 1)
                    dep_file_idx = dep_idx // funcs_per_file
                    if dep_file_idx == f_idx:
                        # Same file reference
                        lines.append(
                            f"def func_{global_idx:04d}():\n"
                            f"    return func_{dep_idx:04d}() + 1\n"
                        )
                    else:
                        # Cross-file import
                        lines.insert(0, f"from mod_{dep_file_idx:03d} import func_{dep_idx:04d}\n")
                        lines.append(
                            f"def func_{global_idx:04d}():\n"
                            f"    return func_{dep_idx:04d}() + 1\n"
                        )

            filepath = os.path.join(tmpdir, f"mod_{f_idx:03d}.py")
            with open(filepath, "w") as fp:
                fp.write("\n".join(lines))

        # Import all modules and collect functions
        sys.path.insert(0, tmpdir)
        all_funcs = []
        try:
            for f_idx in range(n_files):
                mod_name = f"mod_{f_idx:03d}"
                mod = importlib.import_module(mod_name)
                for fn_idx in range(funcs_per_file):
                    global_idx = f_idx * funcs_per_file + fn_idx
                    func = getattr(mod, f"func_{global_idx:04d}", None)
                    if func:
                        all_funcs.append(func)

            clear_caches()
            t0 = time.perf_counter()
            for func in all_funcs:
                compute_dependency_hash(func, tmpdir)
            t_total = time.perf_counter() - t0

            # Cached re-run
            t0 = time.perf_counter()
            for func in all_funcs:
                compute_dependency_hash(func, tmpdir)
            t_cached = time.perf_counter() - t0

            per_func = t_total / len(all_funcs) if all_funcs else 0
            print(f"    Full trace:   {t_total*1000:.1f}ms total, "
                  f"{per_func*1000:.2f}ms/func")
            print(f"    Cached re-run: {t_cached*1000:.1f}ms total "
                  f"(speedup: {t_total/t_cached:.1f}x)" if t_cached > 0 else
                  f"    Cached re-run: {t_cached*1000:.1f}ms total")

            return len(all_funcs), t_total

        finally:
            sys.path.remove(tmpdir)
            # Clean up imported modules
            for f_idx in range(n_files):
                mod_name = f"mod_{f_idx:03d}"
                if mod_name in sys.modules:
                    del sys.modules[mod_name]


def main():
    print("=" * 60)
    print("  AST Dependency Tracing — Performance Benchmark")
    print("=" * 60)

    # Benchmark 1: Spaceflights pipeline
    sf_path = os.path.join(REPO_ROOT, "benchmarks", "barca_bench", "bench_project", "spaceflights.py")
    sf_root = os.path.join(REPO_ROOT, "benchmarks", "barca_bench")
    if os.path.exists(sf_path):
        bench_module("Spaceflights", sf_path, sf_root)
    else:
        print(f"\n  Spaceflights: SKIPPED (not found at {sf_path})")

    # Benchmark 2: Iris pipeline
    iris_path = os.path.join(REPO_ROOT, "examples", "iris_pipeline", "iris_project", "assets.py")
    iris_root = os.path.join(REPO_ROOT, "examples", "iris_pipeline")
    if os.path.exists(iris_path):
        bench_module("Iris pipeline", iris_path, iris_root)
    else:
        print(f"\n  Iris pipeline: SKIPPED (not found at {iris_path})")

    # Benchmark 3: Synthetic scale tests
    for n_files, funcs_per_file in [(1, 100), (10, 10), (10, 50), (50, 20)]:
        bench_synthetic(n_files, funcs_per_file)

    print(f"\n{'=' * 60}")
    print(f"  Target: <1s for 100 assets. Subprocess spawn is ~60ms/asset.")
    print(f"{'=' * 60}")


if __name__ == "__main__":
    main()
