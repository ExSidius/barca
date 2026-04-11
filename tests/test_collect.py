"""Tests for collect() — partition aggregation into dict[tuple, value].

Covers spec rule CollectPartitions:
- collect(asset_fn) returns a CollectInput marker
- Downstream receives dict[tuple[str, ...], OutputType]
- Partition keys are always tuples (even single-dimension)
- collect blocks the downstream entirely if any partition fails
- Once all partitions succeed, the downstream unblocks
"""

from __future__ import annotations

import sys
import textwrap

from barca._collect import CollectInput
from barca._store import MetadataStore


def _cleanup(prefix: str) -> None:
    to_remove = [k for k in sys.modules if k == prefix or k.startswith(prefix + ".")]
    for k in to_remove:
        del sys.modules[k]
    from barca._trace import clear_caches

    clear_caches()


# ---------------------------------------------------------------------------
# Marker identity
# ---------------------------------------------------------------------------


def test_collect_returns_collect_input_marker():
    """collect(fn) must return a CollectInput holding a reference to fn."""
    from barca import collect

    def dummy():
        return 1

    marker = collect(dummy)
    assert isinstance(marker, CollectInput)
    assert marker.upstream is dummy


def test_collect_is_importable_from_top_level():
    """collect should be accessible as `from barca import collect`."""
    import barca

    assert hasattr(barca, "collect")
    assert callable(barca.collect)


# ---------------------------------------------------------------------------
# Usage in downstream asset inputs
# ---------------------------------------------------------------------------


def test_collect_in_inputs_produces_dict(tmp_path):
    """A downstream asset consuming collect() should receive dict[tuple, value]."""
    project_dir = tmp_path / "collectproj"
    project_dir.mkdir()

    mod_dir = project_dir / "cmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, collect, partitions, Always

        @asset(partitions={"ticker": partitions(["AAPL", "MSFT", "GOOG"])}, freshness=Always())
        def prices(ticker: str):
            return {"ticker": ticker, "price": len(ticker) * 100}

        @asset(inputs={"all_prices": collect(prices)}, freshness=Always())
        def summary(all_prices):
            # all_prices must be dict[tuple[str, ...], dict]
            assert isinstance(all_prices, dict)
            for key, value in all_prices.items():
                assert isinstance(key, tuple)
                assert isinstance(value, dict)
            return {
                "count": len(all_prices),
                "tickers_seen": sorted(k[0] for k in all_prices.keys()),
            }
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["cmod.assets"]
        """)
    )

    _cleanup("cmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)
        assert result.failed == 0

        summary_id = store.asset_id_by_logical_name("cmod/assets.py:summary")
        detail = store.asset_detail(summary_id)
        mat = detail.latest_materialization
        assert mat is not None
        assert mat.status == "success"
        # Read the artifact back and assert the shape.
        import json

        data = json.loads((project_dir / mat.artifact_path).read_text())
        assert data["count"] == 3
        assert data["tickers_seen"] == ["AAPL", "GOOG", "MSFT"]
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("cmod")


def test_collect_tuple_keys_are_always_tuples(tmp_path):
    """Even a single-dimension partitioned upstream yields tuple keys."""
    project_dir = tmp_path / "tupkeys"
    project_dir.mkdir()

    mod_dir = project_dir / "tkmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, collect, partitions, Always

        @asset(partitions={"date": partitions(["2024-01"])}, freshness=Always())
        def sales(date: str):
            return {"date": date, "total": 100}

        @asset(inputs={"by_date": collect(sales)}, freshness=Always())
        def rollup(by_date):
            keys = list(by_date.keys())
            assert all(isinstance(k, tuple) for k in keys), "keys must be tuples"
            assert all(len(k) == 1 for k in keys), "single-dim keys are 1-tuples"
            return {"first_key": list(keys[0])}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["tkmod.assets"]
        """)
    )

    _cleanup("tkmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)
        assert result.failed == 0
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("tkmod")


def test_collect_multi_dimensional_keys(tmp_path):
    """Multi-dim partitions yield (dim1, dim2, ...) tuple keys."""
    project_dir = tmp_path / "multikeys"
    project_dir.mkdir()

    mod_dir = project_dir / "mkmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, collect, partitions, Always

        @asset(
            partitions={
                "date": partitions(["2024-01", "2024-02"]),
                "region": partitions(["US", "EU"]),
            },
            freshness=Always(),
        )
        def metrics(date: str, region: str):
            return {"date": date, "region": region}

        @asset(inputs={"all_metrics": collect(metrics)}, freshness=Always())
        def rollup(all_metrics):
            keys = list(all_metrics.keys())
            assert all(isinstance(k, tuple) for k in keys)
            assert all(len(k) == 2 for k in keys), "two-dim partitions → 2-tuples"
            return {"count": len(all_metrics)}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["mkmod.assets"]
        """)
    )

    _cleanup("mkmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)
        assert result.failed == 0

        rollup_id = store.asset_id_by_logical_name("mkmod/assets.py:rollup")
        detail = store.asset_detail(rollup_id)
        import json

        data = json.loads((project_dir / detail.latest_materialization.artifact_path).read_text())
        # 2 dates x 2 regions = 4
        assert data["count"] == 4
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("mkmod")


def test_collect_blocks_on_any_partition_failure(tmp_path):
    """If any partition of the upstream fails, the downstream does not run."""
    project_dir = tmp_path / "blockproj"
    project_dir.mkdir()

    mod_dir = project_dir / "bmod"
    mod_dir.mkdir()
    (mod_dir / "__init__.py").write_text("")
    (mod_dir / "assets.py").write_text(
        textwrap.dedent("""\
        from barca import asset, collect, partitions, Always

        @asset(partitions={"n": partitions(["1", "2", "3"])}, freshness=Always())
        def flaky(n: str):
            if n == "2":
                raise RuntimeError("partition 2 always fails")
            return {"n": n}

        @asset(inputs={"all_flaky": collect(flaky)}, freshness=Always())
        def collector(all_flaky):
            return {"count": len(all_flaky)}
        """)
    )
    (project_dir / "barca.toml").write_text(
        textwrap.dedent("""\
        [project]
        modules = ["bmod.assets"]
        """)
    )

    _cleanup("bmod")
    sys.path.insert(0, str(project_dir))
    try:
        from barca._run import run_pass

        store = MetadataStore(str(project_dir / ".barca" / "metadata.db"))
        result = run_pass(store, project_dir)

        # Collector must NOT have materialised — at least one partition failed.
        collector_id = store.asset_id_by_logical_name("bmod/assets.py:collector")
        detail = store.asset_detail(collector_id)
        assert detail.latest_materialization is None or detail.latest_materialization.status != "success"
        assert result.failed >= 1 or result.stale_blocked >= 1
    finally:
        sys.path.remove(str(project_dir))
        _cleanup("bmod")
