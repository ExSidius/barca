"""CLI entry point for `python -m barca.worker`."""

from __future__ import annotations

import argparse


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--module", required=True)
    parser.add_argument("--function", required=True)
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--input-kwargs", default=None, help="JSON dict of input kwargs")
    args = parser.parse_args()

    from barca._barca import materialize_asset

    materialize_asset(args.module, args.function, args.output_dir, args.input_kwargs)


if __name__ == "__main__":
    main()
