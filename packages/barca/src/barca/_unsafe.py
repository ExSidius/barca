"""@unsafe decorator — mark functions as having untraceable behaviour.

Inspired by Rust's ``unsafe`` blocks. A function decorated with ``@unsafe``:

- Has its dependency_cone_hash computed from its own source only (no
  transitive AST tracing). Editing a helper the function calls does NOT
  invalidate the asset.
- Silences the purity-analysis warnings (globals, I/O calls, etc).
- Carries no correctness guarantee — Barca trusts the developer.

Caching behaviour is identical to pure assets: if inputs and definition
are unchanged, the asset is a cache hit and does not re-materialise.
This matches design decision D10 in ``barca.allium``.

The older ``@unsafe(cache=...)`` parameter has been removed. Caching is
no longer opt-in for unsafe assets — it's always on, same as pure assets.
"""


def unsafe(func=None):
    """Mark a function as having untraceable behaviour.

    Can be used with or without parens::

        @unsafe
        def load_config():
            return yaml.safe_load(open("config.yaml"))

        @unsafe()
        def load_other():
            return 1
    """

    def decorator(fn):
        fn.__unsafe__ = True
        return fn

    if func is None:
        return decorator
    if callable(func):
        return decorator(func)
    # A non-callable, non-None first arg means the user probably passed
    # a kwarg positionally (e.g. cache=True); tell them loudly.
    raise TypeError(f"@unsafe does not accept arguments; got {func!r}. The `cache=` parameter was removed — unsafe assets cache identically to pure assets.")


def is_unsafe(func):
    """Check whether a function is marked ``@unsafe``."""
    return getattr(func, "__unsafe__", False)
