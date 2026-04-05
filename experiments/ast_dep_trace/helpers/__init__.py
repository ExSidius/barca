"""Helper package for cross-file dependency tests.

Re-exports `exported_func` from submodule (Case 9: __init__.py re-export).
"""

from .submodule import exported_func

__all__ = ["exported_func"]
