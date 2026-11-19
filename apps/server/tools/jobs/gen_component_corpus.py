#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Record `page_transaction`'s component parser for the Rust port.

Runs the real `extract_all_components` (`plane/bgtasks/page_transaction_task.py:41-68`)
on the same case list the Rust test uses and writes/checks
`crates/plane-tasks/tests/golden/page_components.json`.

Usage (from `apps/server/`, with the API virtualenv; no services needed —
`plane.settings.production` only reads the four variables below, and the
component parser is a pure function):

    apps/api/.venv/bin/python tools/jobs/gen_component_corpus.py
    apps/api/.venv/bin/python tools/jobs/gen_component_corpus.py --check

The interpreter and bs4 are pinned by `apps/api/.venv` (CPython 3.12.13); the
parser is `html.parser` and the attribute reader is `Tag.get`.
"""

import json
import os
import sys
from pathlib import Path

API = Path(__file__).resolve().parents[3] / "api"
sys.path.insert(0, str(API))
# A self-contained bootstrap, like the other generators: production settings
# read these four even though this script only calls a pure function, and the
# dummy values keep it from touching a service.
os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
os.environ.setdefault("SECRET_KEY", "component-corpus-not-a-secret")
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
os.environ.setdefault("ENABLE_DRF_SPECTACULAR", "0")

import django  # noqa: E402

django.setup()

from plane.bgtasks.page_transaction_task import extract_all_components  # noqa: E402

OUT = (
    Path(__file__).resolve().parents[2]
    / "crates/plane-tasks/tests/golden/page_components.json"
)

MENTION = "mention-component"
IMAGE = "image-component"

CASES = [
    "",
    "<p>plain text</p>",
    "<p></p>",
    "<mention-component></mention-component>",
    '<mention-component id="a"></mention-component>',
    '<mention-component id="a" entity_name="user" entity_identifier="b"></mention-component>',
    "<MENTION-COMPONENT id='a' ENTITY_NAME='user'></MENTION-COMPONENT>",
    '<mention-component id=a entity_name=user></mention-component>',
    '<mention-component id="a" id="b" entity_name="first" entity_name="last"></mention-component>',
    '<mention-component ID="a"></mention-component>',
    '<mention-component id="a&amp;b" entity_name="u&quot;x"></mention-component>',
    '<mention-component id="a&amp"></mention-component>',
    '<mention-component id="a&notit;"></mention-component>',
    '<mention-component id="&#65;&#x42;"></mention-component>',
    '<mention-component id="a" entity_name></mention-component>',
    "<mention-component id></mention-component>",
    '<mention-component id="a"/>',
    '<mention-component id="a" / >',
    '<mention-component id="a" />text',
    "<mention-component/id=\"a\">",
    "<mention-component\tid=\"a\"\nentity_name=\"u\"></mention-component>",
    '<mention-component id="a>b"></mention-component>',
    "<mention-component id='a\"b'></mention-component>",
    '<mention-component id=1 entity_identifier=2 ></mention-component>',
    '<mention-component id="a" entity_name="u?v"></mention-component>',
    '<mention-component id="a" entity_name="u&v"></mention-component>',
    '<mention-component id="a" entity_name="u v"></mention-component>',
    '<mention-component id="a" entity_name="user"',
    "<mention-component id='a\" entity_name='b'>",
    '<mention-componentx id="a"></mention-componentx>',
    '<mention-component- id="a"></mention-component->',
    "<script><mention-component id=\"a\"></mention-component></script>",
    "<SCRIPT><mention-component id=\"a\"></mention-component></SCRIPT>",
    "<style><mention-component id=\"a\"></mention-component></style>",
    "<textarea><mention-component id=\"a\"></mention-component></textarea>",
    "<title><mention-component id=\"a\"></mention-component></title>",
    "<noscript><mention-component id=\"a\"></mention-component></noscript>",
    '<script>if (a < b) { x("<mention-component id=\\"a\\">") }</script>'
    '<mention-component id="b"></mention-component>',
    '<!-- <mention-component id="a"> --><mention-component id="b"></mention-component>',
    '<!-- unterminated <mention-component id="a">',
    '<![CDATA[<mention-component id="a">]]><mention-component id="b"></mention-component>',
    '<?x <mention-component id="a"> ?><mention-component id="b"></mention-component>',
    '<!DOCTYPE html><mention-component id="a"></mention-component>',
    '<!x><mention-component id="a"></mention-component>',
    '<p>a < b</p><mention-component id="a"></mention-component>',
    '</><mention-component id="a"></mention-component>',
    '</ p><mention-component id="a"></mention-component>',
    "<p><mention-component id=\"a\"><image-component id=\"b\" src=\"/x/\"/></mention-component></p>",
    '<image-component id="a" src="https://e.test/x.png"></image-component>',
    '<image-component id="a" src>',
    '<image-component src="a" id="b"></image-component>',
    '<image-component id="a&amp;b" src="/api/assets/1?a=1&amp;b=2"/>',
    '<div><image-component id="a" src="/x/"></image-component></div>',
    '<image-component id="a" width="10" height="20" src="/x/"></image-component>',
    '<mention-component id="a" entity_name="user" entity_type="issue"></mention-component>',
    '<mention-component id="a" entity_identifier="uuid" entity_name="user" extra="x">'
    "</mention-component>",
    '<p>text</p>',
    'text<mention-component id="a"></mention-component>text',
    '<mention-component id="a"></mention-component >',
    '<mention-component\nid="a"\rentity_name="u"\x0cid="b"></mention-component>',
    '<mention-component id="\u00e9"></mention-component>',
    "<mention-component id='a&' entity_name='u'></mention-component>",
    '<mention-component data-id="a"></mention-component>',
    '<mention-component id="a" id></mention-component>',
    '<mention-component id= ></mention-component>',
    '<mention-component id==a></mention-component>',
    '<mention-component id="a" =b></mention-component>',
    "<mention-component id=a/b></mention-component>",
    '<mention-component id="a" /entity_name="u"></mention-component>',
    '<mention-component id="a" entity_name="u"/><mention-component id="b" entity_name="v">',
    '<mention-component id="a"></mention-component><image-component id="b" src="c">'
    '<mention-component id="d" entity_name="e"></mention-component>',
]


def normalise(entities):
    """The Rust shape: a map with only the attributes the tag actually has."""
    return [
        {"attributes": {key: value for key, value in entity.items() if value is not None}}
        for entity in entities
    ]


def build():
    cases = []
    for case in CASES:
        components = extract_all_components(case)
        cases.append(
            {
                "input": case,
                "mentions": normalise(components[MENTION]),
                "images": normalise(components[IMAGE]),
            }
        )
    return {"cases": cases}


def main() -> int:
    corpus = build()
    text = json.dumps(corpus, indent=1, ensure_ascii=False, sort_keys=True) + "\n"
    if "--check" in sys.argv:
        current = OUT.read_text()
        if current != text:
            print(f"{OUT} is stale; run tools/jobs/gen_component_corpus.py")
            return 1
        print(f"{OUT} is current ({len(corpus['cases'])} cases)")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text)
    print(f"wrote {OUT} ({len(corpus['cases'])} cases)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
