# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record Django's `validate_email` and Plane's `contains_url` for a corpus.

The Rust ports in `crates/plane-http-app/src/write/email.rs` are unit-tested
against this file, the same way the datetime parser is. Regenerate from the
repository root with:

    apps/api/.venv/bin/python apps/server/tools/inventory/email_corpus.py

Every case is `{input, valid, contains_url}`. `valid` is
`django.core.validators.validate_email`'s boolean (it raises
`ValidationError` for an invalid address); `contains_url` is
`plane.utils.url.contains_url`.
"""

from __future__ import annotations

import json
import os
import sys

ROOT = os.path.dirname(
    os.path.dirname(
        os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    )
)
API = os.path.join(ROOT, "apps", "api")
sys.path.insert(0, API)
os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
os.environ.setdefault(
    "DATABASE_URL", "postgres://plane:plane@127.0.0.1:55432/plane_tpl"
)
# `plane.celery` builds a Redis client at import time; nothing connects.
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")

import django  # noqa: E402

django.setup()

from django.core.exceptions import ValidationError  # noqa: E402
from django.core.validators import validate_email  # noqa: E402

from plane.utils.url import contains_url  # noqa: E402

OUT = os.path.join(
    ROOT, "apps", "server", "crates", "plane-http-app", "tests", "golden",
    "email-validator.json",
)


def valid_email(value):
    try:
        validate_email(value)
    except ValidationError:
        return False
    return True


def email_cases():
    cases = [
        "",
        "a@b.co",
        "a@b.c",
        "a@b.co.uk",
        "user.name+tag@example.com",
        "USER@EXAMPLE.COM",
        "user@Example.Com",
        "user@example",
        "user@localhost",
        "user@LOCALHOST",
        "user@localhost.",
        "user@example..com",
        "user@.example.com",
        "user@-example.com",
        "user@example-.com",
        "user@exa_mple.com",
        "user@example.c0m",
        "user@example.toolongtld",
        "user@xn--bcher-kva.example",
        "user@bücher.example",
        "user@пример.рф",
        "user@例子.测试",
        '""@example.com',
        '" "@example.com',
        '"a b"@example.com',
        '"a\\"b"@example.com',
        '"a\\ b"@example.com',
        '.user@example.com',
        'user.@example.com',
        'us..er@example.com',
        'user@example.com ',
        ' user@example.com',
        'user @example.com',
        'user@ example.com',
        'user@@example.com',
        '@example.com',
        'user@',
        'user',
        'user@example.com\n',
        'user@example.com\t',
        'user@[127.0.0.1]',
        'user@[127.0.0.256]',
        'user@[::1]',
        'user@[::ffff:127.0.0.1]',
        'user@[2001:db8::1]',
        'user@[2001:db8::0001]',
        'user@[0001::]',
        'user@[1:2:3:4:5:6:7:8]',
        'user@[1:2:3:4:5:6:7:8:9]',
        'user@[:::1]',
        'user@[1::2::3]',
        'user@[fe80::1%25eth0]',
        'user@[ABCDEF::]',
        'user@[abc.def]',
        'user@[1.2.3.4.5]',
        'user@[01.2.3.4]',
        'user@[1.2.3.04]',
        'user@[255.255.255.255]',
        'user@[0.0.0.0]',
        'user@[999.1.1.1]',
        'user@[1:2:3:4:5:6:1.2.3.4]',
        'user@[1:2:3:4:5:6:7:1.2.3.4]',
        'user@' + "a" * 64 + ".com",
        'user@' + "a" * 63 + ".com",
        'user@' + "a" * 64 + "." + "b" * 63 + ".com",
        'u' * 64 + '@example.com',
        'u' * 65 + '@example.com',
        'u' * 300 + '@example.com',
        'u' * 307 + '@example.com',
        'u' * 308 + '@example.com',
        'u' * 309 + '@example.com',
        "user@sub.domain.example.co.uk",
        "user@-.com",
        "user@a-.com",
        "user@a.b-.com",
        "user@a.-b.com",
        "user@a.b.c-.com",
        "user@a.b.c.d.e.f.g.h",
        "user@a.b.c.d.e.f.g.123",
        "user@a.b.c.d.e.f.g.1a",
        "user@a.b.c.d.e.f.g.a1",
        "user@a.b.c.d.e.f.g.a-1",
        "user@a.b.c.d.e.f.g.-a1",
        "!#$%&'*+-/=?^_`{|}~@example.com",
        "a,b@example.com",
        "a:b@example.com",
        "a;b@example.com",
        "a(b)@example.com",
        "a[b]@example.com",
        "a\\b@example.com",
        "a\"b@example.com",
        "a<b@example.com",
        "a>b@example.com",
        "a b@example.com",
        "a\tb@example.com",
        "a\nb@example.com",
        "user@example.com.",
        "user@example.com..",
        "user@example.com-",
        "user@example-.com",
        "user@exa mple.com",
        "user@example.com/path",
        "user@example.com?q=1",
        "user@example.com#x",
        "user@[1:2:3:4:5:6:7:8]:25",
        "user@[::]",
        "user@[:::]",
        "user@[1::]",
        "user@[1:2:3:4:5:6:7::]",
        "user@[::1:2:3:4:5:6:7]",
        "user@[12345::]",
        "user@[1234::]",
        "user@[0:0:0:0:0:0:0:0]",
        "user@[ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff]",
        "user@[::ffff:1.2.3.4]",
        "user@[::ffff:01.2.3.4]",
        "user@[::ffff:1.2.3.4.5]",
        "user@[::ffff:999.2.3.4]",
        "user@[::ffff:1.2.3.04]",
        "user@[1:2:3:4:5:6:7:8].example.com",
    ]
    # Length boundaries either side of Django's 320-character limit.
    for length in (318, 319, 320, 321, 322):
        local = "a" * (length - len("@example.com"))
        cases.append(local + "@example.com")
    # Every Unicode letter range Django's hostname class allows (a sample).
    for label in ("Ünïcode", "日本語", "ελληνικά", "ру́сский", "עברית", "العربية"):
        cases.append("user@" + label + ".com")
    # A few control characters and non-BMP characters.
    cases.append("user@ex\u00e9mple.com")
    cases.append("user@ex\u0000mple.com")
    cases.append("user@ex\u2028mple.com")
    cases.append("user@ex\U0001f600mple.com")
    cases.append("us\U0001f600er@example.com")
    # Characters whose Unicode case folding is not ASCII: Python's
    # IGNORECASE and Rust's `(?i)` must agree on all of them.
    for value in (
        "user@\u0130.example.com",
        "user@\u017f.example.com",
        "user@\u212a.example.com",
        "user@\ufb00.example.com",
        "\u017f@example.com",
        "\u0130@example.com",
        "user@\u03c2.example.com",
        "user@\u03a3.example.com",
        "user@\u2126.example.com",
    ):
        cases.append(value)
    return cases


def url_cases():
    cases = [
        "",
        "hello",
        "hello world",
        "visit https://example.com now",
        "visit http://example.com/path?q=1#frag",
        "HTTPS://EXAMPLE.COM",
        "ftp://example.com",
        "xhttp://example.com",
        "https://",
        "http:// example.com",
        "www.example.com",
        "www.Example.COM",
        "www.example.com/path",
        "www.",
        "www..com",
        "www.a",
        "www.a.",
        "www.a.b",
        "www.-example.com",
        "www.example-.com",
        "www.exa_mple.com",
        "example.com",
        "EXAMPLE.COM",
        "sub.example.com",
        "example.toolongtld",
        "example.c",
        "example.co",
        "example.com.",
        "example.com,",
        "foo.example.com/bar",
        "1.2.3.4",
        "255.255.255.255",
        "256.1.1.1",
        "999.999.999.999",
        "01.2.3.4",
        "1.2.3.4.5",
        "1.2.3.4:8080",
        "::1",
        "http://[::1]",
        "http://127.0.0.1:8080/x",
        "a.b.c.d.e.f",
        "a.b.c.d.e.f.g.h.i",
        "a.b.c.d.e.f.gg",
        "a-1.b-2.com",
        "a--b.com",
        "a..b.com",
        ".a.com",
        "a.com.",
        "text https://example.com text www.foo.bar 1.2.3.4",
        "a" * 500 + " https://example.com",
        "a" * 500,
        "a" * 501 + " https://example.com",
        "a" * 1000,
        "a" * 1001,
        "a" * 1001 + " https://example.com",
        ("a" * 501 + "\nhttps://example.com"),
        ("https://example.com\n" + "a" * 1001),
        ("https://example.com\n" + "a" * 900),
        "ü example.com",
        "é.example.com",
        "straße.de",
        "http://例え.テスト",
        "www.bücher.de",
        "www.例子.测试",
        "see https://例子.测试/x",
        "email user@example.com",
        "user@example.com",
        "a.b",
        "a.bc",
        "a.123",
        "a.12",
        "12.34.56.78",
        "1.2.3.4.",
        "http://user:pass@example.com/x",
        "https://example.com/<script>",
        "//example.com",
        "example.com:443",
        "localhost",
        "localhost:8000",
        "http://localhost:8000",
        "www.localhost",
        "text with a newline\nwww.example.com",
        "line one\r\nwww.example.com",
        "line one\rwww.example.com",
        "line one\u2028www.example.com",
        "line one\u000bwww.example.com",
        "line one\u00a0www.example.com",
        "www.example.com." + "x" * 480,
    ]
    for value in (
        "\u017f.example.com",
        "\u212a.example.com",
        "\u0130.example.com",
        "www.\u017f.com",
        "www.\u212a.com",
        "\u03c2.example.com",
        "https://\u017f.example.com",
        "https://\u0130.example.com",
    ):
        cases.append(value)
    return cases


def main():
    cases = []
    seen = set()
    for value in email_cases():
        if value in seen:
            continue
        seen.add(value)
        cases.append(
            {"input": value, "valid": valid_email(value), "contains_url": contains_url(value)}
        )
    for value in url_cases():
        if value in seen:
            continue
        seen.add(value)
        cases.append(
            {"input": value, "valid": valid_email(value), "contains_url": contains_url(value)}
        )
    document = {
        "python": sys.version.split()[0],
        "django": django.get_version(),
        "cases": cases,
    }
    with open(OUT, "w", encoding="utf-8") as handle:
        json.dump(document, handle, ensure_ascii=False, indent=1)
        handle.write("\n")
    print(f"wrote {len(cases)} cases to {OUT}")


if __name__ == "__main__":
    main()
