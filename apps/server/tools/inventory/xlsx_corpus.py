#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record the XLSX golden corpus from openpyxl (the Rust writer's oracle).

Every case is a list of rows appended to `Workbook().active`. Values are
tagged so the Rust test can build the same cell types without guessing:
`["s", text]`, `["i", int]`, `["f", float]`, `["b", bool]`. `openpyxl` stores
strings inline, writes `docProps/core.xml` with the current UTC time, and uses
`time.localtime()` for the ZIP entry times, so the two timestamp families are
normalised to `<TS>` exactly as `difftest task` normalises them.

    apps/api/.venv/bin/python apps/server/tools/inventory/xlsx_corpus.py
    apps/api/.venv/bin/python apps/server/tools/inventory/xlsx_corpus.py --check

The output is `apps/server/crates/plane-xlsx/tests/golden/workbook.json`,
read by `crates/plane-xlsx/tests/golden.rs`.
"""

from __future__ import annotations

import argparse
import io
import json
import pathlib
import re
import sys
import zipfile

import openpyxl
from openpyxl import Workbook

SERVER = pathlib.Path(__file__).resolve().parents[2]
OUT = SERVER / "crates" / "plane-xlsx" / "tests" / "golden" / "workbook.json"

# The export's header row is 27 columns; another case reaches AA+ to pin the
# column-letter conversion.
EXPORT_HEADERS = [
    "Project Name",
    "Project Identifier",
    "Parent",
    "Identifier",
    "Sequence Id",
    "Name",
    "State Name",
    "Priority",
    "Assignees",
    "Subscribers",
    "Created By Name",
    "Start Date",
    "Target Date",
    "Completed At",
    "Created At",
    "Updated At",
    "Archived At",
    "Estimate",
    "Labels",
    "Cycles",
    "Modules",
    "Links",
    "Relations",
    "Comments",
    "Sub Issues Count",
    "Link Count",
    "Attachment Count",
    "Is Draft",
]

CASES = {
    "empty": [],
    "types": [
        [
            ["s", "h1"],
            ["s", "h2"],
            ["s", "h3"],
            ["s", "h4"],
            ["s", "h5"],
            ["s", "h6"],
            ["s", ""],
        ],
        [
            ["s", "x"],
            ["i", 3],
            ["f", 3.5],
            ["b", True],
            ["b", False],
            ["f", -0.00001],
            ["s", ""],
        ],
    ],
    "export_like": [
        [["s", header] for header in EXPORT_HEADERS],
        [
            ["s", "Alpha Platform"],
            ["s", "ALP"],
            ["s", "ALP-7"],
            ["s", "ALP-12"],
            ["i", 12],
            ["s", "A work item, with a comma"],
            ["s", "In Progress"],
            ["s", "high"],
            ["s", "Ada Admin, Mia Member"],
            ["s", "Mia Member"],
            ["s", "Ada Admin"],
            ["s", "2026-01-05"],
            ["s", ""],
            ["s", ""],
            ["s", "2026-01-05T01:00:00Z"],
            ["s", "2026-01-06T02:03:04.123456Z"],
            ["s", ""],
            ["s", "5"],
            ["s", "bug, ui"],
            ["s", "Sprint 1"],
            ["s", "Module A"],
            ["s", '[{"url": "https://example.com/a", "title": "A"}]'],
            ["s", "[]"],
            ["s", "[{'comment': 'héllo', 'created_by': 'Ada Admin', 'created_at': '2026-01-05 01:00:00'}]"],
            ["b", True],
        ],
    ],
    "special": [
        [
            ["s", "a&<b>\"c'"],
            ["s", " leading"],
            ["s", "trailing "],
            ["s", "line\nbreak"],
            ["s", "tab\there"],
            ["s", "carriage\rreturn"],
            ["s", "héllo 🚀"],
            ["s", "  "],
            ["s", "\u00a0nbsp\u00a0"],
            ["s", "#N/A"],
            ["s", "=1+1"],
        ]
    ],
    "numbers": [
        [
            ["i", 0],
            ["i", -1],
            ["i", 9007199254740993],
            ["f", 0.1],
            ["f", 1e16],
            ["f", 1e-7],
            ["f", 123456789.123456],
        ]
    ],
    "long_text": [[["s", "x" * 40000]]],
}


def normalise(text: str) -> str:
    """Mask the core-properties timestamps, the second timestamp family."""
    return re.sub(
        r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", "<TS>", text
    )


def rust_value(kind: str, value):
    return [kind, value]


def build(case: str, rows) -> dict:
    workbook = Workbook()
    sheet = workbook.active
    for row in rows:
        sheet.append([value for _, value in row])
    buffer = io.BytesIO()
    workbook.save(buffer)
    archive = zipfile.ZipFile(io.BytesIO(buffer.getvalue()))
    entries = []
    for name in archive.namelist():
        text = archive.read(name).decode("utf-8")
        if name == "docProps/core.xml":
            text = normalise(text)
        entries.append({"name": name, "text": text})
    return {
        "rows": [[rust_value(kind, value) for kind, value in row] for row in rows],
        "entries": entries,
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    corpus = {
        "openpyxl": openpyxl.__version__,
        "cases": {name: build(name, rows) for name, rows in CASES.items()},
    }
    text = json.dumps(corpus, indent=2, ensure_ascii=False, sort_keys=False) + "\n"
    if args.check:
        current = OUT.read_text(encoding="utf-8")
        if current != text:
            print(f"{OUT}: stale; run tools/inventory/xlsx_corpus.py", file=sys.stderr)
            return 1
        print(f"{OUT}: up to date ({len(CASES)} cases, openpyxl {openpyxl.__version__})")
        return 0
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT} ({len(CASES)} cases, openpyxl {openpyxl.__version__})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
