#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record Django's email bytes for the SMTP port (`PUBLIC_INSTANCES.md` §2c).

`plane/license/api/views/configuration.py:85-171` (`EmailCredentialCheckEndpoint`)
sends one `EmailMultiAlternatives` through Django's SMTP backend, so the Rust
port must produce the same RFC 5322 bytes Django's `message().as_bytes()` does
and open the connection the same way (`django/core/mail/backends/smtp.py:70-186`).

This script records, from the real Django in `apps/api/.venv`:

* `messages` -- for each case, the exact `EmailMultiAlternatives.message()
  .as_bytes(linesep="\\r\\n")` (what `sendmail` puts on the wire). `Date` and
  `Message-ID` are the only volatile headers; their values are replaced by
  `<DATE>` and `<MESSAGE-ID>` in the header block, and the Rust builder takes
  them as arguments so a test can pass the same placeholders.
* `envelopes` -- the `(from, recipients)` smtplib puts in `MAIL FROM:`/`RCPT
  TO:`: `sanitize_address(email_message.from_email)` and one per recipient
  (`django/core/mail/backends/smtp.py:166-175`), plus `quoteaddr`'s addr-spec
  (`smtplib.py:545,553`).

Regenerate from the repository root:

    apps/api/.venv/bin/python apps/server/tools/mail/email_message_corpus.py

`--check` re-renders into memory and fails on any difference, so a Django or
`email` package change cannot silently invalidate the Rust builder's golden.

The output is `crates/plane-mail/tests/golden/email_messages.json`.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve()
SERVER_ROOT = HERE.parents[2]  # apps/server
REPO_ROOT = HERE.parents[4]  # repository root
API_ROOT = REPO_ROOT / "apps" / "api"

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
os.environ.setdefault("SECRET_KEY", "email-corpus-not-a-secret")
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
os.environ.setdefault("ENABLE_DRF_SPECTACULAR", "0")

sys.path.insert(0, str(API_ROOT))

import django  # noqa: E402

django.setup()

from django.conf import settings  # noqa: E402
from django.core.mail import EmailMultiAlternatives  # noqa: E402
from django.core.mail import message as mail_message  # noqa: E402
from django.core.mail.message import BadHeaderError, sanitize_address  # noqa: E402
from smtplib import quoteaddr  # noqa: E402

OUT = (
    SERVER_ROOT
    / "crates"
    / "plane-mail"
    / "tests"
    / "golden"
    / "email_messages.json"
)

# `plane/license/api/views/configuration.py:102-118`: the endpoint's fixed
# subject/body and the instance-configured From.
ENDPOINT_SUBJECT = "Email Notification from Plane"
ENDPOINT_BODY = "This is a sample email notification sent from Plane application."
ENDPOINT_FROM = "Team Plane <team@mailer.plane.so>"

# The seven values `get_email_configuration` returns
# (`plane/license/utils/instance_value.py:42-59`).
CONFIGURATION = {
    "EMAIL_HOST": "127.0.0.1",
    "EMAIL_HOST_USER": None,
    "EMAIL_HOST_PASSWORD": None,
    "EMAIL_PORT": 587,
    "EMAIL_USE_TLS": "1",
    "EMAIL_USE_SSL": "0",
    "EMAIL_FROM": ENDPOINT_FROM,
}

DATE_RE = re.compile(rb"(?m)^Date:[^\r\n]*(?:\r?\n[ \t][^\r\n]*)*")
# A generated `Message-ID` may be folded onto continuation lines: Python's
# header folder breaks after `Message-ID: ` when the value does not fit in 78
# columns, and the value's length depends on the FQDN and the random digits
# (`email/utils.py:make_msgid`). Match the header and every continuation line.
MESSAGE_ID_RE = re.compile(rb"(?m)^Message-ID:[^\r\n]*(?:\r?\n[ \t][^\r\n]*)*")


class Case:
    def __init__(self, name, message_id=None, **kwargs):
        self.name = name
        # A fixed Message-ID for the folding case: the generator patches
        # `make_msgid` so the corpus pins Python's fold byte for byte.
        self.message_id = message_id
        self.kwargs = kwargs


def cases() -> list[Case]:
    base = dict(
        subject=ENDPOINT_SUBJECT,
        body=ENDPOINT_BODY,
        from_email=ENDPOINT_FROM,
        to=["receiver@example.com"],
    )
    return [
        # The endpoint's own message: fixed subject and body, configured From,
        # one recipient from the request body.
        Case("endpoint", **base),
        Case("endpoint_two_recipients", **{**base, "to": ["a@example.com", "b@example.com"]}),
        Case("endpoint_default_from", **{**base, "from_email": ""}),
        Case("endpoint_display_name_from", **{**base, "from_email": "Plane <hello@plane.so>"}),
        # Body transfer encodings: 7bit (ASCII), 8bit (non-ASCII), QP (a line
        # longer than 998 octets, `SafeMIMEText.set_payload`,
        # `django/core/mail/message.py:172-183`).
        Case("endpoint_utf8_body", **{**base, "body": "Héllo wörld — ünïcode"}),
        Case("endpoint_multiline_body", **{**base, "body": "line one\nline two\n"}),
        Case("endpoint_empty_body", **{**base, "body": ""}),
        Case("endpoint_long_ascii_line", **{**base, "body": "x" * 1200}),
        Case("endpoint_long_utf8_line", **{**base, "body": "ä" * 600}),
        # Header encodings: a UTF-8 subject becomes an RFC 2047 base64 encoded
        # word (`forbid_multi_line_headers`, `django/core/mail/message.py:67-90`).
        Case("endpoint_utf8_subject", **{**base, "subject": "Grüße aus Berlin"}),
        Case("endpoint_utf8_display_from", **{**base, "from_email": "Grüße <hello@plane.so>"}),
        # Long ASCII headers fold at 78 columns
        # (`email/header.py:_ValueFormatter`); a long address list folds at the
        # separators.
        Case("endpoint_long_subject_word", **{**base, "subject": "A" * 100}),
        Case("endpoint_long_subject_words", **{**base, "subject": "word " * 30}),
        # A long, fixed Message-ID folds after the header name
        # (`Message-ID: ` + CRLF + `' '` + value). The generated one normally
        # fits, but its length depends on `socket.getfqdn()` and
        # `make_msgid`'s random digits, so this case pins the fold.
        Case(
            "endpoint_long_message_id",
            message_id=(
                "<179502453307.68922.11840213799578878381"
                "@mailer.harness.very-long-hostname.example.test>"
            ),
            **base,
        ),
        Case(
            "endpoint_long_recipients",
            **{
                **base,
                "to": [
                    "first.recipient@example.com",
                    "second.recipient@example.com",
                    "third.recipient@example.com",
                ],
            },
        ),
        # Cc and Reply-To are only emitted when non-empty
        # (`EmailMessage.message`), and in this order.
        Case(
            "endpoint_cc_reply_to",
            **{**base, "cc": ["c@example.com"], "reply_to": ["reply@example.com"]},
        ),
        # A body line starting with "From " must not be mangle_from_-escaped
        # (Django's `MIMEMixin.as_bytes`), and a body line starting with "." is
        # smtplib's problem, not the message's.
        Case("endpoint_from_line_body", **{**base, "body": "From the desk of\n.dot line"}),
        # A newline in a header is BadHeaderError, caught by the endpoint
        # (`configuration.py:125-127`).
        Case("endpoint_newline_subject", **{**base, "subject": "a\nb"}),
        Case("endpoint_newline_recipient", **{**base, "to": ["a@example.com\nBcc: c@example.com"]}),
    ]


def mask(headers_and_body: bytes, keep_message_id: bool) -> bytes:
    head, separator, body = headers_and_body.partition(b"\r\n\r\n")
    head = DATE_RE.sub(b"Date: <DATE>", head)
    if not keep_message_id:
        head = MESSAGE_ID_RE.sub(b"Message-ID: <MESSAGE-ID>", head)
    return head + separator + body


def message_case(case: Case) -> dict:
    record = {
        "name": case.name,
        "subject": case.kwargs["subject"],
        "body": case.kwargs["body"],
        "from_email": case.kwargs["from_email"],
        "to": case.kwargs.get("to", []),
        "cc": case.kwargs.get("cc", []),
        "reply_to": case.kwargs.get("reply_to", []),
        "message_id": case.message_id,
    }
    message = EmailMultiAlternatives(**case.kwargs)
    try:
        if case.message_id is None:
            rendered = mask(message.message().as_bytes(linesep="\r\n"), keep_message_id=False)
        else:
            # `django.core.mail.message` imported `make_msgid` directly, so the
            # patch has to go through that module's name.
            original = mail_message.make_msgid
            fixed = case.message_id

            def make_fixed(domain=None):
                return fixed

            mail_message.make_msgid = make_fixed
            try:
                rendered = mask(
                    message.message().as_bytes(linesep="\r\n"),
                    keep_message_id=True,
                )
            finally:
                mail_message.make_msgid = original
    except BadHeaderError as error:
        record["error"] = "bad_header"
        record["error_message"] = str(error)
        return record
    record["bytes"] = rendered.decode("utf-8")
    return record


ENVELOPE_ADDRESSES = [
    ENDPOINT_FROM,
    "",
    "receiver@example.com",
    "Plane <hello@plane.so>",
    "Grüße <hello@plane.so>",
    '"Doe, Jane" <jane@example.com>',
    "user.name+tag@example.com",
]


def envelope_case(address: str) -> dict:
    try:
        sanitized = sanitize_address(address, settings.DEFAULT_CHARSET)
    except ValueError as error:
        return {"input": address, "error": "invalid", "error_message": str(error)}
    return {"input": address, "sanitized": sanitized, "quoteaddr": quoteaddr(sanitized)}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    document = {
        "generated_by": "apps/server/tools/mail/email_message_corpus.py",
        "django_version": django.get_version(),
        "default_from_email": settings.DEFAULT_FROM_EMAIL,
        "default_charset": settings.DEFAULT_CHARSET,
        "configuration": CONFIGURATION,
        "endpoint": {
            "subject": ENDPOINT_SUBJECT,
            "body": ENDPOINT_BODY,
            "from_email": ENDPOINT_FROM,
        },
        "messages": [message_case(case) for case in cases()],
        "envelopes": [envelope_case(address) for address in ENVELOPE_ADDRESSES],
    }
    rendered = json.dumps(document, indent=2, ensure_ascii=False) + "\n"
    if args.check:
        existing = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if existing != rendered:
            print(
                f"STALE {OUT.relative_to(REPO_ROOT)}: the committed corpus does not "
                "match Django's current output",
                file=sys.stderr,
            )
            return 1
        print(f"ok    {OUT.relative_to(REPO_ROOT)}: {len(document['messages'])} messages")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(rendered, encoding="utf-8")
    print(f"wrote {OUT.relative_to(REPO_ROOT)}: {len(document['messages'])} messages")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
