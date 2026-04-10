"""@unsafe decorator — marks functions as having untraceable behavior.

Inspired by Rust's `unsafe` blocks. Functions decorated with @unsafe:
- Have their dependency hash = hash of source only (no transitive tracing)
- Are always considered stale (re-materialized on every run)
- Silence dependency tracing warnings
- Propagate "always stale" to downstream dependents
"""


def unsafe(func=None, *, cache=False):
    """Mark a function as having untraceable side effects.

    @unsafe
    def load_config():
        return yaml.safe_load(open("config.yaml"))

    @unsafe(cache=True)  # opt into caching at your own risk
    def load_static_config():
        return yaml.safe_load(open("static.yaml"))
    """

    def decorator(fn):
        fn.__unsafe__ = True
        fn.__unsafe_cache__ = cache
        return fn

    if func is not None:
        return decorator(func)
    return decorator


def is_unsafe(func):
    """Check if a function is marked @unsafe."""
    return getattr(func, "__unsafe__", False)


def is_unsafe_cacheable(func):
    """Check if an @unsafe function opted into caching."""
    return getattr(func, "__unsafe_cache__", False)
