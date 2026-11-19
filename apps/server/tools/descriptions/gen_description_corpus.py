# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Record the description validators' output for the Rust port.

Runs the real Python functions on the same case list the Rust tests use and
writes/checks two corpora, one per crate that owns the port:

* `crates/plane-db/tests/golden/strip_tags.json` — `strip_tags`
  (`plane/utils/html_processor.py:28-31`);
* `crates/plane-http-app/tests/golden/description-validators.json` —
  `validate_html_content` (`plane/utils/content_validator.py:209-239`) and
  `validate_binary_data` (`plane/utils/content_validator.py:29-68`).

Usage (from `apps/server/`, with the dev stack's Django environment sourced —
`plane.settings.production` needs `DATABASE_URL`/`REDIS_URL` even for a pure
function):

    eval "$(bash dev/django-env.sh)"
    apps/api/.venv/bin/python tools/descriptions/gen_description_corpus.py
    apps/api/.venv/bin/python tools/descriptions/gen_description_corpus.py --check

The fixture timestamps are irrelevant here; the functions are pure. The
interpreter is pinned by `apps/api/.venv` (CPython 3.12.13) and the nh3 version
that ships with it (0.2.18, ammonia 4.0.0).
"""

import base64
import json
import os
import sys
from pathlib import Path

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API = _oracle_api_dir()
sys.path.insert(0, str(API))
os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")

import django  # noqa: E402

django.setup()

from plane.utils.content_validator import (  # noqa: E402
    validate_binary_data,
    validate_html_content,
)
from plane.utils.html_processor import strip_tags  # noqa: E402

# --- strip_tags ------------------------------------------------------------
# `HTMLParser(convert_charrefs=True)`: tags vanish, data survives with
# references decoded, comments/declarations vanish, `<script>`/`<style>`
# content is data (CDATA mode), `a < b` is data.
STRIP_CASES = [
    "",
    "plain",
    "<p>hello</p>",
    "<p>Hello <strong>world</strong>!</p>",
    "<div><p>a</p><p>b</p></div>",
    "<p>a<br/>b</p>",
    "<p>  spaced  </p>",
    "<p>a\tb\nc</p>",
    "<p>a\r\nb\rc</p>",
    "text</p>after",
    "<p>unclosed <b>bold",
    "<p>a<b>c",
    "a < b > c",
    "<p>a<1>b</p>",
    "<p>a<!x>b</p>",
    "<!-- c --><p>x</p>",
    "<p>a<!--x-->b</p>",
    "<!-- unterminated <p>x</p>",
    "<!DOCTYPE html><p>x</p>",
    "<?xml version='1.0'?><p>x</p>",
    "<![CDATA[raw <b>text</b>]]>",
    "<![CDATA[unterminated <p>x</p>",
    "<script>alert(1)</script>",
    "<SCRIPT>alert(1)</SCRIPT>",
    "<style>.x{color:red}</style>",
    "<script>if (a < b) { x() }</script>",
    "<script>a &amp; b</script>",
    "<script>a</script >b",
    "<script>a</ script>b",
    "<script/>x",
    "<scriptx>a</scriptx>b",
    "<script>unterminated <b>text",
    "<textarea><b>x</b></textarea>",
    # entities
    "<p>a &amp; b</p>",
    "<p>a &lt;b&gt; c</p>",
    "<p>&nbsp;</p>",
    "<p>&#39;q&#39;</p>",
    "<p>&#x27;quote&#x27;</p>",
    "<p>&#x1F600;</p>",
    "<p>&Aacute; &copy; &hellip; &trade; &euro; &rarr; &frac12; &mdash;</p>",
    "<p>&amp</p>",
    "<p>&amp x</p>",
    "<p>&ampx</p>",
    "<p>&notit;</p>",
    "<p>&; &#; &#x; &unknown;</p>",
    "<p>&#0; &#x110000; &#xD800;</p>",
    "<p>&#13; &#x80; &#x9f;</p>",
    "<p>&#1; &#8; &#127;</p>",
    "<p>&&amp;&</p>",
    "<p>&AMP; &Amp; &amp;</p>",
    "<p>&CounterClockwiseContourIntegral;</p>",
    "<p>&fjlig; &ThickSpace;</p>",
    # unicode
    "<p>caf\u00e9 \u4f60\u597d \U0001F600</p>",
    "<p>\u00e0\u00e8\u00ec\u00f2\u00f9 &egrave;</p>",
    "<p title='a\"b'>x</p>",
    "<p>a' b\"</p>",
    "<td>cell</td>",
    "<p>&#99999999999999999999;</p>",
    "<p>&#xZZ;</p>",
    "<p>&#x41</p>",
]

# --- validate_html_content -------------------------------------------------
# nh3.clean with ALLOWED_TAGS/ATTRIBUTES/SAFE_PROTOCOLS (`content_validator.py`).
HTML_CASES = [
    "<p>hello</p>",
    "<script>alert(1)</script>",
    "<style>.x{color:red}</style>",
    "<p onclick='x'>a</p>",
    "<p data-foo='1' data-tight='2'>x</p>",
    "<p data-tight='true'>x</p>",
    "<a href='javascript:alert(1)'>x</a>",
    "<a href='https://e.test/x?y=1&amp;z=2'>x</a>",
    "<img src='/api/assets/v2/static/x/' width='10'>",
    "<iframe src='x'></iframe>",
    "<p>a &amp; b</p>",
    "<mention-component id='1' entity_name='user'></mention-component>",
    "<p style='color:red'>x</p>",
    "<pre language='js'><code>a</code></pre>",
    "<p>line1\nline2</p>",
    "<!-- comment --><p>x</p>",
    "<p>caf\u00e9 \u4f60\u597d \U0001F600</p>",
    "<div><p>a</p><p>b <b>c</b></p></div>",
    "<table><tr><td colspan='2' textColor='red'>a</td></tr></table>",
    "<label class='x' for='y'>l</label>",
    "<image-component src='s' width='10' height='20' unknown='z'></image-component>",
    "<input type='checkbox' checked>",
    "<a href='mailto:x@y.z' target='_blank'>m</a>",
    "<a href='tel:+123'>t</a>",
    "<svg><circle/></svg>",
    "<p>&amp;lt;</p>",
    "<p>&#39;</p>",
    "<p class='' id=''>x</p>",
    "<th colwidth='1' style='x'>h</th>",
    "<p aria-label='a' role='b'>x</p>",
    "<p unclosed='x'>a",
    "<p>a</p >b",
    "<p><em>nested <strong>tags</strong></em></p>",
    "<blockquote><p>quote</p></blockquote>",
    "",
    "no markup at all",
    "<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>",
]

# --- validate_binary_data --------------------------------------------------
BINARY_CASES = [
    "",
    "aGVsbG8=",
    "not-base64!!!",
    base64.b64encode(b"hello world").decode(),
    base64.b64encode(b"<html><script>x</script></html>").decode(),
    base64.b64encode(b"abc").decode(),
    base64.b64encode(b"%PDF-1.4").decode(),
    base64.b64encode(b"JAVASCRIPT:alert(1)").decode(),
    base64.b64encode(b"data:text/html,x").decode(),
    base64.b64encode("caf\u00e9".encode()).decode(),
    base64.b64encode(b"<iframe src=x>").decode(),
    base64.b64encode(b"<!doctype html>").decode(),
    base64.b64encode(b"ok " * 100).decode(),
    base64.b64encode(b"\x00\x01\x02\x03").decode(),
    "YWJjZA",
    "YWJjZA==",
    "!!!!",
    "a",
    "====",
    " ",
    "aGVsbG8gd29ybGQ",
]


def strip_corpus() -> dict:
    return {
        "generated_by": "apps/server/tools/descriptions/gen_description_corpus.py",
        "python": sys.version.split()[0],
        "cases": [{"input": value, "output": strip_tags(value)} for value in STRIP_CASES],
    }


def validators_corpus() -> dict:
    return {
        "generated_by": "apps/server/tools/descriptions/gen_description_corpus.py",
        "python": sys.version.split()[0],
        "validate_html_content": [
            {
                "input": value,
                "is_valid": validate_html_content(value)[0],
                "error": validate_html_content(value)[1],
                "clean": validate_html_content(value)[2],
            }
            for value in HTML_CASES
        ],
        "validate_binary_data": [
            {
                "input": value,
                "is_valid": validate_binary_data(value)[0],
                "error": validate_binary_data(value)[1],
            }
            for value in BINARY_CASES
        ],
    }


def main(argv: list[str]) -> int:
    server = Path(__file__).resolve().parents[2]
    targets = [
        (server / "crates/plane-db/tests/golden/strip_tags.json", strip_corpus()),
        (
            server / "crates/plane-http-app/tests/golden/description-validators.json",
            validators_corpus(),
        ),
    ]
    failed = False
    for target, corpus in targets:
        target.parent.mkdir(parents=True, exist_ok=True)
        rendered = json.dumps(corpus, indent=1, ensure_ascii=False) + "\n"
        if "--check" in argv:
            current = target.read_text() if target.exists() else ""
            if current != rendered:
                print(f"stale: {target}", file=sys.stderr)
                failed = True
            else:
                print(f"ok: {target}")
            continue
        target.write_text(rendered)
        print(f"wrote {target} ({len(rendered)} bytes)")
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
