#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Prefix every stdin line with the host clock and tee it to an events file.

`difftest bench` prints one line per measured window, after the window ends.
The container sampler's samples and these lines must share a clock; the host
clock is the only one both streams can be given. Used by `run-phase3.sh`:

    difftest bench ... | tools/bench/stamp.py "$out/bench.events"

Lines are written unchanged to stdout so the run still shows its progress.
`phase3.py summarize` reads the `<epoch> <line>` pairs back.
"""

import sys
import time


def main(argv):
    if len(argv) != 2:
        sys.stderr.write("usage: stamp.py <events-file>\n")
        return 2
    events_path = argv[1]
    with open(events_path, "a", encoding="utf-8") as events:
        for line in sys.stdin:
            line = line.rstrip("\n")
            events.write(f"{time.time():.6f} {line}\n")
            events.flush()
            sys.stdout.write(line + "\n")
            sys.stdout.flush()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
