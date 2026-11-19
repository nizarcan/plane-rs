# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Session payload codec: byte-compatible vectors for the Rust port.

Django stores the session dict in ``sessions.session_data`` as
``<signer output>`` produced by
``django.contrib.sessions.backends.base.SessionBase.encode``:

    signing.dumps(session_dict,
                  salt=self.key_salt,
                  serializer=self.serializer,   # sessions.serializers.JSONSerializer
                  compress=True)

``key_salt`` is ``"django.contrib.sessions." + type(self).__qualname__``.  Plane's
store is ``plane.db.models.session.SessionStore`` so the qualname is
``SessionStore`` and the salt is the literal string
``django.contrib.sessions.SessionStore``.

Run from ``apps/api``:

    .venv/bin/python ../server/tools/contract/session_codec.py
"""

import json
import os
import sys
import zlib
from unittest import mock

SECRET_KEY = "contract-test-secret-key-not-for-deployment"
FROZEN_TIME = 1_777_777_777  # 2026-05-01T21:36:17Z, arbitrary but fixed

TOOLS_DIR = os.path.dirname(os.path.abspath(__file__))
# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, os.path.join(TOOLS_DIR, "..", "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_DIR = str(_oracle_api_dir())
sys.path.insert(0, API_DIR)

os.environ["DJANGO_SETTINGS_MODULE"] = "plane.settings.production"
os.environ["SECRET_KEY"] = SECRET_KEY
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")

import django  # noqa: E402

django.setup()

from django.contrib.sessions.serializers import JSONSerializer  # noqa: E402
from django.core import signing  # noqa: E402
from django.utils.crypto import salted_hmac  # noqa: E402

from plane.db.models.session import SessionStore  # noqa: E402


def b62(n):
    """django.core.signing.b62_encode."""
    alphabet = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
    if n == 0:
        return "0"
    sign = "-" if n < 0 else ""
    n = abs(n)
    out = ""
    while n > 0:
        n, rem = divmod(n, 62)
        out = alphabet[rem] + out
    return sign + out


def b64e(raw):
    """django.core.signing.b64_encode."""
    import base64

    return base64.urlsafe_b64encode(raw).strip(b"=")


def hand_encode(session_dict):
    """Re-implement SessionBase.encode from first principles (no Django signing)."""
    data = json.dumps(session_dict, separators=(",", ":")).encode("latin-1")
    compressed = zlib.compress(data)
    is_compressed = len(compressed) < len(data) - 1
    if is_compressed:
        data = compressed
    base64d = b64e(data).decode()
    if is_compressed:
        base64d = "." + base64d
    value = "%s:%s" % (base64d, b62(FROZEN_TIME))
    # salted_hmac(salt + "signer", value, SECRET_KEY, algorithm="sha256")
    mac = salted_hmac(
        "django.contrib.sessions.SessionStore" + "signer", value, SECRET_KEY, algorithm="sha256"
    ).digest()
    return "%s:%s" % (value, b64e(mac).decode())


def main():
    print("Django", django.get_version())
    print("SECRET_KEY        =", SECRET_KEY)
    print("frozen int(time)  =", FROZEN_TIME, "->", b62(FROZEN_TIME))
    print("key_salt          =", SessionStore().key_salt)
    print("SESSION_SERIALIZER=", JSONSerializer)
    print()

    app_session = {
        "_auth_user_id": "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
        "backend": "django.contrib.auth.backends.ModelBackend",
        "_auth_user_hash": "9f2f4b1d3e0c8a7b6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c",
        "_session_expiry": 3600,
        "device_info": {
            "user_agent": "Mozilla/5.0 (contract probe)",
            "ip_address": "203.0.113.7",
            "domain": "http://localhost/",
        },
    }

    with mock.patch("django.core.signing.time.time", return_value=FROZEN_TIME):
        django_encoded = SessionStore().encode(app_session)

    hand_encoded = hand_encode(app_session)

    print("session dict (insertion order is preserved in session_data):")
    print(json.dumps(app_session, indent=2))
    print()
    print("raw JSON bytes fed to the serializer:")
    raw = json.dumps(app_session, separators=(",", ":")).encode("latin-1")
    print("  len =", len(raw))
    print(" ", raw.decode())
    print()
    print("zlib.compress default level output len =", len(zlib.compress(raw)))
    print("compression used (len(z) < len(raw)-1)  =", len(zlib.compress(raw)) < len(raw) - 1)
    print()
    print("django SessionStore().encode(...):")
    print("  ", django_encoded)
    print("hand-rolled encode(...):")
    print("  ", hand_encoded)
    print("MATCH:", django_encoded == hand_encoded)
    print()

    # Signer layout, spelled out
    parts = django_encoded.split(":")
    print("signer layout (value:timestamp:signature), split on ':' ->", len(parts), "parts")
    print("  payload(base64, '.' = zlib) =", parts[0])
    print("  timestamp(b62)             =", parts[1], "=", FROZEN_TIME)
    print("  signature(base64 hmac)     =", parts[2])
    print()

    with mock.patch("django.core.signing.time.time", return_value=FROZEN_TIME):
        decoded = SessionStore().decode(django_encoded)
    print("SessionStore().decode(django_encoded) ==", decoded)
    print("round-trip equal:", decoded == app_session)
    print()

    # A session small enough that zlib does not help: no '.' prefix.
    small = {"_auth_user_id": "a"}
    with mock.patch("django.core.signing.time.time", return_value=FROZEN_TIME):
        small_encoded = SessionStore().encode(small)
    small_raw = json.dumps(small, separators=(",", ":")).encode("latin-1")
    print("small session", small)
    print("  raw JSON len =", len(small_raw), "zlib len =", len(zlib.compress(small_raw)))
    print("  encoded =", small_encoded)
    print("  compressed flag =", small_encoded.startswith("."))
    with mock.patch("django.core.signing.time.time", return_value=FROZEN_TIME):
        print("  decode  =", SessionStore().decode(small_encoded))
    print()

    # Two more edge cases the Rust side must reproduce.
    unicode_session = {"name": "Zoë — 日本語", "emoji": "🚀", "sep": "\u2028"}
    with mock.patch("django.core.signing.time.time", return_value=FROZEN_TIME):
        u_encoded = SessionStore().encode(unicode_session)
    print("non-ASCII session", unicode_session)
    print("  encoded =", u_encoded)
    print("  NOTE ensure_ascii=True -> \\uXXXX escapes, latin-1 codepoint transport")
    print()

    # Tampering: changing the signature must yield {} (BadSignature is swallowed).
    tampered = django_encoded[:-2] + ("AA" if django_encoded[-2:] != "AA" else "BB")
    print("tampered payload decode ->", SessionStore().decode(tampered))
    print("garbage payload decode  ->", SessionStore().decode("not-a-session"))
    print()

    # What SessionStore.create_model_instance puts in the row.
    from plane.db.models.session import VALID_KEY_CHARS

    print("session_key generation: get_random_string(128, ascii_lowercase + digits)")
    print("  VALID_KEY_CHARS =", VALID_KEY_CHARS)
    print("  length = 128")
    return 0


if __name__ == "__main__":
    sys.exit(main())
