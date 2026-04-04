"""CLI entry point for `python -m barca.inspect`."""

from __future__ import annotations

import argparse


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--module", dest="modules", action="append", required=True)
    parser.add_argument("--project-root", dest="project_root", default=None)
    args = parser.parse_args()

    from barca._barca import inspect_modules

    result = inspect_modules(args.modules, args.project_root)
    print(result)


if __name__ == "__main__":
    main()
