"""Summarize trigger-latency samples for the scheduler-overhead benchmark.

Reads a results file of unix timestamps (one per line — the wall-clock time a
scheduled task actually executed), and for a `* * * * *` (top-of-minute) schedule
reports each fire's lateness = seconds past its minute boundary. Emits JSON with
count + min/median/p95/max.

Usage: latency_stats.py <results_file> [label]
"""

import json
import math
import sys


def main() -> None:
    path = sys.argv[1]
    label = sys.argv[2] if len(sys.argv) > 2 else ""
    try:
        with open(path) as f:
            stamps = [float(line) for line in f if line.strip()]
    except FileNotFoundError:
        stamps = []

    # Lateness relative to the minute boundary each fire was due at.
    lateness = sorted(t - 60 * math.floor(t / 60) for t in stamps)

    def pct(p: float):
        if not lateness:
            return None
        i = min(len(lateness) - 1, int(round(p * (len(lateness) - 1))))
        return round(lateness[i], 3)

    out = {
        "label": label,
        "fires": len(lateness),
        "min_s": round(lateness[0], 3) if lateness else None,
        "median_s": pct(0.5),
        "p95_s": pct(0.95),
        "max_s": round(lateness[-1], 3) if lateness else None,
        "note": "seconds past the minute boundary; >55s may be a rolled/missed tick",
    }
    print(json.dumps(out))


if __name__ == "__main__":
    main()
