#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Generate bs4's named character reference table for the Rust port.

`BeautifulSoupHTMLParser.handle_entityref` looks the parsed name up in
`EntitySubstitution.HTML_ENTITY_TO_CHARACTER`, which bs4 4.12.3 builds from
CPython's `html.entities.html5` by stripping the trailing semicolon from every
key and keeping the first value of a duplicate name (the iteration is
`sorted(html5.items())`, `bs4/dammit.py:93-114`). `html.parser` never passes
the semicolon, so the stripped table is exactly what the port needs.

    apps/api/.venv/bin/python apps/server/tools/inventory/bs4_entities.py
    apps/api/.venv/bin/python apps/server/tools/inventory/bs4_entities.py --check

The output is `apps/server/crates/plane-tasks/src/bs4_entities.rs`, read by
`crates/plane-tasks/src/bs4.rs`.
"""

from __future__ import annotations

import argparse
import html.entities
import pathlib
import sys

SERVER = pathlib.Path(__file__).resolve().parents[2]
OUT = SERVER / "crates" / "plane-tasks" / "src" / "bs4_entities.rs"

HEADER = """// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! GENERATED FILE -- do not edit by hand.
//!
//! `EntitySubstitution.HTML_ENTITY_TO_CHARACTER` from bs4 4.12.3: CPython's
//! `html.entities.html5` with the trailing semicolon stripped, first value
//! winning (`bs4/dammit.py:93-114`).
//!
//! Regenerate with
//! `apps/api/.venv/bin/python tools/inventory/bs4_entities.py`;
//! `--check` fails when this file is stale.

// The table contains zero-width and other invisible characters by definition.
#![allow(clippy::invisible_characters)]

/// `(name, replacement)`, sorted by name for [`lookup`].
pub(crate) const ENTITIES: &[(&str, &str)] = &[
"""

FOOTER = """];

/// The replacement text for a named reference, as bs4's handler does.
#[must_use]
pub(crate) fn lookup(name: &str) -> Option<&'static str> {
    ENTITIES
        .binary_search_by(|(key, _)| key.cmp(&name))
        .ok()
        .map(|index| ENTITIES[index].1)
}
"""


def rust_string(value: str) -> str:
    out = ['"']
    for ch in value:
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif ord(ch) < 0x20 or ord(ch) == 0x7F:
            out.append(f"\\u{{{ord(ch):X}}}")
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def build() -> str:
    mapping: dict[str, str] = {}
    for key, value in sorted(html.entities.html5.items()):
        name = key[:-1] if key.endswith(";") else key
        if name not in mapping:
            mapping[name] = value
    lines = [HEADER]
    for name in sorted(mapping):
        lines.append(f"    ({rust_string(name)}, {rust_string(mapping[name])}),\n")
    lines.append(FOOTER)
    return "".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    text = build()
    if args.check:
        current = OUT.read_text() if OUT.exists() else ""
        if current != text:
            print(f"{OUT} is stale; rerun without --check", file=sys.stderr)
            return 1
        print(f"{OUT}: up to date")
        return 0
    OUT.write_text(text)
    print(f"{OUT}: {text.count(chr(10))} lines")
    return 0


if __name__ == "__main__":
    sys.exit(main())
