# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record the CSV corpus used by `plane_core::csv`.

Every case is a list of rows pushed through the exact chain the view uses:

    plane.utils.csv_utils.sanitize_csv_row(row)
    csv.writer(buffer, delimiter=",", quoting=csv.QUOTE_ALL).writerow(...)

`csv.writer` converts non-string values with `str()`, which is where a datetime
becomes `2026-01-05 01:00:00+00:00` (space separator). The corpus records the
converted strings so the Rust writer only has to agree on quoting and line
endings.

Regenerate from the repository root:

    apps/api/.venv/bin/python apps/server/tools/inventory/csv_corpus.py
    apps/api/.venv/bin/python apps/server/tools/inventory/csv_corpus.py --check

The output is `apps/server/crates/plane-core/tests/golden/csv-writer.json`, which
`crates/plane-core/tests/csv.rs` reads.
"""

import argparse
import csv
import datetime
import importlib.util
import io
import json
import pathlib
import sys

SERVER = pathlib.Path(__file__).resolve().parents[2]
CSV_UTILS = SERVER.parent / "api" / "plane" / "utils" / "csv_utils.py"
OUT = SERVER / "crates/plane-core/tests/golden/csv-writer.json"


def load_sanitizer():
    """Import `plane/utils/csv_utils.py` without the Django app."""
    spec = importlib.util.spec_from_file_location("plane_csv_utils", CSV_UTILS)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


HEADER = [
    "Actor name",
    "Issue ID",
    "Project",
    "Created at",
    "Updated at",
    "Action",
    "Field",
    "Old value",
    "New value",
]

UTC = datetime.timezone.utc
FIXED = datetime.datetime(2026, 1, 5, 1, 0, tzinfo=UTC)

CASES = [
    ("header", [HEADER]),
    (
        "activity_rows",
        [
            HEADER,
            # field NULL, no values: the two `None`s become empty quoted fields.
            [
                "Mia Member",
                "ALP - 1",
                "Alpha Platform",
                FIXED,
                FIXED,
                "issue.activity.created",
                None,
                None,
                None,
            ],
            # Every quoting hazard in one row.
            [
                "Ada Admin",
                "BET - 12",
                'Beta "Mobile", Inc.',
                FIXED,
                FIXED,
                "issue.activity.updated",
                "priority",
                "a,b",
                'he said "hi"',
            ],
        ],
    ),
    ("empty_document", []),
    ("empty_row", [[]]),
    (
        "csv_quoting",
        [
            ["plain", "", None, "a,b", 'he said "hi"'],
            ["a\nb", "a\r\nb", "a\rb", '"""', ",,,"],
            ["trailing,", ",leading", "both,", " space ", "  "],
        ],
    ),
    (
        "formula_triggers",
        [
            ["=1+2", "+1", "-1", "@x", "\tx"],
            ["\rx", "\nx", "", None, " =1"],
            ["'=1", "x=1", "-", "=", "@"],
        ],
    ),
    (
        "unicode",
        [
            ["héllo → 世界 🚀", "ß", "İstanbul", "日本語のテキスト"],
            ["emoji 👩‍👩‍👧‍👦 family", "nul-ish \\0", "tab\there"],
        ],
    ),
    (
        "datetimes",
        [
            [
                datetime.datetime(2026, 1, 5, 1, 0, tzinfo=UTC),
                datetime.datetime(2026, 1, 5, 1, 0, 0, 123456, tzinfo=UTC),
                datetime.date(2026, 1, 5),
            ],
        ],
    ),
    (
        "multi_line",
        [
            ["first line\nsecond line", "only\rreturn", "crlf\r\nend"],
            ["line with \"quote\"\nand newline"],
        ],
    ),
]


def render(rows, sanitize_csv_row):
    buffer = io.StringIO()
    writer = csv.writer(buffer, delimiter=",", quoting=csv.QUOTE_ALL)
    for row in rows:
        writer.writerow(sanitize_csv_row(row))
    return buffer.getvalue()


def py_value(value):
    """`csv.writer`'s own conversion, recorded so Rust compares strings."""
    if value is None:
        return None
    if isinstance(value, str):
        return value
    return str(value)


def build():
    sanitize_csv_row = load_sanitizer().sanitize_csv_row
    cases = []
    for name, rows in CASES:
        cases.append(
            {
                "name": name,
                "rows": [[py_value(value) for value in row] for row in rows],
                "csv": render(rows, sanitize_csv_row),
            }
        )
    return {
        "generated_by": "apps/server/tools/inventory/csv_corpus.py",
        "python": sys.version.split()[0],
        "writer": 'csv.writer(delimiter=",", quoting=csv.QUOTE_ALL)',
        "cases": cases,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if the committed file is stale")
    args = parser.parse_args()

    document = build()
    rendered = json.dumps(document, ensure_ascii=False, indent=2) + "\n"
    if args.check:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != rendered:
            sys.exit(f"{OUT} is stale; re-run without --check")
        print(f"{OUT} is up to date ({len(document['cases'])} cases)")
        return
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(rendered, encoding="utf-8")
    print(f"wrote {OUT} ({len(document['cases'])} cases)")


if __name__ == "__main__":
    main()
