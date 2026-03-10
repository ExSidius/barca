"""CLI entry point for barca."""

import argparse
import sys


def main():
    parser = argparse.ArgumentParser(
        prog="barca", description="Minimal asset orchestrator"
    )
    sub = parser.add_subparsers(dest="command")

    sub.add_parser("serve", help="Start the barca server (default)")

    reset_parser = sub.add_parser("reset", help="Remove generated files and caches")
    reset_parser.add_argument(
        "--db", action="store_true", help="Only remove the metadata database (.barca/)"
    )
    reset_parser.add_argument(
        "--artifacts",
        action="store_true",
        help="Only remove materialized artifacts (.barcafiles/)",
    )
    reset_parser.add_argument(
        "--tmp", action="store_true", help="Only remove temporary staging files (tmp/)"
    )

    args = parser.parse_args()

    if args.command == "reset":
        from barca._barca import run_reset

        output = run_reset(db=args.db, artifacts=args.artifacts, tmp=args.tmp)
        sys.stdout.write(output)
    else:
        from barca._barca import run_server

        run_server()


if __name__ == "__main__":
    main()
