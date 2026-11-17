#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record what `lxml` does to `description_html` for `/api/v1/` work items.

`plane/api/serializers/issue.py:102-109` runs

    parsed = html.fromstring(data["description_html"])
    data["description_html"] = html.tostring(parsed, encoding="unicode")

and turns any exception into `400 {"non_field_errors": ["Invalid HTML passed"]}`.
This script records, for a corpus of inputs Plane's editor produces and the
malformed shapes a client can send:

* ``outcome``: ``ok`` with the re-serialised string, or ``error`` with
  ``lxml.etree.ParserError: Document is empty``;
* the same input after `nh3.clean` with Plane's allow-list, because that is
  what is actually stored (`plane/utils/content_validator.py:206-243`).

Output: ``crates/plane-http-public/tests/golden/lxml_html.jsonl``, one JSON
object per line, written in this file's order. `--check` fails when the file is
stale, so CI keeps it in step with the pinned lxml.

    cd apps/api
    .venv/bin/python ../server/tools/inventory/lxml_html_corpus.py
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

SCRIPT_PATH = Path(__file__).resolve()
SERVER_DIR = SCRIPT_PATH.parents[2]
REPO_ROOT = SCRIPT_PATH.parents[4]
API_DIR = REPO_ROOT / "apps" / "api"
CORPUS = SERVER_DIR / "crates" / "plane-http-public" / "tests" / "golden" / "lxml_html.jsonl"

# Shapes `@plane/editor` writes, then the malformed inputs a client can send.
CASES: list[str] = [
    # Empty and whitespace only: the only inputs libxml2 refuses in practice.
    # libxml2's blank set is ASCII space/`\t`/`\n`/`\r`/`\x0c`; every other
    # Unicode space is a text node and therefore an element's worth of input.
    "",
    " ",
    "\n\t ",
    "  \n  ",
    "\r",
    "\x0c",
    "\x0b",
    "\u00a0",
    "\u2003",
    "\u3000",
    "\x1c",
    # Plain text and a single element.
    "hello",
    "hello world",
    "+",
    "\u00a0",
    "<p>hello</p>",
    "<p></p>",
    "<p><br></p>",
    "<div><br></div>",
    # Multiple roots: libxml2 wraps them in a <div>.
    "<p>a</p><p>b</p>",
    "<p>a</p>\n<p>b</p>",
    "text<p>x</p>",
    "<p>x</p>text",
    "<p>a</p>trailing",
    # Editor output: nested spans, mentions, attributes, entities.
    '<p class="editor-paragraph"><span>Hello</span></p>',
    "<ul><li>a</li><li>b</li></ul>",
    "<ol><li>a</li><li>b</li></ol>",
    "<blockquote>quote</blockquote>",
    "<pre><code>let x = 1;</code></pre>",
    '<mention-component id="u1" entity_name="user"></mention-component>',
    '<image-component id="a1" src="https://example.com/x.png"></image-component>',
    '<a href="https://example.com/?a=1&amp;b=2" target="_blank">link</a>',
    "<p>a &amp; b</p>",
    "<p>&lt;script&gt;</p>",
    "<p>&copy; &#169; &#xA9;</p>",
    "<p>emoji \U0001f600 \u00fcn\u00efcode</p>",
    "<p>a\u00a0b</p>",
    "<table><tr><td>x</td></tr></table>",
    "<hr>",
    "<img src=\"x\">",
    "<input type='checkbox' checked>",
    # Case and quoting normalisation.
    "<P CLASS='x'>Hi</P>",
    '<p title="a &amp; b &quot; c">x</p>',
    '<p title=x>y</p>',
    "<p>a > b</p>",
    "<br/>",
    "<p>a<br/>b</p>",
    # Recover-mode fixups.
    "<p>a<p>b",
    "<ul><li>a<li>b</ul>",
    "<table><tr><td>x</table>",
    "<p>x</p></p>",
    "<svg><circle/></svg>",
    "<textarea>a<b</textarea>",
    # Comments, doctypes, documents, scripts.
    "<!-- c --><p>x</p>",
    "<p>a</p><!-- c -->",
    "<!DOCTYPE html><html><body><p>x</p></body></html>",
    "<html><body><p>x</p></body></html>",
    "<script>alert(1)</script>",
    "<style>.x{}</style>",
    # Refused: no element comes out of the parser.
    "<!-- only comment -->",
    "<?xml version='1.0'?>",
    "</p>",
    # Accepted although they look broken.
    "<",
    "<>",
    "< p>",
    "<p",
    "<p ",
    "&bogus;",
    "\x00",
    "\x00a",
]


def bootstrap():
    if str(API_DIR) not in sys.path:
        sys.path.insert(0, str(API_DIR))
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    os.environ.setdefault("SECRET_KEY", "inventory")
    os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
    os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
    os.environ.setdefault("ENABLE_DRF_SPECTACULAR", "0")
    import django

    django.setup()


def records() -> list[dict]:
    bootstrap()
    from lxml import html

    from plane.utils.content_validator import ATTRIBUTES, ALLOWED_TAGS, SAFE_PROTOCOLS
    import nh3

    out: list[dict] = []
    for case in CASES:
        record: dict = {"input": case}
        try:
            parsed = html.fromstring(case)
            normalized = html.tostring(parsed, encoding="unicode")
            record["outcome"] = "ok"
            record["lxml"] = normalized
            record["stored"] = nh3.clean(
                normalized,
                tags=ALLOWED_TAGS,
                attributes=ATTRIBUTES,
                url_schemes=SAFE_PROTOCOLS,
            )
        except Exception as exc:  # noqa: BLE001 - the view catches everything
            record["outcome"] = "error"
            record["error"] = f"{type(exc).__name__}: {exc}"
        out.append(record)
    return out


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=CORPUS)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)

    payload = "".join(json.dumps(record, ensure_ascii=False) + "\n" for record in records())
    if args.check:
        current = args.out.read_text(encoding="utf-8") if args.out.exists() else ""
        if current != payload:
            print(f"stale: {args.out}", file=sys.stderr)
            return 1
        print(f"{args.out.name}: up to date")
        return 0
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(payload, encoding="utf-8")
    print(f"wrote {args.out} ({len(CASES)} cases)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
