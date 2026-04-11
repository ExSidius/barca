"""Parametrized YAML-driven scenario tests.

Each file under ``tests/scenarios/*.yaml`` becomes a pytest case. This file
has no test logic itself — it's just a loader that hands scenarios to the
runner in ``_scenarios.py``.

To add a new scenario: drop a new YAML file into ``tests/scenarios/``. No
Python changes required. See ``tests/_scenarios.py`` for the schema and
supported actions.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from tests._scenarios import Scenario, load_scenarios, run_scenario

SCENARIOS_DIR = Path(__file__).parent / "scenarios"
SCENARIOS: list[Scenario] = load_scenarios(SCENARIOS_DIR)


@pytest.mark.parametrize(
    "scenario",
    SCENARIOS,
    ids=[s.id for s in SCENARIOS],
)
def test_scenario(scenario: Scenario, barca_ctx) -> None:
    run_scenario(scenario, barca_ctx)
