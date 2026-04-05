"""Submodule for testing relative imports and __init__.py re-exports."""


def exported_func(x):
    """Function re-exported via __init__.py."""
    return x * 2 + 1


def internal_helper():
    """Not re-exported — should not appear in tests unless directly imported."""
    return 42
