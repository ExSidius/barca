"""Utility functions for cross-file dependency tests."""


def normalize(text):
    """Normalize text: lowercase and strip whitespace."""
    return text.lower().strip()


def format_output(data):
    """Format data for display."""
    return str(data).upper()
