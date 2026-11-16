#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Memory over time at cumulative request checkpoints.

Reads what `memcurve.sh` produced:

* `samples.log`: host-stamped `<epoch> <name>,memory.current,anon,file,inactive_file,cpu_usec`
  lines from `cgroup-sampler.py` (one target: the benchmarked container);
* `markers.log`: `<epoch> <label> <cumulative-requests>` lines.

For every marker it prints the last sample before it: cache-excluded RSS
(`memory.current - inactive_file`), raw `memory.current`, `anon`, `file`, the
CPU seconds spent since the previous marker, and the increase since the
previous checkpoint.

    tools/bench/memcurve.py --samples S --markers M [--label "mimalloc"] [--json out.json]
"""

import argparse
import json
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
            fields = rest.split(",")
            if len(fields) != 6:
                continue
            try:
                rows.append(
                    (
                        parse_epoch(stamp),
                        fields[0],
                        int(fields[1]),
                        int(fields[2]),
                        int(fields[3]),
                        int(fields[4]),
                        int(fields[5]),
                    )
                )
            except ValueError:
                continue
    rows.sort(key=lambda row: row[0])
    return rows


def load_markers(path):
    markers = []
    with open(path, "r", encoding="utf-8") as handle:
        for line in handle:
            stamp, _, rest = line.strip().partition(" ")
            label, _, requests = rest.partition(" ")
            try:
                markers.append((float(stamp), label, int(requests)))
            except ValueError:
                continue
    return markers


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--samples", required=True)
    parser.add_argument("--markers", required=True)
    parser.add_argument("--label", default="")
    parser.add_argument("--name", default="api", help="sampler target name to report")
    parser.add_argument("--json", default="")
    args = parser.parse_args(argv[1:])

    all_rows = load_samples(args.samples)
    if not all_rows:
        raise SystemExit(f"no samples in {args.samples}")
    names = sorted({row[1] for row in all_rows})
    if args.name not in names:
        raise SystemExit(f"no {args.name!r} samples; present: {names}")
    rows = [row for row in all_rows if row[1] == args.name]
    markers = load_markers(args.markers)
    if not markers:
        raise SystemExit(f"no markers in {args.markers}")

    out = []
    cpu_previous = rows[0][6]
    rss_previous = None
    for index, (when, label, requests) in enumerate(markers):
        sample = None
        for row in rows:
            if row[0] <= when:
                sample = row
            else:
                break
        if sample is None:
            continue
        _, _name, current, anon, file, inactive, cpu = sample
        rss = max(0, current - inactive) if inactive >= 0 else current
        elapsed = when - markers[0][0]
        record = {
            "label": label,
            "requests": requests,
            "elapsed_s": round(elapsed, 1),
            "rss_mib": round(rss / MIB, 1),
            "raw_mib": round(current / MIB, 1),
            "anon_mib": round(anon / MIB, 1),
            "file_mib": round(file / MIB, 1),
            "cpu_s": round((cpu - cpu_previous) / 1e6, 2),
            "growth_mib": None if rss_previous is None else round((rss - rss_previous) / MIB, 1),
        }
        out.append(record)
        cpu_previous = cpu
        rss_previous = rss

    print(f"## {args.label}" if args.label else "## memory curve")
    print()
    print("| checkpoint | requests | elapsed | RSS (cache-excl.) | raw | anon | file | growth | CPU s |")
    print("| --- | --- | --- | --- | --- | --- | --- | --- | --- |")
    for record in out:
        growth = "—" if record["growth_mib"] is None else f"{record['growth_mib']:+.1f} MiB"
        print(
            f"| {record['label']} | {record['requests']:,} | {record['elapsed_s']:.0f} s | "
            f"{record['rss_mib']:.1f} MiB | {record['raw_mib']:.1f} MiB | "
            f"{record['anon_mib']:.1f} MiB | {record['file_mib']:.1f} MiB | "
            f"{growth} | {record['cpu_s']:.2f} |"
        )
    if args.json:
        with open(args.json, "w", encoding="utf-8") as handle:
            json.dump(out, handle, indent=2)
        print(f"\nwrote {args.json}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
