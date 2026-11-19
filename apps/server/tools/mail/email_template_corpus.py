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
  .venv/bin/python ../server/tools/mail/email_template_corpus.py --check

`--check` renders everything in memory and fails with a non-zero exit when the
committed corpus differs; it never writes the file (CI uses it).

The multipart case masks the random boundary (`<BOUNDARY>`), `Date:` and the
Message-ID local part, exactly as `email_message_corpus.py` does.
"""

import argparse
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
    {
        "name": "analytic_export",
        "template": "emails/exports/analytics.html",
        "context": {},
    },
    {
        "name": "project_addition",
        "template": "emails/notifications/project_addition.html",
        "context": {
            "project_name": "Alpha & Co",
            "workspace_name": "Acme",
            "email": "ada@example.com",
            "inviter_first_name": "Rex",
            "project_url": "https://plane.example.com/acme/projects/"
            "b0000000-0000-4000-8000-000000000011/issues",
        },
    },
    {
        "name": "workspace_invitation",
        "template": "emails/invitations/workspace_invitation.html",
        "context": {
            "email": "ada@example.com",
            "first_name": "Rex <Receiver>",
            "workspace_name": "Acme",
            "abs_url": "https://plane.example.com/workspace-invitations/"
            "?invitation_id=b0000000-0000-4000-8000-000000000030&slug=acme&token=t0k",
        },
    },
    {
        "name": "user_activation",
        "template": "emails/user/user_activation.html",
        "context": {"email": "ada@example.com", "profile_url": "https://plane.example.com/profile"},
    },
    {
        "name": "user_deactivation",
        "template": "emails/user/user_deactivation.html",
        "context": {"email": "ada@example.com", "login_url": "https://plane.example.com/login"},
    },
    {
        "name": "email_updated",
        "template": "emails/user/email_updated.html",
        "context": {"email": "ada@example.com"},
    },
    {
        "name": "webhook_deactivate",
        "template": "emails/notifications/webhook-deactivate.html",
        "context": {
            "email": "ada@example.com",
            "message": "Webhook https://hooks.example.com/plane?x=1 has been deactivated due to "
            "failed requests.",
            "webhook_url": "https://plane.example.com/acme/settings/webhooks/"
            "8a1b2c3d-4e5f-6789-abcd-ef0123456789",
        },
    },
    {
        "name": "issue_updates_fields",
        "template": "emails/notifications/issue-updates.html",
        "context": {
            "data": [
                {
                    "actor_detail": {
                        "avatar_url": "https://app.example.com/avatars/ada.png",
                        "first_name": "Ada",
                        "last_name": "Actor",
                    },
                    "changes": {
                        "state": {"old_value": ["Backlog"], "new_value": ["In Progress", "Done"]},
                        "priority": {"old_value": ["urgent"], "new_value": ["low"]},
                        "target_date": {"old_value": ["2026-01-05"], "new_value": []},
                        "labels": {
                            "old_value": ["bug"],
                            "new_value": ["backend", "regression", "urgent"],
                        },
                        "assignees": {"old_value": [], "new_value": ["Rex Receiver", "Avi Assignee"]},
                        "link": {"old_value": ["None"], "new_value": ["https://example.com/pr/1"]},
                        "blocking": {
                            "old_value": ["P8B-1", "P8B-2", "P8B-3"],
                            "new_value": ["P8B-4"],
                        },
                        "duplicate": {
                            "old_value": ["P8B-5", "P8B-6", "P8B-7"],
                            "new_value": ["P8B-8", "P8B-9", "P8B-10"],
                        },
                        "name": {"old_value": ["Old name"], "new_value": ["Webhook issue"]},
                    },
                    "issue_details": {"name": "Webhook issue", "identifier": "P8B-7"},
                    "activity_time": "09:05 AM",
                },
                {
                    "actor_detail": {
                        "avatar_url": "",
                        "first_name": "Rex",
                        "last_name": "Receiver",
                    },
                    "changes": {"status": {"old_value": ["Todo"], "new_value": ["Done"]}},
                    "issue_details": {"name": "Webhook issue", "identifier": "P8B-7"},
                    "activity_time": "11:59 PM",
                },
            ],
            "summary": "Updates were made to the issue by",
            "actors_involved": 2,
            "issue": {
                "issue_identifier": "P8B-7",
                "name": "Webhook issue",
                "issue_url": "https://app.example.com/p8b-ws/projects/"
                "b0000000-0000-4000-8000-000000000011/issues/"
                "b0000000-0000-4000-8000-000000000013",
            },
            "receiver": {"email": "receiver@example.com"},
            "issue_url": "https://app.example.com/p8b-ws/projects/"
            "b0000000-0000-4000-8000-000000000011/issues/"
            "b0000000-0000-4000-8000-000000000013",
            "project_url": "https://app.example.com/p8b-ws/projects/"
            "b0000000-0000-4000-8000-000000000011/issues/",
            "workspace": "p8b-ws",
            "project": "P8B project",
            "user_preference": "https://app.example.com/p8b-ws/settings/account/notifications/",
            "comments": [],
            "entity_type": "issue",
        },
    },
    {
        "name": "issue_updates_comments_and_mentions",
        "template": "emails/notifications/issue-updates.html",
        "context": {
            "data": [],
            "summary": "Updates were made to the issue by",
            "actors_involved": 1,
            "issue": {
                "issue_identifier": "P8B-7",
                "name": "Webhook issue",
                "issue_url": "https://app.example.com/p8b-ws/issues/1",
            },
            "receiver": {"email": "receiver@example.com"},
            "issue_url": "https://app.example.com/p8b-ws/issues/1",
            "project_url": "https://app.example.com/p8b-ws/issues/",
            "workspace": "p8b-ws",
            "project": "P8B project",
            "user_preference": "https://app.example.com/p8b-ws/settings/account/notifications/",
            "comments": [
                {
                    "actor_comments": {
                        "old_value": [],
                        "new_value": [
                            "<p>A comment with a <b>bold</b> word &amp; an entity.</p>",
                            "<p>Second line</p>",
                        ],
                    },
                    "actor_detail": {
                        "avatar_url": "https://app.example.com/avatars/rex.png",
                        "first_name": "Rex",
                        "last_name": "Receiver",
                    },
                },
                {
                    "actor_comments": {
                        "old_value": ["<p>old mention</p>"],
                        "new_value": ["<p>Hello @Ada Actor, see this</p>"],
                    },
                    "actor_detail": {
                        "avatar_url": "",
                        "first_name": "Avi",
                        "last_name": "Assignee",
                    },
                },
            ],
            "entity_type": "issue",
        },
    },
    {
        "name": "issue_updates_empty_names_escaped",
        "template": "emails/notifications/issue-updates.html",
        "context": {
            "data": [
                {
                    "actor_detail": {"avatar_url": "", "first_name": "", "last_name": ""},
                    "changes": {
                        "target_date": {"old_value": ["2026-01-05"], "new_value": ["2026-02-01"]},
                        "priority": {"old_value": ["low"], "new_value": ["urgent"]},
                    },
                    "issue_details": {"name": "a<b&c", "identifier": "P8B-7"},
                    "activity_time": "12:00 AM",
                }
            ],
            "summary": "Updates were made to the issue by",
            "actors_involved": 1,
            "issue": {
                "issue_identifier": "P8B-7",
                "name": "<script>alert('x')</script> & \"quotes\"",
                "issue_url": "https://app.example.com/p8b-ws/issues/1?x=\"'>",
            },
            "receiver": {"email": "a<b&c@example.com"},
            "issue_url": "https://app.example.com/p8b-ws/issues/1?x=\"'>",
            "project_url": "https://app.example.com/p8b-ws/issues/",
            "workspace": "p8b-ws",
            "project": "P8B <project>",
            "user_preference": "https://app.example.com/p8b-ws/settings/account/notifications/",
            "comments": [
                {
                    "actor_comments": {
                        "old_value": [],
                        "new_value": ["<p>one</p><b>two</b>&lt;three&gt;<!-- comment -->four"],
                    },
                    "actor_detail": {"avatar_url": "", "first_name": "Ada", "last_name": "Actor"},
                }
            ],
            "entity_type": "issue",
        },
    },
    {
        "name": "webhook_deactivate_escaped",
        "template": "emails/notifications/webhook-deactivate.html",
        "context": {
            "email": "a<b&c@example.com",
            "message": "Webhook <script> has been deactivated due to failed requests.",
            "webhook_url": "https://plane.example.com/acme/settings/webhooks/1?x=\"'>",
        },
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


def record_multipart():
    from django.core.mail import EmailMultiAlternatives

    message = EmailMultiAlternatives(
        subject="Your unique Plane login code is A1B2C3",
        body="\n\nHello from Plane\n\n",
        from_email="Team Plane <team@mailer.plane.so>",
        to=["ada@example.com"],
    )
    message.attach_alternative("<html><body><p>Hello from Plane</p></body></html>", "text/html")
    return mask_generated(message.message().as_bytes(linesep="\r\n").decode("utf-8"))


def record_webhook_multipart():
    """The deactivation e-mail: a 32 KB HTML alternative with 4 KB lines.

    The long lines make `SafeMIMEText` choose quoted-printable for the HTML
    part, which is the only case in the corpus that exercises
    `email.quoprimime.body_encode` (spaces stay literal, trailing spaces are
    quoted, lines wrap at 76 with a soft break).
    """
    from django.core.mail import EmailMultiAlternatives
    from django.template.loader import render_to_string

    from plane_utils_email import generate_plain_text_from_html

    html = render_to_string(
        "emails/notifications/webhook-deactivate.html",
        {
            "email": "receiver@example.com",
            "message": "Webhook http://127.0.0.1:1234/hook-one has been deactivated due to "
            "failed requests.",
            "webhook_url": "https://app.example.com/p8b-ws/settings/webhooks/"
            "b0000000-0000-4000-8000-000000000021",
        },
    )
    message = EmailMultiAlternatives(
        subject="Webhook Deactivated",
        body=generate_plain_text_from_html(html),
        from_email="Team Plane <team@mailer.plane.so>",
        to=["receiver@example.com"],
    )
    message.attach_alternative(html, "text/html")
    return mask_generated(message.message().as_bytes(linesep="\r\n").decode("utf-8"))


def record_attachment():
    """`analytic_plot_export.send_export_email`'s exact message shape."""
    from django.core.mail import EmailMultiAlternatives
    from django.template.loader import render_to_string

    from plane_utils_email import generate_plain_text_from_html

    html = render_to_string("emails/exports/analytics.html", {})
    message = EmailMultiAlternatives(
        subject="Your Export is ready",
        body=generate_plain_text_from_html(html),
        from_email="Team Plane <team@mailer.plane.so>",
        to=["ada@example.com"],
    )
    message.attach("p8c-export-analytics.csv", "Project Name,Identifier\r\nAlpha,ALP-1\r\n")
    return mask_generated(message.message().as_bytes(linesep="\r\n").decode("utf-8"))


def mask_generated(encoded: str) -> str:
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


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--check",
        action="store_true",
        help="compare the committed corpus with Django's output; never writes",
    )
    args = parser.parse_args(argv)
    configure()
    corpus = {
        "templates": record_templates(),
        "plain_text": record_plain_text(),
        "multipart": record_multipart(),
        "webhook_multipart": record_webhook_multipart(),
        "attachment": record_attachment(),
    }
    rendered = json.dumps(corpus, indent=2, ensure_ascii=False) + "\n"
    if args.check:
        existing = ""
        if os.path.exists(OUT_PATH):
            with open(OUT_PATH, encoding="utf-8") as handle:
                existing = handle.read()
        if existing != rendered:
            print(
                f"STALE {os.path.relpath(OUT_PATH, REPO_ROOT)}: the committed corpus "
                "does not match Django's current templates",
                file=sys.stderr,
            )
            return 1
        print(f"{os.path.relpath(OUT_PATH, REPO_ROOT)} is current", file=sys.stderr)
        return 0
    with open(OUT_PATH, "w", encoding="utf-8") as handle:
        handle.write(rendered)
    print(f"wrote {OUT_PATH}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
