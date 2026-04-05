"""Cross-file helpers for dependency tracing tests."""


def normalize(text):
    """Strip and lowercase text."""
    return text.strip().lower()


def format_output(value, prefix="result"):
    """Format a value with a prefix."""
    return f"{prefix}: {value}"
