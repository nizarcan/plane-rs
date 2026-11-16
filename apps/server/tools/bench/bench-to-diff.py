#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Turn a bench scenario into the differential scenario that verifies it.

A `scenarios/bench/reads-*.yaml` file lists weighted `requests`; `difftest run`
needs ordered `steps`. The two formats are otherwise identical (the bench
validator delegates to the differential one), so the conversion is a rename
plus dropping `weight` and making the implicit `expect_status: 200` explicit.

The output is written outside `scenarios/bench/` on purpose: the harness's own
test loads every `*.yaml` in that directory as a bench scenario, and a
`steps:`-shaped file would make it fail.

    apps/api/.venv/bin/python tools/bench/bench-to-diff.py \
        scenarios/bench/reads-users-me.yaml target/bench/p3/verify-users-me.yaml
"""

import sys

import yaml

STEP_FIELDS = ("query", "headers", "json", "form", "body")


def main(argv):
    if len(argv) != 3:
        sys.stderr.write("usage: bench-to-diff.py <bench.yaml> <diff.yaml>\n")
        return 2
    with open(argv[1], "r", encoding="utf-8") as handle:
        bench = yaml.safe_load(handle)

    steps = []
    for request in bench["requests"]:
        step = {
            "name": request["name"],
            "method": request.get("method", "GET"),
            "path": request["path"],
            "jar": request.get("jar", "default"),
            # Explicit, because a differential step without an expectation
            # accepts any status and the point of the run is that it is 200.
            "expect_status": request.get("expect_status", 200),
        }
        for field in STEP_FIELDS:
            if field in request:
                step[field] = request[field]
        steps.append(step)

    scenario = {
        "name": f"{bench['name']}-verify",
        "template_db": bench.get("template_db", "plane_tpl"),
        "redis_db": [4, 5],
        "steps": steps,
    }
    if bench.get("vars"):
        scenario["vars"] = bench["vars"]

    with open(argv[2], "w", encoding="utf-8") as handle:
        yaml.safe_dump(scenario, handle, sort_keys=False, default_flow_style=False)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
