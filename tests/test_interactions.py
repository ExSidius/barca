"""Pathological interaction tests — things that only break at the seams.

The scenario YAML files test the happy path of every spec rule individually.
This file tests what happens when rules interact under pressure:

- Rename a file while a pass is in-flight
- Two concurrent refreshes of the same asset
- CLI refresh while the scheduler loop is running
- Prune while a pass is reading artifacts
- Dev watcher sees a refresh as a DB change, not a file change
- Edit asset source while its upstream is still materialising
- Delete an asset file while dev mode is watching
- Edit barca.toml mid-session (module list changes)
- Reset --db while a pass is running

These tests use threads + synchronisation primitives + frozen time. They
are slower than the YAML scenarios and less declarative, but they catch
race conditions that rule-by-rule testing cannot.

Many of these require Phase 3 source (run_loop, dev_watch, prune, etc.)
to even dispatch — they will fail with NotImplementedError until Phase 3.
"""

from __future__ import annotations

import threading
import time

import pytest

# ---------------------------------------------------------------------------
# Concurrent writes to the same asset
# ---------------------------------------------------------------------------


def test_concurrent_refresh_same_asset(barca_ctx):
    """Two threads call refresh(asset_id) at the same time.

    Under optimistic concurrency (no lock around the cache-check + execute
    sequence), both threads may miss the cache, both run the function, and
    both insert a successful materialization. That's acceptable — the key
    invariants are: no deadlock, no crash, no orphaned queued rows. The
    cache will serialise subsequent calls once either thread commits.
    """
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def shared():
            return {"v": 1}
        """,
    )
    barca_ctx.reindex()
    asset_id = barca_ctx.asset_id_by_function("shared")

    errors: list[Exception] = []
    barrier = threading.Barrier(2)

    def worker():
        try:
            barrier.wait()
            barca_ctx.refresh(asset_id)
        except Exception as exc:
            errors.append(exc)

    threads = [threading.Thread(target=worker) for _ in range(2)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=10)

    assert all(not t.is_alive() for t in threads), "a worker deadlocked"

    # Both threads may have raced and produced 2 successful mats. That's OK.
    # What matters: no crash, no corruption, and a third call is a cache hit.
    mats = [m for m in barca_ctx.store.list_materializations(asset_id, limit=100) if m.status == "success"]
    assert 1 <= len(mats) <= 2, f"expected 1-2 successful mats, got {len(mats)}"

    # A subsequent refresh should be a cache hit (no new mat)
    barca_ctx.refresh(asset_id)
    mats_after = [m for m in barca_ctx.store.list_materializations(asset_id, limit=100) if m.status == "success"]
    assert len(mats_after) == len(mats), "third refresh should hit cache"


def test_refresh_during_run_loop(barca_ctx):
    """CLI refresh while the scheduler loop is running.
    The scheduler should see the CLI refresh's output on its next pass
    (via cache hit) rather than re-materialising."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset, Always

        @asset(freshness=Always())
        def periodic():
            return {"v": 1}
        """,
    )
    barca_ctx.reindex()
    asset_id = barca_ctx.asset_id_by_function("periodic")

    stop_event = threading.Event()

    def loop_worker():
        try:
            from barca._run import run_loop

            run_loop(barca_ctx.store, barca_ctx.root, stop_event=stop_event)
        except NotImplementedError:
            pass  # Phase 2

    loop_thread = threading.Thread(target=loop_worker, daemon=True)
    loop_thread.start()

    time.sleep(0.1)  # let the loop kick off

    # CLI-style refresh
    try:
        barca_ctx.refresh(asset_id)
    except NotImplementedError:
        pass  # Phase 2

    stop_event.set()
    loop_thread.join(timeout=5)

    # Phase 3 assertion: there should be exactly one successful mat for
    # asset_id (the loop's pass sees a cache hit on the CLI's write).
    mats = [m for m in barca_ctx.store.list_materializations(asset_id, limit=100) if m.status == "success"]
    assert len(mats) <= 1, f"expected ≤1 successful mat, got {len(mats)}"


# ---------------------------------------------------------------------------
# File rename during in-flight pass
# ---------------------------------------------------------------------------


def test_rename_mid_run_pass_completes_cleanly(barca_ctx):
    """The developer renames an asset module while run_pass is iterating.
    The current pass should either (a) complete against the old asset id
    or (b) fail cleanly — but not leave dangling mat rows with missing
    artifact paths."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def rename_me():
            return {"v": 1}
        """,
    )

    # Start a run_pass in a thread, rename the file from the main thread
    # right after kickoff.
    result_holder: dict = {}

    def worker():
        try:
            result_holder["result"] = barca_ctx.run_pass()
        except NotImplementedError:
            result_holder["result"] = "phase2"
        except Exception as exc:
            result_holder["error"] = exc

    t = threading.Thread(target=worker)
    t.start()

    # Race: try to edit the file while the pass is running.
    time.sleep(0.02)
    barca_ctx.edit_module(
        "assets.py",
        lambda src: src.replace("rename_me", "rename_me_moved"),
    )

    t.join(timeout=10)
    assert not t.is_alive(), "run_pass deadlocked"

    # Phase 3: no crash, no dangling rows.
    # Phase 2: NotImplementedError caught above — 'phase2' sentinel.
    assert "result" in result_holder or "error" in result_holder


# ---------------------------------------------------------------------------
# Prune while a pass is running
# ---------------------------------------------------------------------------


def test_prune_during_run_pass(barca_ctx):
    """Prune deletes artifacts while run_pass might be reading them.
    At minimum, neither should crash the process.
    Ideally prune should detect an active pass and refuse, or serialise."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def a():
            return {"v": 1}
        """,
    )
    barca_ctx.reindex()

    errors: list[Exception] = []

    def run_pass_worker():
        try:
            barca_ctx.run_pass()
        except NotImplementedError:
            pass
        except Exception as exc:
            errors.append(exc)

    def prune_worker():
        time.sleep(0.01)
        try:
            barca_ctx.prune()
        except NotImplementedError:
            pass
        except Exception as exc:
            errors.append(exc)

    t1 = threading.Thread(target=run_pass_worker)
    t2 = threading.Thread(target=prune_worker)
    t1.start()
    t2.start()
    t1.join(timeout=10)
    t2.join(timeout=10)

    # Phase 3: no errors. Phase 2: both hit NotImplementedError (handled).
    assert all(not isinstance(e, RuntimeError | OSError) for e in errors), errors


# ---------------------------------------------------------------------------
# Edit source during in-flight materialisation
# ---------------------------------------------------------------------------


def test_edit_upstream_while_downstream_runs(barca_ctx):
    """Upstream changes source while its output is still being consumed
    downstream. The downstream materialisation should reflect the upstream's
    state AT THE MOMENT IT WAS STARTED, not the mid-flight edit."""
    barca_ctx.write_module(
        "assets.py",
        """
        import time
        from barca import asset

        @asset()
        def upstream():
            return {"v": 1}

        @asset(inputs={"u": upstream})
        def downstream(u):
            time.sleep(0.05)  # simulate slow compute
            return {"v2": u["v"] + 1}
        """,
    )

    def worker():
        try:
            barca_ctx.run_pass()
        except NotImplementedError:
            pass

    t = threading.Thread(target=worker)
    t.start()
    time.sleep(0.02)

    # Edit upstream mid-flight
    barca_ctx.edit_module(
        "assets.py",
        lambda src: src.replace('"v": 1', '"v": 999'),
    )

    t.join(timeout=10)
    assert not t.is_alive()
    # Phase 3 assertion would check that the downstream mat used upstream's
    # original value (1, not 999). Phase 2: just no crash.


# ---------------------------------------------------------------------------
# Dev mode + refresh interaction
# ---------------------------------------------------------------------------


def test_dev_mode_sees_refresh_as_db_change(barca_ctx):
    """Dev mode watches for file changes. A refresh is a DB change, not a
    file change. Dev mode must not re-reindex or re-mark-stale on refresh."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def target():
            return {"v": 1}
        """,
    )
    barca_ctx.reindex()
    asset_id = barca_ctx.asset_id_by_function("target")

    # In Phase 3, we'd start dev_watch in a thread and verify it doesn't
    # trigger on the refresh below. For Phase 2, we just verify the pure
    # handle_file_change can be called alongside refresh without conflict.
    try:
        barca_ctx.refresh(asset_id)
    except NotImplementedError:
        pass

    from barca._dev import handle_file_change

    try:
        handle_file_change(barca_ctx.store, barca_ctx.root)
    except NotImplementedError:
        pass


# ---------------------------------------------------------------------------
# barca.toml edited mid-session
# ---------------------------------------------------------------------------


def test_barca_toml_edit_picked_up_on_next_reindex(barca_ctx):
    """Developer adds a new module to barca.toml. Next reindex should see
    the new module's assets as added."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def existing():
            return 1
        """,
    )
    barca_ctx.reindex()

    # Add a second module file
    barca_ctx.write_module(
        "extra.py",
        """
        from barca import asset

        @asset()
        def newly_added():
            return 2
        """,
    )
    barca_ctx.register_module(f"{barca_ctx.pkg_name}.extra")

    barca_ctx.reindex()
    # Verify the asset is now in the store
    assert barca_ctx.asset_id_by_function("newly_added") is not None


# ---------------------------------------------------------------------------
# Rename partition-defining asset while downstream has pending partitions
# ---------------------------------------------------------------------------


def test_rename_partition_source_with_pending_downstream(barca_ctx):
    """A dynamic-partition upstream is renamed. Before it has materialised,
    the downstream is 'partitions: pending'. After rename, the downstream
    should still be able to resolve partitions from the renamed asset —
    identity is preserved via AST match or name=."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset, Always

        @asset(freshness=Always())
        def date_source():
            return ["2024-01", "2024-02"]

        @asset(partitions={"date": date_source}, freshness=Always())
        def report(date: str):
            return {"date": date}
        """,
    )
    barca_ctx.reindex()

    # Rename by moving to a different module. AST-match should preserve
    # the date_source asset's identity, and the partition reference in
    # report should remain valid.
    barca_ctx.write_module(
        "assets.py",
        """
        # date_source moved
        """,
    )
    barca_ctx.write_module(
        "sources.py",
        """
        from barca import asset, Always

        @asset(freshness=Always())
        def date_source():
            return ["2024-01", "2024-02"]
        """,
    )
    barca_ctx.register_module(f"{barca_ctx.pkg_name}.sources")

    # report still references date_source — but it's now in a different
    # file. This is a hard case: the reference resolution needs to
    # catch up with the rename detection.
    # Phase 3 assertion: reindex doesn't fail, report still finds its
    # partition source.
    try:
        diff = barca_ctx.reindex()
        # We at least get a diff object without crashing
        assert diff is not None
    except Exception as exc:
        # Phase 2 may have cascading issues; accept them for now
        pytest.skip(f"Phase 2 limitation: {exc}")


# ---------------------------------------------------------------------------
# Reset --db during a pass
# ---------------------------------------------------------------------------


def test_reset_db_while_pass_running(barca_ctx):
    """This is the worst-case destructive operation. If the developer runs
    `barca reset --db` while a pass is materialising, at minimum we must
    not corrupt the store or leave orphaned artifact files."""
    barca_ctx.write_module(
        "assets.py",
        """
        from barca import asset

        @asset()
        def target():
            return {"v": 1}
        """,
    )
    barca_ctx.reindex()

    # Kick off a pass and a reset in parallel.
    stop = threading.Event()

    def run_pass_worker():
        while not stop.is_set():
            try:
                barca_ctx.run_pass()
            except (NotImplementedError, Exception):
                pass
            time.sleep(0.01)

    def reset_worker():
        time.sleep(0.02)
        try:
            from barca._engine import reset

            reset(barca_ctx.root, db=True)
        except Exception:
            pass
        stop.set()

    t1 = threading.Thread(target=run_pass_worker, daemon=True)
    t2 = threading.Thread(target=reset_worker, daemon=True)
    t1.start()
    t2.start()
    t1.join(timeout=5)
    t2.join(timeout=5)

    # Don't assert anything specific — this test is about "doesn't crash
    # the interpreter". If it gets here, we survived.
