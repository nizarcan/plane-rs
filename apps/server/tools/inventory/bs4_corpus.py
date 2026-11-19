#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record `str(BeautifulSoup(html, "html.parser"))` for the Rust port.

`plane/bgtasks/copy_s3_object.py:60-70` rewrites the `src` of every
`<image-component>` and then serialises the whole soup; the bytes it writes to
`description_html` are `str(soup)`. The port in `crates/plane-tasks/src/bs4.rs`
has to reproduce bs4 4.12.3's `html.parser` tree builder and its `"minimal"`
formatter exactly, so the oracle is the real bs4 in `apps/api/.venv`.

    apps/api/.venv/bin/python apps/server/tools/inventory/bs4_corpus.py
    apps/api/.venv/bin/python apps/server/tools/inventory/bs4_corpus.py --check

`--check` re-records the corpus in memory and compares it with the committed
file, exiting non-zero when it is stale (CI runs it). The output is
`apps/server/crates/plane-tasks/tests/golden/bs4_html.json`, read by
`crates/plane-tasks/tests/bs4_golden.rs`.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import pathlib
import sys

SERVER = pathlib.Path(__file__).resolve().parents[2]

import bs4  # noqa: E402
from bs4 import BeautifulSoup  # noqa: E402

# Load `plane/utils/html_processor.py` by path: importing the `plane` package
# boots celery and needs the dev environment, which this generator must not.
_HTML_PROCESSOR = SERVER.parent / "api" / "plane" / "utils" / "html_processor.py"
_spec = importlib.util.spec_from_file_location("plane_html_processor", _HTML_PROCESSOR)
_html_processor = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_html_processor)
strip_tags = _html_processor.strip_tags

OUT = SERVER / "crates" / "plane-tasks" / "tests" / "golden" / "bs4_html.json"

# The editor's own markup plus the shapes `html.parser`/bs4 treat specially.
# Every case is `name -> html`; the recorder stores the exact output.
CASES: dict[str, str] = {
    "editor-paragraph": "<p>Hello <strong>world</strong> and <em>everyone</em>.</p>",
    "editor-empty-paragraph": "<p></p>",
    "editor-mention": (
        '<p>Hi <mention-component id="f0000000-0000-4000-8000-000000000001" '
        'name="Ada Admin"></mention-component>, please look.</p>'
    ),
    "editor-mention-data": (
        '<p><mention-component id="f0000000-0000-4000-8000-000000000002" '
        'entity-name="issue" entity-identifier="ALP-12">#12</mention-component></p>'
    ),
    "editor-image": (
        '<p><image-component src="f0000000-0000-4000-8000-000000000041" '
        'width="640" height="480" aspect-ratio="1.333"></image-component></p>'
    ),
    "editor-image-external": (
        '<p><image-component src="https://example.com/pic.png" '
        'alt="a picture"></image-component></p>'
    ),
    "editor-image-self-closing": (
        '<image-component src="f0000000-0000-4000-8000-000000000042"/>'
    ),
    "editor-image-nested-in-list": (
        "<ul><li>one<image-component src=\"f0000000-0000-4000-8000-000000000043\">"
        "</image-component></li></ul>"
    ),
    "editor-headings": "<h1>Title</h1><h2>Sub &amp; more</h2><h3>Third</h3>",
    "editor-table": (
        "<table><tbody>"
        "<tr><th>Name</th><th>Value</th></tr>"
        "<tr><td>alpha</td><td>1 &amp; 2</td></tr>"
        "</tbody></table>"
    ),
    "editor-table-no-tbody": "<table><tr><td>a<td>b</table>",
    "editor-code-block": (
        '<pre><code class="language-python">if a &lt; b:\n    print("hi")\n</code></pre>'
    ),
    "editor-inline-code": "<p>Use <code>a &amp;&amp; b</code> here.</p>",
    "editor-nested-lists": (
        "<ol><li>first<ul><li>nested a</li><li>nested b<ol><li>deep</li></ol></li>"
        "</ul></li><li>second</li></ol>"
    ),
    "editor-blockquote-hr": "<blockquote><p>quoted</p></blockquote><hr><p>after</p>",
    "editor-link": (
        '<p><a href="https://example.com/a?x=1&amp;y=2#frag" target="_blank" '
        'rel="noopener noreferrer">link</a></p>'
    ),
    "editor-break": "<p>line one<br>line two<br/>line three</p>",
    "editor-unicode": "<p>emoji 😀, dashes — and non-breaking\u00a0space</p>",
    "editor-entities": "<p>&copy; &lt;tag&gt; &amp;amp; &#147; &#x1F600; &nbsp;</p>",
    "editor-styled": (
        '<p><span style="color: #ff0000; font-weight: bold;">red</span>'
        '<span class="a  b">spaced</span></p>'
    ),
    "editor-full-document": (
        "<!DOCTYPE html><html><head><meta charset=\"utf8\"><title>Doc</title></head>"
        "<body><p>body</p></body></html>"
    ),
    "editor-script-style": (
        "<script>if (a < b && c) { x(\"</div>\") }</script>"
        "<style>a > b { color: red }</style>"
    ),
    "editor-pre-whitespace": "<pre>  x\n  y </pre><textarea>\n  z\n</textarea>",
    "malformed-unclosed": "<div><p>one<p>two",
    "malformed-stray-end": "</p><p>x</p></div></p>y",
    "malformed-misnested": "<p><b>x</p>y</b>z",
    "malformed-nested-li": "<ul><li>a<li>b</ul>",
    "malformed-incomplete-tag": "<p>x</p",
    "malformed-incomplete-comment": "<!-- unterminated",
    "malformed-incomplete-entity": "<p>&amp",
    "malformed-bare-ampersand": "<p>a & b</p>",
    "malformed-lt": "<p>a < b and <3</p>",
    "malformed-bogus-comment": "<p><!--not -- a --></p><!--a--!>",
    "malformed-declaration": "<!DOCTYPE><p>x</p>",
    "malformed-doctype": "<!doctype HTML PUBLIC \"-//W3C//DTD HTML 4.01//EN\">x",
    "malformed-cdata": "<![CDATA[x<y]]>",
    "malformed-pi": "<?xml version=\"1.0\"?><p>x</p>",
    "malformed-attr-quotes": (
        "<p title='he said \"hi\"' data-x=\"a'b\" empty=\"\">q</p>"
    ),
    "malformed-attr-entities": "<p title=\"a&amp;b<c>d\" alt=\"x &quot;y&quot;\">z</p>",
    "malformed-attr-duplicate": '<p a="1" A="2" b>x</p>',
    "malformed-attr-unquoted": "<p a=1 b=two/>three c = 4>x</p>",
    "malformed-uppercase": '<P CLASS="A"><B>x</B></P>',
    "malformed-self-closing-div": "<div/><span/>tail",
    "malformed-void-close": "<p>a<br></br>b<img src=\"u\"></img>c</p>",
    "malformed-text-only": "plain text only",
    "malformed-empty": "",
    "malformed-whitespace": "  <p>x</p>  ",
    "malformed-newlines": "<div>\n <p>x</p>\n</div>",
    "malformed-null": "<p>\x00x</p>",
    "malformed-html-comment-whitespace": "<p><!--   --></p>",
    "editor-title": "<title>a &amp; b < c</title><p>x</p>",
    "editor-textarea-entities": "<textarea>a &lt; b &amp; c</textarea>",
    "editor-mention-in-table": (
        "<table><tbody><tr><td><mention-component id=\"f0000000-0000-4000-8000-000000000003\" "
        "name=\"Mia\"></mention-component></td></tr></tbody></table>"
    ),
    "malformed-template": "<template><p>x</p></template>",
    "malformed-unterminated-attr": "<p a=\"x>y</p>",
    "malformed-bare-attr-end": "<p a=>x</p>",
    "malformed-slash-space": "<p/ >x</p>",
    "malformed-two-voids": "<p>a<br><br></br>b</p>",
    "malformed-comment-abrupt": "<!---><p>x</p>",
    "malformed-lt-eof": "<p>x<",
    "malformed-amp-eof": "<p>&a",
    "malformed-xmp": "<xmp><b>x</b></xmp>",
    "malformed-nested-script": "<script>var a = '</div>';</script><p>x</p>",
    "malformed-uppercase-doctype": "<!DoCtYpE html><p>x</p>",
    "malformed-attr-newline": "<p\na=\"1\"\nb=2>x</p>",
    "malformed-attr-amp-eof": "<p title=\"a&amp\">x</p>",
}


def record() -> dict:
    cases = []
    for name, html in CASES.items():
        output = str(BeautifulSoup(html, "html.parser"))
        cases.append(
            {
                "name": name,
                "html": html,
                "output": output,
                # `Page.save`/`Issue.save` write this to description_stripped.
                "stripped": strip_tags(html),
            }
        )
    return {
        "bs4": bs4.__version__,
        "python": sys.version.split()[0],
        "cases": cases,
    }


def render(corpus: dict) -> str:
    return json.dumps(corpus, indent=2, ensure_ascii=False) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail when the committed corpus differs from a fresh recording",
    )
    args = parser.parse_args()
    corpus = record()
    text = render(corpus)
    if args.check:
        current = OUT.read_text() if OUT.exists() else ""
        if current != text:
            print(f"{OUT} is stale; rerun without --check", file=sys.stderr)
            return 1
        print(f"{OUT}: {len(corpus['cases'])} cases up to date")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text)
    print(f"{OUT}: recorded {len(corpus['cases'])} cases")
    return 0


if __name__ == "__main__":
    sys.exit(main())
