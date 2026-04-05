"""Parse barca.toml configuration."""

from __future__ import annotations

import tomllib
from pathlib import Path


def load_config(repo_root: Path) -> dict:
    config_path = repo_root / "barca.toml"
    if config_path.exists():
        return tomllib.loads(config_path.read_text())
    return {}


def configured_modules(config: dict) -> list[str]:
    return config.get("project", {}).get("modules", [])
