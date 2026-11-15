# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Produce a deterministic Fernet test vector for the instance-configuration
secret store, using the repository's own encryption code path.

``plane/license/utils/encryption.py`` derives a Fernet key with PBKDF2-HMAC-
SHA256(SECRET_KEY, b"salt", 100000) and base64-url-encodes the 32-byte output.
``apps/server/ENV.md`` documents the scheme; this script is what that document's
worked example was generated with, and the Rust port must be able to decrypt
the token printed here.

``Fernet.encrypt`` stamps ``int(time.time())`` and a random 16-byte IV into the
token, so the script patches both to fixed values for the vector and also prints
one real (non-deterministic) token as a sanity check.

Run it with the API virtualenv, from anywhere::

    apps/api/.venv/bin/python apps/server/tools/inventory/encryption_vector.py
"""

from __future__ import annotations

import base64
import hashlib
import sys
from pathlib import Path
from unittest.mock import patch

HERE = Path(__file__).resolve()
REPO_ROOT = HERE.parents[4]
API_DIR = REPO_ROOT / "apps" / "api"

# Fixed, obviously-fake inputs. These are not secrets and must never be reused.
SECRET_KEY = "plane-rs-fernet-vector-not-a-real-secret"
PLAINTEXT = "postgres://plane:plane@plane-db:5432/plane"
FIXED_TIME = 1_700_000_000  # 2023-11-14T22:13:20Z
FIXED_IV = bytes(range(16))  # 000102...0f

sys.path.insert(0, str(API_DIR))

import django  # noqa: E402
from django.conf import settings  # noqa: E402

if not settings.configured:
    # Minimal settings: importing `plane.license.utils.encryption` runs
    # `plane/__init__.py`, which imports `plane.celery`, which builds a Redis
    # client at import time (see ENV.md, "REDIS_URL is required at import").
    settings.configure(
        SECRET_KEY=SECRET_KEY,
        DEBUG=False,
        USE_TZ=True,
        TIME_ZONE="UTC",
        INSTALLED_APPS=[],
        REDIS_URL="redis://127.0.0.1:6379/0",
        REDIS_SSL=False,
    )
django.setup()

import logging  # noqa: E402

# decrypt_data/encrypt_data log swallowed exceptions to "plane.exception".
# The failure cases below are deliberate; keep the output readable.
_exception_logger = logging.getLogger("plane.exception")
_exception_logger.handlers = [logging.NullHandler()]
_exception_logger.propagate = False

from plane.license.utils.encryption import (  # noqa: E402
    decrypt_data,
    derive_key,
    encrypt_data,
)


def hexdump(label: str, data: bytes) -> None:
    print(f"{label:<22} {data.hex()}  ({len(data)} bytes)")


def main() -> int:
    print("== inputs ==")
    print(f"{'SECRET_KEY':<22} {SECRET_KEY!r}")
    print(f"{'PLAINTEXT':<22} {PLAINTEXT!r}")
    print(f"{'fixed timestamp':<22} {FIXED_TIME}")
    print(f"{'fixed IV':<22} {FIXED_IV.hex()}")

    print("\n== key derivation (derive_key) ==")
    secret_bytes = SECRET_KEY.encode()
    hexdump("secret key utf-8", secret_bytes)
    salt = b"salt"
    hexdump("salt", salt)
    iterations = 100000
    print(f"{'iterations':<22} {iterations}")
    print(f"{'hash':<22} pbkdf2_hmac('sha256', ...)")
    dk = hashlib.pbkdf2_hmac("sha256", secret_bytes, salt, iterations)
    hexdump("PBKDF2 output", dk)
    fernet_key = base64.urlsafe_b64encode(dk)
    print(f"{'fernet key':<22} {fernet_key.decode()!r}  ({len(fernet_key)} bytes)")
    assert fernet_key == derive_key(SECRET_KEY), "derive_key drifted from this reimplementation"

    raw = base64.urlsafe_b64decode(fernet_key)
    hexdump("signing key ([:16])", raw[:16])
    hexdump("encryption key ([16:])", raw[16:])
    print(f"{'cipher':<22} AES-128-CBC, PKCS7 padding, HMAC-SHA256")

    print("\n== deterministic token ==")
    with (
        patch("cryptography.fernet.time.time", return_value=FIXED_TIME),
        patch("cryptography.fernet.os.urandom", return_value=FIXED_IV),
    ):
        token = encrypt_data(PLAINTEXT)
    print(f"ciphertext token       {token}")

    token_bytes = base64.urlsafe_b64decode(token)
    hexdump("decoded token", token_bytes)
    print(f"{'version byte':<22} 0x{token_bytes[0]:02x}")
    print(f"{'timestamp':<22} {int.from_bytes(token_bytes[1:9], 'big')}")
    hexdump("IV", token_bytes[9:25])
    hexdump("ciphertext", token_bytes[25:-32])
    hexdump("HMAC-SHA256", token_bytes[-32:])

    print("\n== round-trip through the repository code ==")
    recovered = decrypt_data(token)
    print(f"{'decrypt_data(token)':<22} {recovered!r}")
    assert recovered == PLAINTEXT, "round-trip failed"
    print("round-trip             OK")

    print("\n== decryption failure behaviour ==")
    settings.SECRET_KEY = "a-different-secret-key"
    wrong = decrypt_data(token)
    print(f"{'wrong SECRET_KEY':<22} decrypt_data(token) -> {wrong!r} (exception swallowed)")
    settings.SECRET_KEY = SECRET_KEY
    tampered = token[:-4] + ("AAAA" if not token.endswith("AAAA") else "BBBB")
    print(f"{'tampered token':<22} decrypt_data(token) -> {decrypt_data(tampered)!r} (exception swallowed)")
    print(f"{'empty input':<22} decrypt_data('') -> {decrypt_data('')!r}")
    print(f"{'empty plaintext':<22} encrypt_data('') -> {encrypt_data('')!r}")

    print("\n== a real (non-deterministic) token ==")
    real = encrypt_data(PLAINTEXT)
    print(f"{'token':<22} {real}")
    print(f"{'round-trip':<22} {decrypt_data(real)!r}")

    print("\n== copy-paste vector for ENV.md ==")
    print(f"secret_key       {SECRET_KEY}")
    print(f"plaintext        {PLAINTEXT}")
    print(f"timestamp        {FIXED_TIME}")
    print(f"iv_hex           {FIXED_IV.hex()}")
    print(f"pbkdf2_hex       {dk.hex()}")
    print(f"fernet_key       {fernet_key.decode()}")
    print(f"token            {token}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
