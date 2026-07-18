"""Tests for the adaptive pull-queue executor pieces visible from Python.

Covers worker self-timing (CPU/wall/RSS riding on completion messages), the
tier-1 artifact LRU, and the persisted cost estimates that seed cross-run
batch sizing.
"""

import contextlib
import shutil
import sqlite3
import textwrap
from pathlib import Path

import pytest

import barca
from barca._worker import _ArtifactLRU, _peak_rss_bytes


@pytest.fixture(autouse=True)
def clean_barca_dir():
    barca_dir = Path(".barca")
    if barca_dir.exists():
        shutil.rmtree(barca_dir)
    yield
    if barca_dir.exists():
        shutil.rmtree(barca_dir)


def write_module(tmp_path, filename, code):
    p = tmp_path / filename
    p.write_text(textwrap.dedent(code))
    return str(p)


def _query(sql):
    """One-shot query with the connection closed afterwards — a lingering
    sqlite3 handle would hold a lock that blocks the next barca run."""
    with contextlib.closing(sqlite3.connect(".barca/metadata.db")) as conn:
        return conn.execute(sql).fetchall()


# ─── Unit: measurement helpers ───────────────────────────────────────────────


class TestPeakRss:
    def test_returns_positive_bytes(self):
        rss = _peak_rss_bytes()
        # A running CPython interpreter needs at least a few MB.
        assert rss > 1024 * 1024


class TestArtifactLRU:
    def test_miss_returns_none(self):
        lru = _ArtifactLRU()
        assert lru.get("/nowhere/x.json") is None

    def test_hit_returns_equal_value(self):
        lru = _ArtifactLRU()
        lru.put("/a.json", {"k": [1, 2, 3]})
        assert lru.get("/a.json") == {"k": [1, 2, 3]}

    def test_hit_is_isolated_from_mutation(self):
        # A task mutating its input must never poison a later task's view.
        lru = _ArtifactLRU()
        original = {"rows": [1, 2, 3]}
        lru.put("/a.json", original)
        first = lru.get("/a.json")
        first["rows"].append(999)
        assert lru.get("/a.json") == {"rows": [1, 2, 3]}

    def test_put_copies_value(self):
        lru = _ArtifactLRU()
        value = {"rows": [1]}
        lru.put("/a.json", value)
        value["rows"].append(2)  # caller mutates after put
        assert lru.get("/a.json") == {"rows": [1]}

    def test_eviction_drops_least_recent(self):
        lru = _ArtifactLRU(max_entries=2)
        lru.put("/a", 1)
        lru.put("/b", 2)
        assert lru.get("/a") == 1  # touch /a → /b is now least recent
        lru.put("/c", 3)
        assert lru.get("/b") is None
        assert lru.get("/a") == 1
        assert lru.get("/c") == 3

    def test_uncopyable_value_is_skipped_not_fatal(self):
        lru = _ArtifactLRU()

        class Uncopyable:
            def __deepcopy__(self, memo):
                raise RuntimeError("no copies")

        lru.put("/a", Uncopyable())
        # put failed silently → miss, caller falls through to the store.
        assert lru.get("/a") is None


# ─── Integration: timing + estimates persisted through a real run ───────────


class TestTimingPersisted:
    def test_cpu_and_rss_recorded_in_materializations(self, tmp_path):
        f = write_module(
            tmp_path,
            "m.py",
            """
            from barca import asset

            @asset()
            def busy():
                total = 0
                for i in range(200_000):
                    total += i * i
                return {"total": total}
        """,
        )
        barca.get(f)
        rows = _query(
            "SELECT cpu_seconds, max_rss_bytes, elapsed_seconds FROM materializations "
            "WHERE node_id LIKE '%:busy' AND status = 'success'"
        )
        assert rows
        cpu, rss, wall = rows[0]
        assert cpu is not None and cpu >= 0.0
        assert rss is not None and rss > 1024 * 1024
        assert wall is not None and wall > 0.0

    def test_cost_estimate_persisted_and_reused(self, tmp_path):
        f = write_module(
            tmp_path,
            "m.py",
            """
            import time
            from barca import asset

            @asset()
            def steady():
                time.sleep(0.05)
                return 1
        """,
        )
        barca.get(f)
        rows = _query(
            "SELECT estimate_seconds, samples FROM cost_estimates WHERE node_id LIKE '%:steady'"
        )
        assert rows
        estimate, samples = rows[0]
        assert samples >= 1
        # First observation IS the estimate; a 50ms sleep must land well
        # under the 30s cold default and above zero.
        assert 0.0 < estimate < 5.0

        # Second (uncached) run folds a new observation into the same row.
        barca.get(f, no_cache=True)
        rows2 = _query(
            "SELECT estimate_seconds, samples FROM cost_estimates WHERE node_id LIKE '%:steady'"
        )
        assert rows2
        assert rows2[0][1] >= samples + 1


class TestManyTinyPartitions:
    def test_fan_out_completes_correctly(self, tmp_path):
        """Batch-pulled tiny partitions must all materialize exactly once.

        (Fan-in via collect() is exercised in test_reliability.py — this
        guards the lease/batch machinery: no partition lost, none run twice.)
        """
        f = write_module(
            tmp_path,
            "m.py",
            """
            from barca import asset, partitions

            @asset(partitions={"i": partitions([str(n) for n in range(24)])})
            def shard(i):
                return {"i": int(i)}
        """,
        )
        barca.get(f)
        rows = _query(
            "SELECT COUNT(*), COUNT(DISTINCT node_id) FROM materializations "
            "WHERE node_id LIKE '%:shard[%' AND status = 'success'"
        )
        total_rows, distinct_nodes = rows[0]
        assert distinct_nodes == 24, "every partition must materialize"
        assert total_rows == 24, "no partition may run twice (duplicate lease)"

    def test_mixed_duration_partitions_complete_exactly_once(self, tmp_path):
        """Dynamic task sizing: partitions of one node with very different
        costs (1ms vs 60ms) must each materialize exactly once — batch pulls
        sized from early observations must never lose or double-run the
        heavy stragglers."""
        f = write_module(
            tmp_path,
            "m.py",
            """
            import time
            from barca import asset, partitions

            KEYS = [f"p{i:02d}" for i in range(12)]

            @asset(partitions={"key": partitions(KEYS)})
            def work(key):
                # Even-indexed partitions are ~1ms, odd are 60ms.
                time.sleep(0.001 if int(key[1:]) % 2 == 0 else 0.06)
                return {"key": key}
        """,
        )
        barca.get(f)
        rows = _query(
            "SELECT COUNT(*), COUNT(DISTINCT node_id) FROM materializations "
            "WHERE node_id LIKE '%:work[%' AND status = 'success'"
        )
        total, distinct = rows[0]
        assert distinct == 12, "every partition must materialize"
        assert total == 12, "no partition may run twice"

        # The estimator must reflect the cost split: heavy partitions carry
        # larger estimates than tiny ones.
        est = dict(
            _query(
                "SELECT node_id, estimate_seconds FROM cost_estimates WHERE node_id LIKE '%:work[%'"
            )
        )
        assert len(est) == 12
        heavy = [v for k, v in est.items() if int(k.split("p")[-1].rstrip("]")) % 2 == 1]
        tiny = [v for k, v in est.items() if int(k.split("p")[-1].rstrip("]")) % 2 == 0]
        assert min(heavy) > max(tiny), (
            f"heavy estimates {sorted(heavy)} must exceed tiny {sorted(tiny)}"
        )

    def test_second_run_seeds_estimates_for_partitions(self, tmp_path):
        f = write_module(
            tmp_path,
            "m.py",
            """
            from barca import asset, partitions

            @asset(partitions={"i": partitions([str(n) for n in range(8)])})
            def shard(i):
                return {"i": int(i)}
        """,
        )
        barca.get(f)
        count = _query("SELECT COUNT(*) FROM cost_estimates WHERE node_id LIKE '%:shard[%'")[0][0]
        assert count == 8
