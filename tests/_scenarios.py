"""YAML-driven scenario tests.

A scenario is a declarative description of a small Barca project plus a
sequence of actions and expected outcomes. Scenarios live in
``tests/scenarios/*.yaml`` and are loaded by ``conftest.py`` via
``load_scenarios()``, which parametrizes ``test_scenario()`` in
``test_scenarios.py``.

This is Barca's equivalent of uv's ``packse``-generated scenario tests,
but simpler: we don't need a wheel generator — the scenarios produce
Python modules and exercise them via the normal ``BarcaTestContext``.

Schema
------

.. code-block:: yaml

    name: manual_blocks_downstream
    description: A stale manual upstream blocks an Always downstream.

    # Python modules to write under the test package. Keys are relative
    # paths inside the package (e.g. "assets.py"), values are the source
    # (automatically dedented).
    modules:
      assets.py: |
        from barca import asset, Manual, Always

        @asset(freshness=Manual())
        def upstream():
            return 1

        @asset(inputs={"u": upstream}, freshness=Always())
        def downstream(u):
            return u + 1

    # Ordered list of steps. Each step runs an action and optionally
    # asserts expectations about the outcome or the post-state.
    steps:
      - action: reindex
        # No expectations — just sets up state.

      - action: run_pass
        expect:
          executed_assets: 0
          manual_skipped: 1
          stale_blocked: 1

      - action: refresh
        target: upstream
        # Implicit: no assertions

      - action: run_pass
        expect:
          executed_assets: 1

Supported actions
-----------------

- ``reindex``                — call ``ctx.reindex()``
- ``run_pass``               — call ``ctx.run_pass()``
- ``refresh``                — call ``ctx.refresh(ctx.asset_id_by_function(target))``
                               (optionally with ``stale_policy: error|warn|pass``)
- ``trigger_sensor``         — call ``ctx.trigger_sensor(ctx.asset_id_by_function(target))``
- ``prune``                  — call ``ctx.prune()``
- ``edit_module``            — replace the content of ``target`` (module path) with ``source``
- ``delete_module``          — delete a module from the package directory
- ``advance_time``           — call ``ctx.advance_time(seconds)``
- ``assert_file_exists``     — assert a file exists at ``target`` (relative to project root)
- ``assert_file_missing``    — assert a file does NOT exist at ``target``
- ``assert_file_contains``   — assert the file at ``target`` contains ``source`` substring
- ``assert_asset_status``    — assert the asset named ``target`` has status ``source``
                               (matches against materialization_status from list_assets)

Supported expectations
----------------------

- ``field: value``           — ``result.field == value``
- ``executed_assets: 1``     — shorthand for the common result fields
- ``raises: TypeError``      — the step should raise this exception
- ``assets_snapshot: |``     — full assets_snapshot() should equal this text
- ``diff: |``                — diff between the last action and this action

Scenarios that assert on features not yet implemented (``run_pass``,
``prune``, ``freshness``, etc.) will naturally fail during Phase 2 and
pass after Phase 3. That's the point.
"""

from __future__ import annotations

import textwrap
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml

# ---------------------------------------------------------------------------
# Dataclasses
# ---------------------------------------------------------------------------


@dataclass
class Step:
    action: str
    target: str | None = None
    source: str | None = None
    seconds: int | None = None
    stale_policy: str = "error"
    expect: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_yaml(cls, data: dict[str, Any]) -> Step:
        return cls(
            action=data["action"],
            target=data.get("target"),
            source=data.get("source"),
            seconds=data.get("seconds"),
            stale_policy=data.get("stale_policy", "error"),
            expect=data.get("expect", {}),
        )


@dataclass
class Scenario:
    name: str
    description: str
    modules: dict[str, str]
    steps: list[Step]
    source_file: Path

    @property
    def id(self) -> str:
        return f"{self.source_file.stem}::{self.name}"


# ---------------------------------------------------------------------------
# Loader
# ---------------------------------------------------------------------------


def load_scenarios(scenarios_dir: Path) -> list[Scenario]:
    """Load every .yaml file under ``scenarios_dir`` as a Scenario."""
    scenarios: list[Scenario] = []
    if not scenarios_dir.exists():
        return scenarios
    for yaml_path in sorted(scenarios_dir.glob("*.yaml")):
        with yaml_path.open() as f:
            data = yaml.safe_load(f)
        scenarios.append(
            Scenario(
                name=data.get("name", yaml_path.stem),
                description=data.get("description", ""),
                modules={rel: textwrap.dedent(src) for rel, src in data.get("modules", {}).items()},
                steps=[Step.from_yaml(s) for s in data.get("steps", [])],
                source_file=yaml_path,
            )
        )
    return scenarios


# ---------------------------------------------------------------------------
# Runner
# ---------------------------------------------------------------------------


def run_scenario(scenario: Scenario, ctx) -> None:
    """Execute a scenario against a BarcaTestContext.

    Raises AssertionError on any expectation mismatch. Any step with
    ``expect.raises`` is wrapped in a try/except that validates the
    exception type.
    """
    # 1. Write all modules AND auto-register any non-default modules
    # The default context registers `{pkg}.assets`; scenarios may write
    # other .py files that need to be added to barca.toml.
    for rel, src in scenario.modules.items():
        ctx.write_module(rel, src)  # bumps mtime via context
        if rel.endswith(".py") and rel != "assets.py" and rel != "__init__.py":
            # Convert "pipeline_a.py" → "{pkg}.pipeline_a"
            module_stem = rel[: -len(".py")].replace("/", ".")
            dotted = f"{ctx.pkg_name}.{module_stem}"
            ctx.register_module(dotted)
    # Also bump mtime on any __init__.py files to avoid stale package state
    import time as _time

    future = _time.time() + 2
    for init_py in ctx.pkg_dir.rglob("__init__.py"):
        try:
            import os as _os

            _os.utime(init_py, (future, future))
        except OSError:
            pass

    # 2. Execute steps in order
    last_result: Any = None
    for i, step in enumerate(scenario.steps):
        step_id = f"{scenario.id} step[{i}] action={step.action}"
        try:
            last_result = _run_step(step, ctx, last_result)
        except Exception as exc:
            expected_exc = step.expect.get("raises")
            if expected_exc is not None:
                # Check exception type by name (supports "TypeError", "ValueError", etc.)
                assert type(exc).__name__ == expected_exc, f"{step_id}: expected {expected_exc}, got {type(exc).__name__}: {exc}"
                continue
            raise AssertionError(f"{step_id}: unexpected exception: {exc}") from exc

        # If the step expected an exception but none was raised, that's a failure
        if step.expect.get("raises"):
            raise AssertionError(f"{step_id}: expected {step.expect['raises']} but step completed successfully")

        # 3. Check other expectations against the result
        _check_expectations(step, step_id, ctx, last_result)


def _run_step(step: Step, ctx, last_result: Any) -> Any:
    """Dispatch a step to the appropriate context method. Returns the action's result."""
    if step.action == "reindex":
        return ctx.reindex()
    if step.action == "run_pass":
        return ctx.run_pass()
    if step.action == "refresh":
        if step.target is None:
            raise ValueError("refresh requires a 'target' field")
        asset_id = ctx.asset_id_by_function(step.target)
        if asset_id is None:
            raise LookupError(f"refresh target {step.target!r} not found in store")
        return ctx.refresh(asset_id, stale_policy=step.stale_policy)
    if step.action == "trigger_sensor":
        if step.target is None:
            raise ValueError("trigger_sensor requires a 'target' field")
        asset_id = ctx.asset_id_by_function(step.target)
        if asset_id is None:
            raise LookupError(f"trigger_sensor target {step.target!r} not found in store")
        return ctx.trigger_sensor(asset_id)
    if step.action == "prune":
        return ctx.prune()
    if step.action == "edit_module":
        if step.target is None or step.source is None:
            raise ValueError("edit_module requires 'target' and 'source' fields")
        ctx.write_module(step.target, step.source)
        return None
    if step.action == "delete_module":
        if step.target is None:
            raise ValueError("delete_module requires a 'target' field")
        target_path = ctx.pkg_dir / step.target
        if target_path.exists():
            target_path.unlink()
        return None
    if step.action == "advance_time":
        if step.seconds is None:
            raise ValueError("advance_time requires a 'seconds' field")
        ctx.advance_time(step.seconds)
        return None
    if step.action == "assert_file_exists":
        if step.target is None:
            raise ValueError("assert_file_exists requires a 'target' field")
        target_path = ctx.root / step.target
        assert target_path.exists(), f"expected file to exist: {step.target}"
        return None
    if step.action == "assert_file_missing":
        if step.target is None:
            raise ValueError("assert_file_missing requires a 'target' field")
        target_path = ctx.root / step.target
        assert not target_path.exists(), f"expected file to NOT exist: {step.target}"
        return None
    if step.action == "assert_file_contains":
        if step.target is None or step.source is None:
            raise ValueError("assert_file_contains requires 'target' and 'source'")
        target_path = ctx.root / step.target
        assert target_path.exists(), f"expected file to exist: {step.target}"
        content = target_path.read_text()
        assert step.source in content, f"expected {step.source!r} in {step.target}; got:\n{content}"
        return None
    if step.action == "assert_asset_status":
        if step.target is None:
            raise ValueError("assert_asset_status requires 'target'")
        # target = function name, source = expected status ('success', 'failed', or None)
        expected = step.source  # may be None
        assets = ctx.store.list_assets()
        matches = [a for a in assets if a.function_name == step.target]
        assert matches, f"no asset with function_name={step.target!r}"
        row = matches[0]

        # Pick the right status source by kind
        if row.kind == "sensor":
            obs = ctx.store.latest_sensor_observation(row.asset_id)
            if obs is None:
                actual = None
            else:
                # Sensor status: 'success' if update_detected OR stored successfully,
                # 'failed' if the output_json contains an error marker from trigger_sensor
                try:
                    import json as _json

                    payload = _json.loads(obs.output_json) if obs.output_json else None
                except Exception:
                    payload = None
                if isinstance(payload, dict) and "error" in payload:
                    actual = "failed"
                else:
                    actual = "success"
        elif row.kind == "effect":
            ex = ctx.store.latest_effect_execution(row.asset_id)
            actual = ex.status if ex else None
        else:
            actual = getattr(row, "materialization_status", None)

        assert actual == expected, f"asset {step.target} status: expected {expected!r}, got {actual!r}"
        return None
    raise ValueError(f"unknown scenario action: {step.action}")


def _check_expectations(step: Step, step_id: str, ctx, result: Any) -> None:
    """Compare ``step.expect`` to the result and post-state."""
    for key, expected in step.expect.items():
        if key in ("raises",):
            continue
        if key == "assets_snapshot":
            actual = ctx.assets_snapshot()
            expected_text = textwrap.dedent(expected)
            assert actual == expected_text, f"{step_id}: assets_snapshot mismatch\n  expected:\n{expected_text}\n  actual:\n{actual}"
            continue
        if key == "added":
            # Works when reindex returns ReindexDiff; works when it returns a list
            added_names = _extract_names(result, "added")
            _match_set(expected, added_names, step_id, "added")
            continue
        if key == "removed":
            removed_names = _extract_names(result, "removed")
            _match_set(expected, removed_names, step_id, "removed")
            continue
        if key == "renamed":
            # Expect list of (old, new) pairs; actual is list of tuples
            renamed = getattr(result, "renamed", [])
            assert len(renamed) == len(expected), f"{step_id}: expected {len(expected)} renames, got {len(renamed)}"
            continue
        # Generic field access on result (for RunPassResult.executed_assets, etc.)
        actual = getattr(result, key, None)
        assert actual == expected, f"{step_id}: expected {key}={expected}, got {actual}"


def _extract_names(result: Any, kind: str) -> list[str]:
    """Pull a list of names from a ReindexDiff-shaped object, or from a list of AssetSummary."""
    if hasattr(result, kind):
        return list(getattr(result, kind))
    # Fallback: ``reindex`` currently returns list[AssetSummary]. If the caller
    # asked for 'added', treat every asset as 'added'. This is a shim for Phase 2.
    if kind == "added":
        return [getattr(a, "function_name", "") for a in result]
    return []


def _match_set(expected: Any, actual: list[str], step_id: str, label: str) -> None:
    """Match expected names against actual, supporting substring matching.

    Expected can be a list of strings. Each string must appear as a substring
    in at least one entry of actual. This makes it robust to Barca's
    logical_name format (``{package}/{file}.py:{function}``).
    """
    expected_list = list(expected) if isinstance(expected, list | tuple) else [expected]
    for needle in expected_list:
        assert any(needle in item for item in actual), f"{step_id}: expected {label} to contain {needle!r}, got {actual}"
