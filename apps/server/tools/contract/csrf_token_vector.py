# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""CSRF masked-token vectors for the Rust port.

Django 5.2.17, ``django/middleware/csrf.py``:

    CSRF_SECRET_LENGTH = 32
    CSRF_TOKEN_LENGTH  = 64
    CSRF_ALLOWED_CHARS = string.ascii_letters + string.digits   # 62 chars
    mask               = get_random_string(32, CSRF_ALLOWED_CHARS)
    cipher[i]          = chars[(chars.index(secret[i]) + chars.index(mask[i])) % 62]
    masked             = mask + cipher                          # 64 chars

Unmasking adds nothing new: ``secret[i] = chars[x - y]`` with ``x = index(token[32+i])``,
``y = index(mask[i])`` and Python's negative-index wrap.  Note the alphabet is
``A-Z a-z 0-9`` (Python's ``string.ascii_letters + string.digits``), *not* base64 and
*not* ``base64.b64encode``-style.

Run from ``apps/api``:

    .venv/bin/python ../server/tools/contract/csrf_token_vector.py
"""

import os
import string
import sys
from unittest import mock

TOOLS_DIR = os.path.dirname(os.path.abspath(__file__))
# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, os.path.join(TOOLS_DIR, "..", "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_DIR = str(_oracle_api_dir())
sys.path.insert(0, API_DIR)

os.environ["DJANGO_SETTINGS_MODULE"] = "plane.settings.production"
os.environ.setdefault("SECRET_KEY", "contract-test-secret-key-not-for-deployment")
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")

import django  # noqa: E402

django.setup()

from django.middleware import csrf  # noqa: E402

CHARS = string.ascii_letters + string.digits

# Hand-chosen to exercise the modulo wrap and both letter cases.
SECRET = "0123456789abcdefghijklmnopqrstuv"  # 32 chars
MASK = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef"  # 32 chars


def mask_with(secret, mask):
    pairs = zip((CHARS.index(x) for x in secret), (CHARS.index(x) for x in mask))
    cipher = "".join(CHARS[(x + y) % len(CHARS)] for x, y in pairs)
    return mask + cipher


def unmask(token):
    mask = token[: csrf.CSRF_SECRET_LENGTH]
    rest = token[csrf.CSRF_SECRET_LENGTH :]
    pairs = zip((CHARS.index(x) for x in rest), (CHARS.index(x) for x in mask))
    return "".join(CHARS[x - y] for x, y in pairs)


def main():
    print("Django", django.get_version())
    print("CSRF_SECRET_LENGTH =", csrf.CSRF_SECRET_LENGTH)
    print("CSRF_TOKEN_LENGTH  =", csrf.CSRF_TOKEN_LENGTH)
    print("CSRF_ALLOWED_CHARS =", CHARS, "(len", len(CHARS), ")")
    print("CSRF_HEADER_NAME   =", __import__("django.conf", fromlist=["settings"]).settings.CSRF_HEADER_NAME)
    print()

    expected = mask_with(SECRET, MASK)

    # Django's own implementation, with the RNG pinned to MASK.
    with mock.patch.object(csrf, "get_random_string", lambda n, allowed_chars=None: MASK):
        actual = csrf._mask_cipher_secret(SECRET)

    print("VECTOR 1 (deterministic)")
    print("  secret =", SECRET)
    print("  mask   =", MASK)
    print("  masked =", actual)
    print("  hand-rolled masked =", expected)
    print("  MATCH:", actual == expected, "len:", len(actual))
    print("  django _unmask_cipher_token(masked) =", csrf._unmask_cipher_token(actual))
    print("  hand-rolled unmask(masked)          =", unmask(actual))
    print("  recovered == secret:", csrf._unmask_cipher_token(actual) == SECRET)
    print()

    # Unmasked 32-char tokens are still accepted (Django <4.0 cookies) and the fast
    # path in _does_token_match only unmasked-matches on length 64.
    print("  _does_token_match(masked, secret)   =", csrf._does_token_match(actual, SECRET))
    print("  _does_token_match(secret, secret)   =", csrf._does_token_match(SECRET, SECRET))
    print("  _does_token_match(secret, other)    =", csrf._does_token_match("0" * 32, SECRET))
    print()

    print("VECTOR 2 (random, produced by django)")
    real_secret = csrf._get_new_csrf_string()
    real_masked = csrf._mask_cipher_secret(real_secret)
    print("  secret =", real_secret)
    print("  masked =", real_masked)
    print("  unmask =", csrf._unmask_cipher_token(real_masked))
    print("  match  =", csrf._unmask_cipher_token(real_masked) == real_secret)
    print()

    print("Format checks (_check_token_format):")
    for value, label in [
        (actual, "64 chars, allowed alphabet"),
        (SECRET, "32 chars, allowed alphabet"),
        ("x" * 63 + "!", "64 chars, bad alphabet"),
        ("_" * 64, "64 chars, bad alphabet (underscore)"),
        ("A" * 63, "63 chars"),
        ("", "empty"),
    ]:
        try:
            csrf._check_token_format(value)
            verdict = "OK"
        except csrf.InvalidTokenFormat as exc:
            verdict = "InvalidTokenFormat: %s" % exc.reason
        print("  %-34s -> %s" % (label, verdict))
    print()

    print("Failure reason strings (verbatim, django/middleware/csrf.py:29-42):")
    for name in (
        "REASON_BAD_ORIGIN",
        "REASON_NO_REFERER",
        "REASON_BAD_REFERER",
        "REASON_NO_CSRF_COOKIE",
        "REASON_CSRF_TOKEN_MISSING",
        "REASON_MALFORMED_REFERER",
        "REASON_INSECURE_REFERER",
        "REASON_INCORRECT_LENGTH",
        "REASON_INVALID_CHARACTERS",
    ):
        print("  %-26s %r" % (name, getattr(csrf, name)))
    print()
    print("DRF wraps these as: PermissionDenied('CSRF Failed: %s' % reason)")
    print("  -> 403 {'detail': 'CSRF Failed: <reason>'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
