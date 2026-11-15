# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Turn `difftest bench` JSON reports into the rows BENCH.md asks for.

One invocation of `difftest bench` measures one scenario against one target
configuration and writes a JSON report plus a Markdown table. BENCH.md wants a
single table per phase whose rows are:

    date, commit, scenario, target, workers/threads, concurrency,
    rps, p50, p95, p99, idle RSS, peak RSS, CPU s

This script reads any number of reports and prints exactly those rows from the
**median** block of each (three repetitions), plus a headroom table and the load
averages that say how quiet the machine was. Copy the output into BENCH.md; do
not retype numbers.

Usage (from apps/server):

    tools/bench/summarize.py target/bench/phase1-*/*.json
    tools/bench/summarize.py --commit 13cbdbeb4e target/bench/phase1-*/*.json
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path


def worker_label(target: dict) -> str:
    if target.get("workers") is not None:
        return f"workers={target['workers']}"
    if target.get("threads") is not None:
        return f"threads={target['threads']}"
    return "default"


def mib(value: int | float) -> str:
    return f"{value / (1024 * 1024):.1f} MiB"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reports", nargs="+", type=Path)
    parser.add_argument(
        "--commit",
        default=None,
        help="commit column (default: `git rev-parse --short HEAD` in the repository)",
    )
    parser.add_argument("--date", default=None, help="date column (default: report date)")
    args = parser.parse_args()

    commit = args.commit
    if commit is None:
        try:
            commit = subprocess.run(
                ["git", "rev-parse", "--short", "HEAD"],
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip()
        except (OSError, subprocess.CalledProcessError):
            commit = "unknown"

    reports = []
    for path in args.reports:
        try:
            reports.append((path, json.loads(path.read_text())))
        except (OSError, json.JSONDecodeError) as error:
            print(f"error: {path}: {error}", file=sys.stderr)
            return 1

    print("| date | commit | scenario | target | workers/threads | conc | rps | p50 ms | "
          "p95 ms | p99 ms | idle RSS | peak RSS | CPU s |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    for path, report in reports:
        date = args.date or str(report.get("generated_at", ""))[:10]
        scenario = report["scheme"]["scenario"]
        target = report["target"]
        label = worker_label(target)
        for row in report["medians"]:
            print(
                f"| {date} | {commit} | {scenario} | {target['name']} | {label} | "
                f"{row['concurrency']} | {row['rps']:.0f} | {row['p50_ms']:.2f} | "
                f"{row['p95_ms']:.2f} | {row['p99_ms']:.2f} | "
                f"{mib(row['idle_rss_bytes'])} | {mib(row['peak_rss_bytes'])} | "
                f"{row['cpu_secs']:.2f} |"
            )

    headroom = [(path, report) for path, report in reports if report.get("headroom")]
    if headroom:
        print()
        print("| date | commit | scenario | concurrencies | rps | p50 ms | p95 ms | p99 ms |")
        print("| --- | --- | --- | --- | --- | --- | --- | --- |")
        for _path, report in headroom:
            scenario = report["scheme"]["scenario"]
            for point in report["headroom"]:
                print(
                    f"| {args.date or str(report.get('generated_at', ''))[:10]} | {commit} | "
                    f"{scenario} | {point['concurrency']} | {point['rps']:.0f} | "
                    f"{point['p50_ms']:.2f} | {point['p95_ms']:.2f} | {point['p99_ms']:.2f} |"
                )

    print()
    print("Run inventory (load average is the shared machine's 1-minute average):")
    for path, report in reports:
        scheme = report["scheme"]
        target = report["target"]
        print(
            f"- {path}: {scheme['scenario']} / {target['name']} "
            f"({worker_label(target)}), {scheme['repetitions']} repetition(s), "
            f"{scheme['duration_secs']:g}s + {scheme['warmup_secs']:g}s warm-up, "
            f"load {report.get('load_average_start')} -> {report.get('load_average_end')}, "
            f"generated {report.get('generated_at')}"
        )
        problems = [
            row
            for row in report["medians"]
            if row["errors"] or row["reconnects"]
        ]
        for row in problems:
            print(
                f"  ! concurrency {row['concurrency']}: {row['errors']} error(s), "
                f"{row['reconnects']} reconnect(s) in the worst repetition"
            )
        for note in report.get("notes", []):
            print(f"  note: {note}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
