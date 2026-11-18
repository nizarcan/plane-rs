#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Regenerate the HTML parser's lookup tables from their pinned sources.

`crates/plane-http-public/src/common/html_tables.rs` holds the two tables
`common/html.rs` drives its libxml2-compatible HTML parser from:

* `START_CLOSE` -- `htmlStartClose` from libxml2 2.14.6 (`HTMLparser.c`), the
  parser lxml links against in `apps/api/.venv` (it reports
  `LIBXML_VERSION (2, 14, 6)`). The table is pinned verbatim at
  `tools/inventory/data/libxml2-2.14.6-htmlStartClose.txt`.
* `ENTITIES` -- CPython's `html.entities.html5`, the WHATWG named character
  reference table. libxml2's `html5ent.inc` is generated from the same source;
  the third field is libxml2's `ENT_F_SEMICOLON`, derived from whether the
  unterminated spelling also exists in `html5`.

    cd apps/api
    .venv/bin/python ../server/tools/inventory/lxml_html_tables.py
    .venv/bin/python ../server/tools/inventory/lxml_html_tables.py --check

`--check` exits non-zero when the generated file is stale, so CI can pin it.
"""

from __future__ import annotations

import argparse
import html.entities
import re
import shutil
import subprocess
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve()
SERVER_DIR = SCRIPT_PATH.parents[2]
PINNED_START_CLOSE = (
    SERVER_DIR / "tools" / "inventory" / "data" / "libxml2-2.14.6-htmlStartClose.txt"
)
OUTPUT = SERVER_DIR / "crates" / "plane-http-public" / "src" / "common" / "html_tables.rs"


def rustfmt(source: str) -> str:
    """Format the generated source the way `cargo fmt` does.

    The committed file is rustfmt-formatted, so emitting unformatted code made
    `--check` fail on a hand-formatted but semantically identical file. Rustfmt
    is part of the toolchain the gates already require.
    """
    executable = shutil.which("rustfmt")
    if executable is None:
        raise SystemExit("rustfmt not found on PATH (the Rust toolchain provides it)")
    result = subprocess.run(
        [executable, "--edition", "2024", "--emit", "stdout", "--quiet"],
        input=source,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise SystemExit(f"rustfmt failed: {result.stderr.strip()}")
    return result.stdout

def rust_string(value: str) -> str:
    # Every character of a replacement is written as `\u{...}`: the table is
    # full of invisible and astral characters, and keeping the escaping uniform
    # makes a diff of the generated file readable. Rust decodes the escapes at
    # compile time.
    return '"' + "".join(f"\\u{{{ord(character):x}}}" for character in value) + '"'


def start_close() -> list[tuple[str, str]]:
    text = PINNED_START_CLOSE.read_text(encoding="utf-8")
    pairs = re.findall(r'\{ "([^"]+)", "([^"]+)" \}', text)
    if len(pairs) != 249:
        raise SystemExit(f"{PINNED_START_CLOSE}: expected 249 entries, found {len(pairs)}")
    # `implies_close` binary-searches the table, so it must stay sorted.
    if pairs != sorted(pairs):
        raise SystemExit(f"{PINNED_START_CLOSE}: entries are not sorted")
    return pairs


def entities() -> list[tuple[str, str, bool]]:
    table = html.entities.html5
    rows = []
    for key, value in table.items():
        if not key.endswith(";"):
            continue
        name = key[:-1]
        rows.append((name, value, name not in table))
    rows.sort(key=lambda row: row[0])
    if len(rows) != 2125:
        raise SystemExit(f"html.entities.html5: expected 2125 entries, found {len(rows)}")
    return rows


def render() -> str:
    lines = [
        "// Copyright (c) 2023-present Plane Software, Inc. and contributors",
        "// SPDX-License-Identifier: AGPL-3.0-only",
        "// See the LICENSE file for details.",
        "",
        "//! GENERATED FILE -- do not edit by hand.",
        "//!",
        "//! Regenerate with",
        "//! `apps/api/.venv/bin/python tools/inventory/lxml_html_tables.py`;",
        "//! `--check` fails when this file is stale.",
        "//!",
        "//! * [`START_CLOSE`]: `htmlStartClose` from libxml2 2.14.6's",
        "//!   `HTMLparser.c`, pinned verbatim at",
        "//!   `tools/inventory/data/libxml2-2.14.6-htmlStartClose.txt`.",
        "//! * [`ENTITIES`]: CPython's `html.entities.html5`, the WHATWG named",
        "//!   character reference table libxml2's `html5ent.inc` is generated",
        "//!   from. The third field is `ENT_F_SEMICOLON`: `false` when the",
        "//!   unterminated spelling is also a valid reference.",
        "",
        "/// `htmlStartClose`: `(open tag, tag that implies its end)`, sorted.",
        "pub(crate) const START_CLOSE: &[(&str, &str)] = &[",
    ]
    for old, new in start_close():
        lines.append(f'    ("{old}", "{new}"),')
    lines.extend(["];", "", "/// The WHATWG named references, sorted by name.", "pub(crate) const ENTITIES: &[(&str, &str, bool)] = &["])
    for name, value, semicolon in entities():
        lines.append(f"    ({rust_string(name)}, {rust_string(value)}, {str(semicolon).lower()}),")
    lines.append("];")
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail when the file is stale")
    args = parser.parse_args()

    body = rustfmt(render())
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_text(encoding="utf-8") != body:
            print(f"{OUTPUT} is stale; rerun without --check")
            return 1
        print(f"{OUTPUT}: up to date")
        return 0
    OUTPUT.write_text(body, encoding="utf-8")
    print(f"{OUTPUT}: written")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
