#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Sample cgroup v2 counters for the bench containers, 4 times a second.

This runs *inside* the `plane-rs-bench-stats` sidecar, not on the host: the
Docker VM's cgroup filesystem is not visible from macOS, and a container that
mounts it read-only is the cheapest way to read `memory.current`, `memory.stat`
and `cpu.stat` for another container's cgroup.

One CSV line per sample, no timestamp: the host reads this container's stdout
with `docker logs --timestamps`, so every line carries a host-clock arrival
time. `difftest bench` cannot sample a container (`ps` on the host cannot see
the VM's processes), which is why Phase 3's memory and CPU numbers come from
here; see `apps/server/BENCH.md`.
"""

import os
import sys
import time

# `docker info` on this VM uses the cgroupfs driver, so a container's cgroup is
# /sys/fs/cgroup/docker/<container id>. The mount point inside the sidecar is
# /host/cgroup (see run-phase3.sh).
CGROUP_ROOT = os.environ.get("CGROUP_ROOT", "/host/cgroup")
INTERVAL = float(os.environ.get("INTERVAL", "0.25"))


def read_text(path):
    with open(path, "r", encoding="ascii") as handle:
        return handle.read()


def memory_stat(directory):
    values = {}
    try:
        text = read_text(os.path.join(directory, "memory.stat"))
    except OSError:
        return values
    for line in text.splitlines():
        parts = line.split()
        if len(parts) == 2:
            try:
                values[parts[0]] = int(parts[1])
            except ValueError:
                pass
    return values


def cpu_usage_usec(directory):
    try:
        text = read_text(os.path.join(directory, "cpu.stat"))
    except OSError:
        return -1
    for line in text.splitlines():
        key, _, value = line.partition(" ")
        if key == "usage_usec":
            try:
                return int(value)
            except ValueError:
                return -1
    return -1


def sample(name, directory):
    try:
        current = int(read_text(os.path.join(directory, "memory.current")).strip())
    except (OSError, ValueError):
        current = -1
    stat = memory_stat(directory)
    return (
        name,
        current,
        stat.get("anon", -1),
        stat.get("file", -1),
        stat.get("inactive_file", -1),
        cpu_usage_usec(directory),
    )


def main():
    targets = []
    for item in os.environ.get("TARGETS", "").split(","):
        item = item.strip()
        if not item:
            continue
        name, _, container = item.partition(":")
        directory = os.path.join(CGROUP_ROOT, "docker", container)
        if not os.path.isdir(directory):
            sys.stderr.write(
                f"cgroup-sampler: {name}: no cgroup at {directory} "
                f"(is the cgroup driver cgroupfs?)\n"
            )
            return 2
        targets.append((name, directory))
    if not targets:
        sys.stderr.write("cgroup-sampler: TARGETS is empty\n")
        return 2

    # Line-buffered so `docker logs -f` sees each sample as it is written.
    out = sys.stdout
    next_at = time.monotonic()
    while True:
        for name, directory in targets:
            row = sample(name, directory)
            out.write(",".join(str(value) for value in row) + "\n")
        out.flush()
        # Fixed cadence, not "sleep after work": a slow read must not make the
        # sample rate drift.
        next_at += INTERVAL
        delay = next_at - time.monotonic()
        if delay > 0:
            time.sleep(delay)
        else:
            next_at = time.monotonic()


if __name__ == "__main__":
    sys.exit(main())
