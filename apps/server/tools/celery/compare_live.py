# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Compare the four decodings of one live-check case.

For one case, ``live_check.sh`` leaves four files:

* ``py_kombu.json``  — Python's message drained with Python kombu;
* ``py_testkit.json`` — the same Python message drained with the harness's
  Rust decoder (``plane-testkit::celery``);
* ``rs_kombu.json``  — the Rust message drained with Python kombu;
* ``rs_testkit.json`` — the Rust message drained with the harness decoder.

It checks that

1. all four decode to the same task, headers, properties, args, kwargs and
   embed dict;
2. the Python- and Rust-published **body bytes** are identical;
3. the raw attributes match ``tools/celery/golden/messages.json`` for the
   case (so the live broker does not alter what the memory transport
   recorded).
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve()
GOLDEN = HERE.parent / "golden" / "messages.json"


def load(path: pathlib.Path):
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def testkit_decoded(message: dict) -> dict:
    return {
        "task": message.get("task"),
        "headers": message.get("headers", {}),
        "properties": message.get("properties", {}),
        "args": message.get("args"),
        "kwargs": message.get("kwargs"),
        "body_extra": message.get("body_extra"),
    }


def diff(expected, actual, where: str) -> list[str]:
    if expected == actual:
        return []
    if isinstance(expected, dict) and isinstance(actual, dict):
        problems = []
        for key in sorted(set(expected) | set(actual)):
            if key not in expected:
                problems.append(f"{where}.{key}: unexpected {actual[key]!r}")
            elif key not in actual:
                problems.append(f"{where}.{key}: missing (expected {expected[key]!r})")
            else:
                problems.extend(diff(expected[key], actual[key], f"{where}.{key}"))
        return problems
    return [f"{where}: expected {expected!r}, got {actual!r}"]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--case", required=True)
    parser.add_argument("--dir", required=True, type=pathlib.Path)
    args = parser.parse_args()

    py_kombu = load(args.dir / "py_kombu.json")
    py_testkit = load(args.dir / "py_testkit.json")
    rs_kombu = load(args.dir / "rs_kombu.json")
    rs_testkit = load(args.dir / "rs_testkit.json")

    for name, value in (
        ("py_kombu", py_kombu),
        ("py_testkit", py_testkit),
        ("rs_kombu", rs_kombu),
        ("rs_testkit", rs_testkit),
    ):
        if len(value) != 1:
            print(f"FAIL {args.case}: expected 1 message in {name}, got {len(value)}")
            return 1

    decoded = {
        "py_kombu": py_kombu[0]["decoded"],
        "py_testkit": testkit_decoded(py_testkit[0]),
        "rs_kombu": rs_kombu[0]["decoded"],
        "rs_testkit": testkit_decoded(rs_testkit[0]),
    }
    problems = []
    for name in ("py_testkit", "rs_kombu", "rs_testkit"):
        problems.extend(diff(decoded["py_kombu"], decoded[name], name))

    raw_py = py_kombu[0]["raw"]
    raw_rs = rs_kombu[0]["raw"]
    problems.extend(diff(raw_py, raw_rs, "raw"))

    with GOLDEN.open(encoding="utf-8") as handle:
        golden = {message["case"]: message for message in json.load(handle)["messages"]}
    recorded = golden.get(args.case)
    if recorded is None:
        problems.append(f"case {args.case!r} is not in golden/messages.json")
    else:
        expected = {
            "task": recorded["task"],
            "task_id": recorded["task_id"],
            "properties": recorded["properties"],
            "headers": recorded["headers"],
            "body_b64": recorded["body_b64"],
        }
        problems.extend(diff(expected, raw_py, "golden"))

    if problems:
        print(f"FAIL {args.case}")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print(
        f"OK   {args.case}: body {len(raw_py['body_b64'])} b64 chars, "
        f"4 decoders agree, golden matches"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
