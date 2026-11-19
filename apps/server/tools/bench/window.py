#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Join the cgroup sampler and the event marks for one arbitrary window.

`phase3.py` summarizes `difftest bench` windows: it knows the measured window
ends when the bench prints its `conc N:` line. The Phase 8 task-throughput
measurement is not a `difftest bench` run, so it marks its window explicitly
(`EVENT <label>`, written by `run-phase8.sh`'s `mark`) and joins the two
streams here.

    tools/bench/window.py --samples samples-rustdrain.log \
        --events events-rustdrain.log --start drain-start --end drain-end \
        --out drain-rust.json

Output: duration, CPU seconds (the `cpu.stat` `usage_usec` delta between the
last sample before the start mark and the last sample before the end mark),
peak and final cache-excluded memory, raw `memory.current`, and sample counts.
"""

import argparse
import json
import statistics
import sys
from datetime import datetime

MIB = 1024.0 * 1024.0


def parse_epoch(text):
    try:
        return float(text)
    except ValueError:
        pass
    if text.endswith("Z"):
        text = text[:-1] + "+00:00"
    return datetime.fromisoformat(text).timestamp()


def load_samples(path):
    rows = []
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.strip()
            if not line:
                continue
            stamp, _, rest = line.partition(" ")
            try:
                when = parse_epoch(stamp)
            except ValueError:
                continue
            fields = rest.split(",")
            if len(fields) != 6:
                continue
            try:
                values = [int(field) for field in fields[1:]]
            except ValueError:
                continue
            rows.append((when, fields[0], *values))
    rows.sort(key=lambda row: row[0])
    return rows


def load_marks(path):
    marks = {}
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            stamp, _, text = line.partition(" ")
            try:
                when = float(stamp)
            except ValueError:
                continue
            if text.startswith("EVENT "):
                marks[text[len("EVENT ") :].strip()] = when
    return marks


def rss_bytes(row):
    current, _anon, _file, inactive = row[2], row[3], row[4], row[5]
    if current < 0:
        return 0
    if inactive < 0:
        return current
    return max(0, current - inactive)


def main(argv):
    parser = argparse.ArgumentParser()
    parser.add_argument("--samples", required=True)
    parser.add_argument("--events", required=True)
    parser.add_argument("--target", required=True, help="sampler target name")
    parser.add_argument("--start", required=True)
    parser.add_argument("--end", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv[1:])

    rows = [row for row in load_samples(args.samples) if row[1] == args.target]
    if not rows:
        raise SystemExit(f"no samples for target {args.target!r} in {args.samples}")
    marks = load_marks(args.events)
    for label in (args.start, args.end):
        if label not in marks:
            raise SystemExit(f"no EVENT {label} in {args.events}")
    start, end = marks[args.start], marks[args.end]
    if end <= start:
        raise SystemExit(f"end mark {end} is not after start mark {start}")

    inside = [row for row in rows if start <= row[0] <= end]
    before = [row for row in rows if row[0] <= start]
    if len(inside) < 2 or not before:
        raise SystemExit(
            f"need at least two samples inside the window and one before it "
            f"(inside={len(inside)}, before={len(before)})"
        )
    cpu_start = before[-1][6]
    cpu_end = inside[-1][6]
    entry = {
        "window": [args.start, args.end],
        "start_epoch": start,
        "end_epoch": end,
        "duration_secs": end - start,
        "samples": len(inside),
        "sample_interval_s": (inside[-1][0] - inside[0][0]) / (len(inside) - 1),
        "cpu_secs": (cpu_end - cpu_start) / 1e6 if cpu_start >= 0 and cpu_end >= 0 else 0.0,
        "peak_rss_bytes": max(rss_bytes(row) for row in inside),
        "peak_raw_bytes": max(row[2] for row in inside),
        "after_rss_bytes": rss_bytes(inside[-1]),
        "after_raw_bytes": inside[-1][2],
        "median_rss_bytes": statistics.median(rss_bytes(row) for row in inside),
    }
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(entry, handle, indent=2)
    print(
        f"{args.start}..{args.end}: {entry['duration_secs']:.2f} s, "
        f"CPU {entry['cpu_secs']:.2f} s, peak {entry['peak_rss_bytes'] / MIB:.1f} MiB, "
        f"{entry['samples']} samples"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
