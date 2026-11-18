#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record Django's rendered e-mail templates and plain-text bodies.

The Rust port (`crates/plane-mail`) renders the checked-in `.j2` copies with
minijinja and must reproduce `render_to_string` and
`generate_plain_text_from_html` byte for byte. This script is the oracle: it
runs the real Django template engine with a configured settings object (no
database, no Redis) and writes:

  crates/plane-mail/tests/golden/email_templates.json

Usage (from apps/api, where the virtualenv lives):

  .venv/bin/python ../server/tools/mail/email_template_corpus.py

The multipart case masks the random boundary (`<BOUNDARY>`), `Date:` and the
Message-ID local part, exactly as `email_message_corpus.py` does.
"""

import json
import os
import sys

import django
from django.conf import settings

REPO_ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", ".."))
API_DIR = os.path.join(REPO_ROOT, "apps", "api")
OUT_PATH = os.path.join(
    REPO_ROOT, "apps", "server", "crates", "plane-mail", "tests", "golden", "email_templates.json"
)

TEMPLATE_CASES = [
    {
        "name": "forgot_password",
        "template": "emails/auth/forgot_password.html",
        "context": {
            "first_name": "Ada",
            "forgot_password_url": "https://plane.example.com/accounts/reset-password/"
            "?uidb64=MQ&token=abc123&email=ada%40example.com",
            "email": "ada@example.com",
        },
    },
    {
        "name": "magic_signin",
        "template": "emails/auth/magic_signin.html",
        "context": {"code": "A1B2C3", "email": "ada@example.com"},
    },
    {
        "name": "magic_signin_escaped",
        "template": "emails/auth/magic_signin.html",
        "context": {"code": "<&\"'>", "email": "a<b&c@example.com"},
    },
]

PLAIN_TEXT_CASES = [
    {
        "name": "styles_comments_and_tags",
        "html": "<style>p { color: red }</style><!-- c --><p>Hello &amp; bye</p>",
    },
    {
        "name": "script_is_data",
        "html": "<script>if (a < b) x();</script>done",
    },
    {
        "name": "double_unescape",
        "html": "<p>&amp;lt;</p>",
    },
    {
        "name": "whitespace_collapse",
        "html": "<div>one</div>\n\n\n\n<div>two</div>\n  \n\tthree",
    },
    {
        "name": "stray_angle_brackets",
        "html": "a < b > c",
    },
]


def configure():
    # `plane` lives in apps/api, which is the working directory the documented
    # command uses; add it explicitly so the script also works from the repo root.
    if API_DIR not in sys.path:
        sys.path.insert(0, API_DIR)
    settings.configure(
        DEBUG=False,
        TEMPLATES=[
            {
                "BACKEND": "django.template.backends.django.DjangoTemplates",
                "DIRS": [os.path.join(API_DIR, "templates")],
                "APP_DIRS": False,
                "OPTIONS": {},
            }
        ],
        INSTALLED_APPS=[],
        USE_TZ=True,
        TIME_ZONE="UTC",
        EMAIL_USE_LOCALTIME=False,
        DEFAULT_CHARSET="utf-8",
    )
    django.setup()
    load_plane_email_module()


def load_plane_email_module():
    """Load `plane/utils/email.py` without importing the `plane` package.

    `plane/__init__.py` imports `plane.celery`, which builds a Redis client at
    import time. This script only needs `generate_plain_text_from_html`, whose
    imports are `html`, `re` and `django.utils.html.strip_tags`.
    """
    import importlib.util

    path = os.path.join(API_DIR, "plane", "utils", "email.py")
    spec = importlib.util.spec_from_file_location("plane_utils_email", path)
    module = importlib.util.module_from_spec(spec)
    sys.modules["plane_utils_email"] = module
    spec.loader.exec_module(module)
    return module


def record_templates():
    from django.template.loader import render_to_string

    from plane_utils_email import generate_plain_text_from_html

    cases = []
    for case in TEMPLATE_CASES:
        html = render_to_string(case["template"], case["context"])
        cases.append(
            {
                "name": case["name"],
                "template": case["template"],
                "context": case["context"],
                "html": html,
                "text": generate_plain_text_from_html(html),
            }
        )
    return cases


def record_plain_text():
    from plane_utils_email import generate_plain_text_from_html

    return [
        {
            "name": case["name"],
            "html": case["html"],
            "text": generate_plain_text_from_html(case["html"]),
        }
        for case in PLAIN_TEXT_CASES
    ]


def record_multipart():
    from django.core.mail import EmailMultiAlternatives

    message = EmailMultiAlternatives(
        subject="Your unique Plane login code is A1B2C3",
        body="\n\nHello from Plane\n\n",
        from_email="Team Plane <team@mailer.plane.so>",
        to=["ada@example.com"],
    )
    message.attach_alternative("<html><body><p>Hello from Plane</p></body></html>", "text/html")
    encoded = message.message().as_bytes(linesep="\r\n").decode("utf-8")
    # The boundary is random; replace every occurrence including the header.
    start = encoded.index('boundary="') + len('boundary="')
    end = encoded.index('"', start)
    boundary = encoded[start:end]
    encoded = encoded.replace(boundary, "<BOUNDARY-0000000000000000000000000>")
    # Date and the Message-ID local part are generated per send. The
    # Message-ID can be folded across two lines by Python's header writer.
    lines = []
    skip_continuation = False
    for line in encoded.split("\r\n"):
        if skip_continuation and line.startswith(" "):
            continue
        skip_continuation = False
        if line.startswith("Date: "):
            lines.append("Date: <DATE>")
        elif line.startswith("Message-ID:"):
            lines.append("Message-ID: <MESSAGE-ID>")
            skip_continuation = True
        else:
            lines.append(line)
    return "\r\n".join(lines)


def main():
    configure()
    corpus = {
        "templates": record_templates(),
        "plain_text": record_plain_text(),
        "multipart": record_multipart(),
    }
    with open(OUT_PATH, "w", encoding="utf-8") as handle:
        json.dump(corpus, handle, indent=2, ensure_ascii=False)
        handle.write("\n")
    print(f"wrote {OUT_PATH}", file=sys.stderr)


if __name__ == "__main__":
    main()
