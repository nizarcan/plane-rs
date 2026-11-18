#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record extra `lxml` normalisation cases for the v1 `description_html` gate.

`tools/inventory/lxml_html_corpus.py` records the 70-input hand-written corpus
that `crates/plane-http-public/tests/lxml_html.rs` asserts. This script records
a wider, deterministic set on top of it: every HTML construct Plane's editor
emits, the malformed shapes a client can send, and combinations that exercise
libxml2's implied-end-tag table. Output is
`crates/plane-http-public/tests/golden/lxml_html_extra.jsonl`, one JSON object
per line:

    {"input": "<p>a<p>b", "lxml": "<div><p>a</p><p>b</p></div>"}

`--check` fails when the file is stale.

    cd apps/api
    .venv/bin/python ../server/tools/inventory/lxml_html_extra_corpus.py
"""

from __future__ import annotations

import argparse
import json
import random
from pathlib import Path

from lxml import etree, html

SCRIPT_PATH = Path(__file__).resolve()
SERVER_DIR = SCRIPT_PATH.parents[2]
CORPUS = SERVER_DIR / "crates" / "plane-http-public" / "tests" / "golden" / "lxml_html_extra.jsonl"

# Fragments Plane's editor emits, plus the malformed shapes clients send.
SEEDS: list[str] = [
    "<p>a<p>b",
    "<p>a<div>b</div>c",
    "<b>x<p>y</p></b>",
    "<span><p>a</p></span>",
    "<h1><p>a</p></h1>",
    "<h1>a<h2>b</h1>",
    "<table><tr><td>a<td>b<tr><td>c</table>",
    "<table><caption>c</caption><thead><tr><th>h</th></tr></thead><tbody><tr><td>d</td></tr></tbody></table>",
    "<ul><li>a<ul><li>b</ul></ul>",
    "<ol><li>a<li>b</ol>",
    "<dl><dt>a<dd>b<dt>c</dl>",
    "<select><option>a<option>b</select>",
    "<form><p>a</form>",
    "<a href=x><a href=y>z</a>",
    "<p>x</p></p>",
    "<p>a</p><!-- c --><p>b</p>",
    "<p>a</p><!--x-->",
    "<div><p>a</div>",
    "<b><i>x</b></i>",
    "<p>&amp; &#38; &ampx; &AMP; &Amp; &notin; &not; &hellip; &check;</p>",
    "<p>&#x110000; &#xD800; &#65; &#0; &#x1F600;</p>",
    "<p>a\r\nb</p>",
    "<p title=\"a\r\nb\">x</p>",
    "<p title=\"a&quot;b'c\">x</p>",
    "<p title=\"a&amp;b&lt;c&gt;d\">x</p>",
    "<a href=\" a b \">x</a>",
    "<a href=\"\u00fc.png?x=1&y=2\">x</a>",
    "<img SRC=\"\u00fc.png\" ALT='x'>",
    "<p foo>x</p><input checked><p data-x=\"1\" data-y>z</p>",
    "<p title=a title=b>x</p>",
    "<P TITLE=x CLASS=y>z</P>",
    "<p/>x",
    "<p />",
    "<br /><hr/><img src=x/>",
    "<li></li><li>a",
    "<td></td><option></option>",
    "<html><head><title>t</title></head><body>x</body></html>",
    "<!DOCTYPE html><html><body><p>x</p></body></html>",
    "<head><title>x</title></head>y",
    "<body>text</body>",
    "<html></html>",
    "<html>  </html>",
    "<script>a<b && c>d</script>",
    "<style>.x{content:'<a>'}</style>",
    "<textarea>a &amp; b</textarea>",
    "<title>a &amp; b</title>",
    "<p><textarea>a</textarea></p>",
    "<noscript><p>a</p></noscript>",
    "<svg><circle r=\"1\"/></svg>",
    "<mention-component id=\"u1\" entity_name=\"user\"></mention-component>",
    "<image-component id=\"a1\" src=\"https://example.com/x.png\"></image-component>",
    "<p><label data-x=\"1\">b</label></p>",
    "<pre><code>let x = 1 &lt; 2;</code></pre>",
    "<blockquote>quote</blockquote><hr>",
    "<p>a<br>b<br/>c</p>",
    "<p>\u00a0 \u2003 \u3000 \u000b \u001c</p>",
    "<p>emoji \U0001f600 \u00fcn\u00efcode</p>",
    "<p>a > b & c < d</p>",
    "<div data-tight=\"true\" data-type=\"text\"><p class=\"editor-paragraph\"><span>Hello</span></p></div>",
    "<pre language=\"js\"><code spellcheck=\"false\">a</code></pre>",
    "<input type='checkbox' checked>",
    "<p><p>a</p></p>",
    "<table><tr><td><p>a</p></td></tr></table>",
    "<p>a<table><tr><td>x</table>b",
    "<td>x",
    "<tr><td>x",
    "<li>a",
    "<caption>a</caption>",
    "<p", "<p ", "<p class=\"x", "<", "<>", "< p>", "&bogus;", "\u0000", "\u0000a",
    "<div>", "<div><p>a",
    "", " ", "\n\t ", "\r", "\f", "hello", "+",
]

WRAPPERS = [
    ("", ""),
    ("<div>", "</div>"),
    ("<p>", "</p>"),
    ("<ul><li>", "</li></ul>"),
    ("<td>", "</td>"),
    ("\n", "  "),
    ("<!-- c -->", ""),
]


def record(value: str) -> dict | None:
    try:
        element = html.fromstring(value)
    except Exception as error:  # noqa: BLE001 - the corpus records the verdict
        return {"input": value, "error": f"{type(error).__name__}: {error}"}
    return {"input": value, "lxml": etree.tostring(element, encoding="unicode", method="html")}


def build() -> list[dict]:
    rng = random.Random(20261118)
    inputs: list[str] = list(SEEDS)
    for seed in SEEDS:
        for prefix, suffix in WRAPPERS:
            inputs.append(prefix + seed + suffix)
    for _ in range(120):
        left = rng.choice(SEEDS)
        right = rng.choice(SEEDS)
        separator = rng.choice(["", "\n", " ", "<!--x-->"])
        inputs.append(left + separator + right)
    seen: set[str] = set()
    records = []
    for value in inputs:
        if value in seen:
            continue
        seen.add(value)
        entry = record(value)
        if entry is not None:
            records.append(entry)
    return records


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail when the file is stale")
    args = parser.parse_args()

    records = build()
    body = "".join(json.dumps(record, ensure_ascii=False) + "\n" for record in records)
    if args.check:
        if not CORPUS.exists() or CORPUS.read_text(encoding="utf-8") != body:
            print(f"{CORPUS} is stale; rerun without --check")
            return 1
        print(f"{CORPUS}: {len(records)} cases, up to date")
        return 0
    CORPUS.write_text(body, encoding="utf-8")
    print(f"{CORPUS}: {len(records)} cases")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
