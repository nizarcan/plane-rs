# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Run the real magic-code state machine against the Rust implementation.

`crates/plane-auth/tests/magic_interop.rs` and
`crates/plane-cache/tests/eval_interop.rs` drive this tool with a job file:

* ``eval`` — the `INCR` + first-`EXPIRE` Lua script
  (`provider/credentials/magic_code.py:40-46`) against a caller-chosen key, for
  a list of `(key, ttl)` increments. Returns the counter value and the key's
  `TTL` after each step, so the Rust side can prove both the returned value and
  the "the window starts at the first increment" rule.
* ``state`` — the raw value, type and TTL of keys (never a token key's bytes:
  the record holds the login code).
* ``initiate`` — `MagicCodeProvider(key=email).initiate()`, returning the key
  name, the TTLs and a **redacted** description of the record (the code itself
  is never printed).
* ``verify`` — `MagicCodeProvider(key="magic_"+email, code=...).set_user_data()`,
  with the code either given literally or read from Redis by the tool (so a
  code the Rust side wrote is never copied through Rust at all). Returns the
  resulting `AuthenticationException` (`error_code`/`error_message`/payload) or
  `ok`, plus the key state afterwards.

The database lookup `_raise_for_email` makes (`User.objects.filter(...)`
`.exists()`) is stubbed: the job says whether the address exists, the tool
records every address the provider looked up, and the Redis state — the part
this interop is for — runs for real. The Rust provider takes the same lookup
from a real `users` table in `crates/plane-auth/tests/magic_interop.rs`.

Nothing here prints a magic code, a session key or a password.

    REDIS_URL=redis://127.0.0.1:56379/9 EMAIL_HOST=127.0.0.1 \
    DATABASE_URL=postgres://plane:plane@127.0.0.1:55432/plane_tpl \
    PYTHONPATH=. .venv/bin/python ../server/tools/auth/magic_interop.py \
        --job job.json --out out.json
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

sys.path.insert(0, str(_oracle_api_dir()))

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
# `MagicCodeProvider.__init__` refuses to run without SMTP configured; no mail
# is sent by `initiate` (the task is the caller's job).
os.environ.setdefault("EMAIL_HOST", "127.0.0.1")
# `SKIP_ENV_VAR=1` (the default) makes `get_configuration_value` read
# `instance_configurations` from Postgres. The tool only needs the two flags,
# and reads them from the environment instead, so the Redis state machine can
# be compared without a database (`plane/license/utils/instance_value.py:18-39`).
os.environ.setdefault("SKIP_ENV_VAR", "0")

assert "DJANGO_SETTINGS_MODULE" in os.environ

import django  # noqa: E402

django.setup()

ALLOWED_DATABASES = (9, 13, 14)


def _database_index(url: str) -> int:
    from redis.connection import parse_url

    kwargs = parse_url(url)
    return int(kwargs.get("db") or 0)


def _redis():
    from plane.settings.redis import redis_instance

    return redis_instance()


class _FakeUserQuerySet:
    def __init__(self, exists: bool):
        self._exists = exists

    def exists(self) -> bool:
        return self._exists


class _FakeUserManager:
    def __init__(self, exists: bool, lookups: list):
        self._exists = exists
        self._lookups = lookups

    def filter(self, **kwargs):
        self._lookups.append(kwargs.get("email"))
        return _FakeUserQuerySet(self._exists)


class _FakeUser:
    def __init__(self, exists: bool, lookups: list):
        self.objects = _FakeUserManager(exists, lookups)


def _patch_user(exists: bool, lookups: list):
    """Replace `magic_code.User` with a recorder for the two `.exists()` calls."""
    import unittest.mock

    from plane.authentication.provider.credentials import magic_code

    return unittest.mock.patch.object(magic_code, "User", _FakeUser(exists, lookups))


def _key_report(ri, key: str, *, secrets: bool) -> dict:
    """The value/type/TTL of `key`; a secret key reports only its shape."""
    pipe = ri.pipeline()
    pipe.exists(key)
    pipe.type(key)
    pipe.ttl(key)
    exists, kind, ttl = pipe.execute()
    if isinstance(kind, bytes):
        kind = kind.decode()
    report: dict = {"exists": bool(exists), "type": kind, "ttl": int(ttl)}
    if exists and not secrets:
        raw = ri.get(key)
        report["value_hex"] = raw.hex() if raw is not None else None
    return report


def _token_shape(ri, key: str) -> dict | None:
    """Whether `key` holds a JSON record with a six-digit token."""
    raw = ri.get(key)
    if raw is None:
        return None
    data = json.loads(raw)
    token = data.get("token")
    return {
        "keys": sorted(data.keys()),
        "email_present": "email" in data,
        "current_attempt": data.get("current_attempt"),
        "token_len": len(str(token)) if token is not None else None,
        "token_is_six_digits": isinstance(token, str) and len(token) == 6 and token.isdigit(),
        "raw_len": len(raw),
    }


def op_eval(job: dict) -> dict:
    from plane.authentication.provider.credentials.magic_code import MagicCodeProvider

    ri = _redis()
    script = MagicCodeProvider._INCREMENT_COUNTER_SCRIPT
    steps = []
    for step in job["steps"]:
        key = step["key"]
        ttl = int(step["ttl"])
        value = int(ri.eval(script, 1, key, ttl))
        steps.append({"key": key, "result": value, "ttl": int(ri.ttl(key))})
    return {"steps": steps}


def op_state(job: dict) -> dict:
    ri = _redis()
    return {
        "keys": {
            key: _key_report(ri, key, secrets=bool(job.get("secret_keys", True)))
            for key in job["keys"]
        }
    }


def op_seed(job: dict) -> dict:
    """Write a value/TTL directly, so Rust can read a Python-written record."""
    ri = _redis()
    key = job["key"]
    value = job["value"]
    ttl = job.get("ttl")
    if ttl is None:
        ri.set(key, value)
    else:
        ri.set(key, value, ex=int(ttl))
    return {"key": key, "ttl": int(ri.ttl(key))}


def op_delete(job: dict) -> dict:
    ri = _redis()
    return {"deleted": int(ri.delete(*job["keys"]))}


def op_incr(job: dict) -> dict:
    """`redis-py`'s plain `incr`, for the "INCR keeps the TTL" comparison."""
    ri = _redis()
    return {"value": int(ri.incr(job["key"])), "ttl": int(ri.ttl(job["key"]))}


def op_initiate(job: dict) -> dict:
    from plane.authentication.provider.credentials.magic_code import MagicCodeProvider

    ri = _redis()
    email = job["email"]
    lookups: list = []
    with _patch_user(bool(job.get("user_exists", False)), lookups):
        provider = MagicCodeProvider(request=None, key=email)
        try:
            key, token = provider.initiate()
        except Exception as error:  # AuthenticationException
            return {
                "outcome": "error",
                "error_code": getattr(error, "error_code", None),
                "error_message": getattr(error, "error_message", None),
                "payload": getattr(error, "payload", None),
                "user_lookups": lookups,
                "token_key": f"magic_{email}",
                "state": {
                    name: _key_report(ri, f"magic_{email}{suffix}", secrets=True)
                    for name, suffix in (
                        ("token", ""),
                        ("issued", ":issued"),
                        ("verify", ":verify_attempts"),
                    )
                },
            }
    token_key = key
    return {
        "outcome": "ok",
        "token_key": token_key,
        "token_len": len(token),
        "token_is_six_digits": token.isdigit() and len(token) == 6,
        "user_lookups": lookups,
        "record": _token_shape(ri, token_key),
        "state": {
            "token": _key_report(ri, token_key, secrets=True),
            "issued": _key_report(ri, f"{token_key}:issued", secrets=False),
            "verify": _key_report(ri, f"{token_key}:verify_attempts", secrets=True),
        },
    }


def _error_dict(error) -> dict:
    return {
        "outcome": "error",
        "error_code": getattr(error, "error_code", None),
        "error_message": getattr(error, "error_message", None),
        "payload": getattr(error, "payload", None),
    }


def op_verify(job: dict) -> dict:
    from plane.authentication.provider.credentials.magic_code import MagicCodeProvider

    ri = _redis()
    key = job["key"]
    if job.get("code_from_redis"):
        raw = ri.get(key)
        if raw is None:
            return {"outcome": "missing_record"}
        code = str(json.loads(raw)["token"])
    else:
        code = job["code"]
    lookups: list = []
    with _patch_user(bool(job.get("user_exists", False)), lookups):
        provider = MagicCodeProvider(request=None, key=key, code=code)
        try:
            provider.set_user_data()
            result = {"outcome": "ok"}
        except Exception as error:  # AuthenticationException
            result = _error_dict(error)
    result["user_lookups"] = lookups
    result["state"] = {
        name: _key_report(ri, suffix_key, secrets=True)
        for name, suffix_key in (
            ("token", key),
            ("issued", f"{key}:issued"),
            ("verify", f"{key}:verify_attempts"),
        )
    }
    if provider.user_data is not None:
        # The user_data the provider built: no secret in it, but keep the shape.
        result["user_data_keys"] = sorted(provider.user_data.keys())
        result["user_data_user_keys"] = sorted(provider.user_data.get("user", {}).keys())
        result["user_data_email_matches"] = provider.user_data.get("email") == key.replace("magic_", "", 1)
    return result


OPS = {
    "eval": op_eval,
    "state": op_state,
    "seed": op_seed,
    "delete": op_delete,
    "incr": op_incr,
    "initiate": op_initiate,
    "verify": op_verify,
}


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--job", required=True)
    parser.add_argument("--out", required=True)
    args = parser.parse_args(argv)

    url = os.environ.get("REDIS_URL", "")
    if not url:
        print("REDIS_URL is not set", file=sys.stderr)
        return 2
    database = _database_index(url)
    if database not in ALLOWED_DATABASES:
        print(
            f"refusing database {database}: this tool deletes keys and only "
            f"databases {ALLOWED_DATABASES} are allowed",
            file=sys.stderr,
        )
        return 2

    with open(args.job, "r", encoding="utf-8") as handle:
        job = json.load(handle)
    operation = OPS[job["op"]]
    result = operation(job)
    result["database"] = database
    with open(args.out, "w", encoding="utf-8") as handle:
        json.dump(result, handle)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
