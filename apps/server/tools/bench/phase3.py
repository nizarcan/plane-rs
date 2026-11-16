#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Post-process Phase 3's container benchmark.

`difftest bench` measures throughput and latency itself, but in attach mode it
has no PID: `ps` on the host cannot see the Docker VM's processes, so the
memory and CPU columns are zero. `run-phase3.sh` therefore runs a sidecar
(`plane-rs-bench-stats`, see `cgroup-sampler.py`) that streams cgroup v2
counters for both containers, and `stamp.py` timestamps the bench's own output.
This script joins the two streams:

  * a measured window ends when `difftest bench` prints its `conc <n>:` line,
    so the window is `[line time - duration, line time]`;
  * peak memory is the highest `memory.current` in that window (and the highest
    `memory.current - inactive_file`, which is what `docker stats` reports);
  * CPU seconds are the `cpu.stat` `usage_usec` delta between the last sample
    before the window and the last sample at its end;
  * idle memory is the median of the 3 s before the first warm-up (the bench
    has just settled for `--settle` seconds there).

Subcommands:

    phase3.py summarize --samples S --events E --report R --target api \
        --duration 20 --warmup 5 --out row.json
    phase3.py idle --samples S --events E --out idle.json
    phase3.py table --rows DIR --out table.md [--ratios ratios.md]

The JSON a `summarize` run writes keeps every per-repetition value, so a
suspicious median can be traced to the repetition that produced it.
"""

import argparse
import json
import os
import re
import statistics
import sys
from datetime import datetime

CONC_LINE = re.compile(r"^\s*conc\s+(\d+):")
MIB = 1024.0 * 1024.0


def parse_epoch(text):
    """A Docker `--timestamps` RFC3339Nano stamp or a plain epoch float."""
    try:
        return float(text)
    except ValueError:
        pass
    if text.endswith("Z"):
        text = text[:-1] + "+00:00"
    return datetime.fromisoformat(text).timestamp()


def load_samples(path):
    """`[(time, name, memory_current, anon, file, inactive_file, cpu_usec)]`."""
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
    by_name = {}
    for row in rows:
        by_name.setdefault(row[1], []).append(row)
    return by_name


def load_events(path):
    events = []
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        for line in handle:
            line = line.rstrip("\n")
            stamp, _, text = line.partition(" ")
            try:
                when = float(stamp)
            except ValueError:
                continue
            events.append((when, text))
    events.sort(key=lambda item: item[0])
    return events


def samples_between(series, start, end):
    return [row for row in series if start <= row[0] <= end]


def rss_bytes(row):
    """`memory.current` minus inactive file cache: what `docker stats` shows."""
    current, _anon, _file, inactive = row[2], row[3], row[4], row[5]
    if current < 0:
        return 0
    if inactive < 0:
        return current
    return max(0, current - inactive)


def summarize(args):
    series = load_samples(args.samples)
    if args.target not in series:
        raise SystemExit(f"no samples for target {args.target!r} in {args.samples}")
    for name, rows in series.items():
        if len(rows) < 10:
            raise SystemExit(f"target {name}: only {len(rows)} samples")
    rows = series[args.target]
    events = load_events(args.events)

    with open(args.report, "r", encoding="utf-8") as handle:
        report = json.load(handle)
    measurements = report["measurements"]
    windows = [
        (int(match.group(1)), when)
        for when, text in events
        if (match := CONC_LINE.match(text)) and "client ceiling" not in text
    ]
    # `difftest bench` prints the client-headroom lines with the same `conc N:`
    # prefix after the measurement lines; they are not target windows.
    windows = windows[: len(measurements)]
    if len(windows) != len(measurements):
        raise SystemExit(
            f"{args.report}: {len(measurements)} measurements but "
            f"{len(windows)} `conc` lines in {args.events}"
        )

    enriched = []
    for measurement, (concurrency, window_end) in zip(measurements, windows):
        if measurement["concurrency"] != concurrency:
            raise SystemExit(
                f"window/measurement order mismatch: report has "
                f"{measurement['concurrency']}, events have {concurrency}"
            )
        window_start = window_end - args.duration
        inside = samples_between(rows, window_start, window_end)
        if not inside:
            raise SystemExit(
                f"no samples in [{window_start:.3f}, {window_end:.3f}] for {args.target}"
            )
        before = [row for row in rows if row[0] <= window_start]
        if not before:
            raise SystemExit("no sample before the first window")
        cpu_start = before[-1][6]
        cpu_end = inside[-1][6]
        cpu_secs = (cpu_end - cpu_start) / 1e6 if cpu_start >= 0 and cpu_end >= 0 else 0.0
        measurement = dict(measurement)
        measurement["container"] = {
            "samples": len(inside),
            "peak_bytes": max(row[2] for row in inside),
            "peak_rss_bytes": max(rss_bytes(row) for row in inside),
            "after_bytes": inside[-1][2],
            "after_rss_bytes": rss_bytes(inside[-1]),
            "cpu_secs": cpu_secs,
            "sample_interval_s": (
                (inside[-1][0] - inside[0][0]) / (len(inside) - 1) if len(inside) > 1 else 0.0
            ),
        }
        enriched.append(measurement)

    # Idle: the bench has settled for `--settle` seconds immediately before the
    # first warm-up. Take the median of the three seconds before that.
    first_window_start = windows[0][1] - args.duration
    idle_end = first_window_start - args.warmup
    idle_rows = samples_between(rows, idle_end - 3.0, idle_end)
    idle = {
        "bytes": median([row[2] for row in idle_rows]),
        "rss_bytes": median([rss_bytes(row) for row in idle_rows]),
        "samples": len(idle_rows),
    }

    medians = []
    for concurrency in sorted({m["concurrency"] for m in enriched}):
        group = [m for m in enriched if m["concurrency"] == concurrency]

        def med(fn):
            return statistics.median(fn(m) for m in group)

        medians.append(
            {
                "concurrency": concurrency,
                "rps": med(lambda m: m["rps"]),
                "p50_ms": med(lambda m: m["p50_ms"]),
                "p95_ms": med(lambda m: m["p95_ms"]),
                "p99_ms": med(lambda m: m["p99_ms"]),
                "errors": max(m["errors"] for m in group),
                "reconnects": max(m["reconnects"] for m in group),
                "requests": statistics.median(m["requests"] for m in group),
                "idle_bytes": idle["bytes"],
                "idle_rss_bytes": idle["rss_bytes"],
                "peak_bytes": med(lambda m: m["container"]["peak_bytes"]),
                "peak_rss_bytes": med(lambda m: m["container"]["peak_rss_bytes"]),
                "cpu_secs": med(lambda m: m["container"]["cpu_secs"]),
                "rps_reps": [m["rps"] for m in group],
                "cpu_reps": [m["container"]["cpu_secs"] for m in group],
                "peak_reps": [m["container"]["peak_rss_bytes"] for m in group],
            }
        )

    result = {
        "scenario": report["scheme"]["scenario"],
        "target": args.target,
        "target_kind": report["target"]["kind"],
        "generated_at": report["generated_at"],
        "duration_secs": args.duration,
        "warmup_secs": args.warmup,
        "repetitions": report["scheme"]["repetitions"],
        "concurrencies": report["scheme"]["concurrencies"],
        "load_average_start": report.get("load_average_start"),
        "load_average_end": report.get("load_average_end"),
        "idle": idle,
        "medians": medians,
        "measurements": enriched,
        "notes": report.get("notes", []),
    }
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(result, handle, indent=2)
    for row in medians:
        print(
            f"  {args.target:6s} conc {row['concurrency']:>2}: "
            f"{row['rps']:>8.0f} req/s  p50 {row['p50_ms']:>7.2f}  "
            f"p95 {row['p95_ms']:>7.2f}  p99 {row['p99_ms']:>7.2f} ms  "
            f"idle {row['idle_rss_bytes'] / MIB:>6.1f}  "
            f"peak {row['peak_rss_bytes'] / MIB:>6.1f} MiB  "
            f"cpu {row['cpu_secs']:>6.2f} s  err {row['errors']}"
        )
    print(f"wrote {args.out}")
    return 0


def median(values):
    values = [value for value in values if value is not None]
    if not values:
        return 0
    return statistics.median(values)


def idle(args):
    series = load_samples(args.samples)
    events = load_events(args.events)
    marks = {}
    for when, text in events:
        if text.startswith("EVENT "):
            marks[text[len("EVENT ") :].strip()] = when
    out = {}
    for name, rows in series.items():
        entry = {}
        for label in ("start", "run-end"):
            key = f"{name}:{label}"
            if key not in marks:
                continue
            window = samples_between(rows, marks[key] - 5.0, marks[key])
            entry[label] = {
                "bytes": median([row[2] for row in window]),
                "rss_bytes": median([rss_bytes(row) for row in window]),
                "samples": len(window),
            }
        out[name] = entry
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(out, handle, indent=2)
    for name, entry in out.items():
        start = entry.get("start", {})
        end = entry.get("run-end", {})
        print(
            f"  {name:6s} idle: after start "
            f"{start.get('rss_bytes', 0) / MIB:>6.1f} MiB, after run "
            f"{end.get('rss_bytes', 0) / MIB:>6.1f} MiB"
        )
    print(f"wrote {args.out}")
    return 0


def load_rows(directory):
    rows = []
    for name in sorted(os.listdir(directory)):
        if not (name.startswith("row-") and name.endswith(".json")):
            continue
        with open(os.path.join(directory, name), "r", encoding="utf-8") as handle:
            rows.append(json.load(handle))
    return rows


def table(args):
    rows = load_rows(args.rows)
    if not rows:
        raise SystemExit(f"no row-*.json in {args.rows}")
    lines = [
        "| scenario | target | conc | rps | p50 ms | p95 ms | p99 ms | errors | idle | peak | CPU s |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for row in sorted(rows, key=lambda item: (item["scenario"], item["target"])):
        for median in row["medians"]:
            lines.append(
                "| {scenario} | {target} | {conc} | {rps:.0f} | {p50:.2f} | {p95:.2f} | "
                "{p99:.2f} | {errors} | {idle:.1f} MiB | {peak:.1f} MiB | {cpu:.2f} |".format(
                    scenario=row["scenario"],
                    target=row["target"],
                    conc=median["concurrency"],
                    rps=median["rps"],
                    p50=median["p50_ms"],
                    p95=median["p95_ms"],
                    p99=median["p99_ms"],
                    errors=median["errors"],
                    # The brief's memory targets are RSS-like (cache excluded);
                    # `docker stats` reports the same subtraction.
                    idle=median["idle_rss_bytes"] / MIB,
                    peak=median["peak_rss_bytes"] / MIB,
                    cpu=median["cpu_secs"],
                )
            )
    text = "\n".join(lines) + "\n"
    with open(args.out, "w", encoding="utf-8") as handle:
        handle.write(text)
    print(text)

    if args.ratios:
        index = {}
        for row in rows:
            for median in row["medians"]:
                index[(row["scenario"], median["concurrency"], row["target"])] = median
        ratio_lines = [
            "| scenario | conc | rps django | rps rust | rps ratio | p95 django | p95 rust | p95 ratio |",
            "| --- | --- | --- | --- | --- | --- | --- | --- |",
        ]
        for scenario in sorted({row["scenario"] for row in rows}):
            for concurrency in sorted(
                {median["concurrency"] for row in rows for median in row["medians"]}
            ):
                a = index.get((scenario, concurrency, "django"))
                b = index.get((scenario, concurrency, "api"))
                if not a or not b or a["rps"] == 0 or b["p95_ms"] == 0:
                    continue
                ratio_lines.append(
                    f"| {scenario} | {concurrency} | {a['rps']:.0f} | {b['rps']:.0f} | "
                    f"{b['rps'] / a['rps']:.1f}x | {a['p95_ms']:.2f} | {b['p95_ms']:.2f} | "
                    f"{a['p95_ms'] / b['p95_ms']:.1f}x |"
                )
        ratio_text = "\n".join(ratio_lines) + "\n"
        with open(args.ratios, "w", encoding="utf-8") as handle:
            handle.write(ratio_text)
        print(ratio_text)
    return 0


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)

    summarize_parser = sub.add_parser("summarize")
    summarize_parser.add_argument("--samples", required=True)
    summarize_parser.add_argument("--events", required=True)
    summarize_parser.add_argument("--report", required=True)
    summarize_parser.add_argument("--target", required=True)
    summarize_parser.add_argument("--duration", type=float, required=True)
    summarize_parser.add_argument("--warmup", type=float, required=True)
    summarize_parser.add_argument("--out", required=True)
    summarize_parser.set_defaults(func=summarize)

    idle_parser = sub.add_parser("idle")
    idle_parser.add_argument("--samples", required=True)
    idle_parser.add_argument("--events", required=True)
    idle_parser.add_argument("--out", required=True)
    idle_parser.set_defaults(func=idle)

    table_parser = sub.add_parser("table")
    table_parser.add_argument("--rows", required=True)
    table_parser.add_argument("--out", required=True)
    table_parser.add_argument("--ratios")
    table_parser.set_defaults(func=table)

    args = parser.parse_args(argv[1:])
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main(sys.argv))
