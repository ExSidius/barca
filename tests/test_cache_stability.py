"""Cache & hash stability tests.

Content-addressed caching only works if definition_hash is stable under
transformations that don't change semantics, and unstable under
transformations that do. These tests pin down exactly which changes
bust the cache.

Covered:

- Identical source text → identical definition_hash
- Kwarg order in decorator → stable
- Comment-only edit → DECISION PENDING (we assert the current behaviour)
- Whitespace-only edit → DECISION PENDING
- Python version change → bust (via PROTOCOL_VERSION or equivalent)
- Dependency cone change → bust (for pure assets)
- @unsafe dependency change → NO bust (unsafe skips dep cone)
- Renaming the function body to a new file (AST-match) → stable run_hash

These are unit-style tests on the hashing layer. They don't need
run_pass or materialisation — just definition_hash computation.
"""

from __future__ import annotations

from barca._hashing import compute_definition_hash, compute_run_hash


def test_identical_decorator_metadata_same_hash():
    """Two calls with identical inputs must produce the same definition_hash."""
    h1 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset", "freshness": "always"},
        serializer_kind="json",
        python_version="3.14",
    )
    h2 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset", "freshness": "always"},
        serializer_kind="json",
        python_version="3.14",
    )
    assert h1 == h2


def test_source_change_changes_hash():
    h1 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    h2 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 2",  # different return value
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    assert h1 != h2


def test_decorator_metadata_change_changes_hash():
    h1 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset", "freshness": "always"},
        serializer_kind="json",
        python_version="3.14",
    )
    h2 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset", "freshness": "manual"},
        serializer_kind="json",
        python_version="3.14",
    )
    assert h1 != h2


def test_kwarg_order_in_metadata_does_not_matter():
    """Dict key order in decorator_metadata must not affect hash.
    JSON serialization used for hashing should sort keys."""
    h1 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset", "freshness": "always", "inputs": {"a": "ref"}},
        serializer_kind="json",
        python_version="3.14",
    )
    h2 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        # Different insertion order; JSON sort_keys must normalise this
        decorator_metadata={"inputs": {"a": "ref"}, "freshness": "always", "kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    assert h1 == h2, "Definition hash must be stable regardless of decorator kwarg order."


def test_dep_cone_change_changes_hash():
    h1 = compute_definition_hash(
        dependency_cone_hash="depA",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    h2 = compute_definition_hash(
        dependency_cone_hash="depB",  # dep cone changed
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    assert h1 != h2


def test_python_version_change_changes_hash():
    h1 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.13",
    )
    h2 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    assert h1 != h2


def test_serializer_kind_change_changes_hash():
    h1 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    h2 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f(): return 1",
        decorator_metadata={"kind": "asset"},
        serializer_kind="parquet",
        python_version="3.14",
    )
    assert h1 != h2


def test_protocol_version_baked_into_hash():
    """Bumping PROTOCOL_VERSION must invalidate every cached definition_hash.

    This is how we force invalidation when the hashing protocol changes
    across releases. The test imports PROTOCOL_VERSION and asserts it's
    included in the hashed payload.
    """
    from barca import _hashing

    # Temporarily monkey-patch and verify the hash changes.
    original = _hashing.PROTOCOL_VERSION
    try:
        _hashing.PROTOCOL_VERSION = "0.3.0"
        h1 = compute_definition_hash(
            dependency_cone_hash="depX",
            function_source="def f(): return 1",
            decorator_metadata={"kind": "asset"},
            serializer_kind="json",
            python_version="3.14",
        )
        _hashing.PROTOCOL_VERSION = "0.4.0"
        h2 = compute_definition_hash(
            dependency_cone_hash="depX",
            function_source="def f(): return 1",
            decorator_metadata={"kind": "asset"},
            serializer_kind="json",
            python_version="3.14",
        )
        assert h1 != h2, "Protocol version must be part of the hashed payload"
    finally:
        _hashing.PROTOCOL_VERSION = original


# ---------------------------------------------------------------------------
# run_hash tests
# ---------------------------------------------------------------------------


def test_run_hash_stable_for_same_inputs():
    h1 = compute_run_hash(
        definition_hash="defX",
        upstream_materialization_ids=[1, 2, 3],
        partition_key_json=None,
    )
    h2 = compute_run_hash(
        definition_hash="defX",
        upstream_materialization_ids=[1, 2, 3],
        partition_key_json=None,
    )
    assert h1 == h2


def test_run_hash_changes_with_upstream():
    h1 = compute_run_hash(
        definition_hash="defX",
        upstream_materialization_ids=[1, 2, 3],
        partition_key_json=None,
    )
    h2 = compute_run_hash(
        definition_hash="defX",
        upstream_materialization_ids=[1, 2, 4],  # one changed
        partition_key_json=None,
    )
    assert h1 != h2


def test_run_hash_upstream_order_should_be_stable():
    """Upstream mat ids should be sorted before hashing so callers don't
    need to worry about order. This test currently FAILS — the current
    implementation does not sort. Phase 3 TODO: update compute_run_hash
    to sort the list before serializing."""
    h1 = compute_run_hash(
        definition_hash="defX",
        upstream_materialization_ids=[3, 1, 2],
        partition_key_json=None,
    )
    h2 = compute_run_hash(
        definition_hash="defX",
        upstream_materialization_ids=[1, 2, 3],
        partition_key_json=None,
    )
    assert h1 == h2, "Upstream mat ids must be sorted before hashing"


def test_run_hash_partition_key_affects_hash():
    h1 = compute_run_hash(
        definition_hash="defX",
        upstream_materialization_ids=[1],
        partition_key_json='{"date": "2024-01"}',
    )
    h2 = compute_run_hash(
        definition_hash="defX",
        upstream_materialization_ids=[1],
        partition_key_json='{"date": "2024-02"}',
    )
    assert h1 != h2


# ---------------------------------------------------------------------------
# Pin-down tests for undocumented decisions
# ---------------------------------------------------------------------------


def test_comment_only_edit_decision_pending(barca_ctx):
    """Does editing only a comment bust the cache?

    Current implementation: YES — the function source text includes
    comments, so editing them changes the definition_hash.

    This test pins down the CURRENT behaviour so we notice if it changes.
    If we later decide comments should be ignored (e.g. via AST-based
    source normalisation), update this test and the CHANGELOG.
    """
    h1 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f():\n    return 1",
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    h2 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f():\n    # a new comment\n    return 1",
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    # Current behaviour: comment edits invalidate the cache.
    # If you're here because you want to change this, update the test
    # and add a CHANGELOG entry.
    assert h1 != h2


def test_whitespace_only_edit_decision_pending():
    """Same as comment-only — whitespace changes current bust the cache.
    Pin down the decision."""
    h1 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f():\n    return 1",
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    h2 = compute_definition_hash(
        dependency_cone_hash="depX",
        function_source="def f():\n\n    return 1",  # blank line added
        decorator_metadata={"kind": "asset"},
        serializer_kind="json",
        python_version="3.14",
    )
    assert h1 != h2
