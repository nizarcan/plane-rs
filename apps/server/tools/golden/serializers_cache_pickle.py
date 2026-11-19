#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record Django's cached pickle for the two `@cache_response` workspace endpoints.

The value a hit re-renders is django-redis' protocol-4 pickle of
`{"data": serializer.data, "status": 200}` (`plane/utils/cache.py:25-51`). The
differential harness compares Redis values byte for byte only below 4 KiB and
by type+size above it, so a payload can "pass" while its bytes differ. This
recorder captures the real bytes — and the payload shape, with CPython's string
sharing marked — so `crates/plane-cache/tests/cache_pickle.rs` can rebuild the
same tree through `PyInterner` and assert byte equality.

Usage, from the repository root, against a clone of the rich fixture template
(`plane_fix_tpl`, built by `apps/server/dev/bootstrap-fixture.sh`)::

    apps/server/dev/mkdb.sh t_fw2_pickle --replace --template plane_fix_tpl
    eval "$(PLANE_DB=t_fw2_pickle PLANE_REDIS_DB=2 apps/server/dev/django-env.sh)"
    apps/api/.venv/bin/python apps/server/tools/golden/serializers_cache_pickle.py

Writes ``apps/server/crates/plane-cache/tests/golden/cache_pickle.jsonl``. Each
line::

    {"case": "estimates",
     "path": "/api/workspaces/fix-workspace/estimates/",
     "user_id": "…", "status": 200,
     "pickle_hex": "800495…",
     "payload": {"t": "dict", "v": [...]}}

``payload`` is ``serializer.data`` as a tagged tree:

* ``none`` / ``bool`` / ``int`` (decimal text) / ``float`` (`repr`) / ``uuid``;
* ``str`` with ``s: true`` when the same object occurs more than once in the
  payload — the case where CPython's pickler writes one memo entry and
  `BINGET`s, rather than two literals;
* ``list`` / ``dict`` with the members in insertion order.

Only the types these two serializers produce are accepted; anything else is an
error so the golden cannot silently lose a shape.

The recorder used a specific Redis logical database (`PLANE_REDIS_DB`); it
deletes only the two keys it is about to read and flushes nothing.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import pickle
import sys
from pathlib import Path
from typing import Any
from uuid import UUID

HERE = Path(__file__).resolve().parent
SERVER_DIR = HERE.parents[1]
REPO_ROOT = HERE.parents[3]

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(SERVER_DIR / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_DIR = _oracle_api_dir()
GOLDEN_DIR = SERVER_DIR / "crates" / "plane-cache" / "tests" / "golden"
GOLDEN_FILE = GOLDEN_DIR / "cache_pickle.jsonl"
MANIFEST = SERVER_DIR / "scenarios" / "fixtures" / "fix_tpl.json"

sys.path.insert(0, str(API_DIR))

CASES = (
    ("estimates", "/api/workspaces/fix-workspace/estimates/"),
    ("labels", "/api/workspaces/fix-workspace/labels/"),
)


def typed(value: Any, shared: collections.Counter[int]) -> dict[str, Any]:
    """A tagged, order-preserving view of a picklable DRF value.

    ``shared`` counts Python object identities in the whole payload. A string
    is marked ``s: true`` when the same object occurs more than once: that is
    the only case where CPython's pickler emits a `BINGET` instead of a second
    literal, so it is the only case the Rust builder has to reproduce. (A value
    that occurs once is encoded identically whether or not it is interned.) The
    count is taken from the decoded tree — unpickling recreates the shared
    objects with one `id` — not from `sys.intern`, which would call a decoded
    object interned even when the original was not.
    """
    from rest_framework.utils.serializer_helpers import ReturnDict, ReturnList

    if value is None:
        return {"t": "none"}
    if isinstance(value, bool):
        return {"t": "bool", "v": value}
    if isinstance(value, int):
        return {"t": "int", "v": str(value)}
    if isinstance(value, float):
        return {"t": "float", "v": repr(value)}
    if isinstance(value, str):
        return {"t": "str", "v": value, "s": shared[id(value)] > 1}
    if isinstance(value, UUID):
        return {"t": "uuid", "v": str(value)}
    if isinstance(value, ReturnList):
        return {
            "t": "return_list",
            "v": [typed(item, shared) for item in value],
        }
    if isinstance(value, ReturnDict):
        return {
            "t": "return_dict",
            "v": [
                [typed(key, shared), typed(item, shared)]
                for key, item in value.items()
            ],
        }
    if isinstance(value, (list, tuple)):
        return {"t": "list", "v": [typed(item, shared) for item in value]}
    if isinstance(value, dict):
        return {
            "t": "dict",
            "v": [
                [typed(key, shared), typed(item, shared)]
                for key, item in value.items()
            ],
        }
    raise TypeError(f"unmodelled cached value {type(value).__name__}: {value!r}")


def identity_counts(value: Any, counts: collections.Counter[int]) -> None:
    """Count every container and string object in the decoded payload."""
    counts[id(value)] += 1
    if isinstance(value, str):
        return
    if isinstance(value, (list, tuple)):
        for item in value:
            identity_counts(item, counts)
    elif isinstance(value, dict):
        for key, item in value.items():
            identity_counts(key, counts)
            identity_counts(item, counts)


def shared_objects(data: Any) -> collections.Counter[int]:
    """The ids whose count is above one, i.e. the objects the pickler shared."""
    counts: collections.Counter[int] = collections.Counter()
    identity_counts(data, counts)
    return counts


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=GOLDEN_FILE)
    args = parser.parse_args(argv)

    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    import django

    django.setup()

    import redis
    from django.conf import settings
    from django.core.cache import cache
    from django.test import Client

    database = settings.DATABASES["default"]["NAME"]
    if not str(database).startswith("t_"):
        raise SystemExit(
            f"refusing to read {database!r}: expected a t_* clone of plane_fix_tpl"
        )
    if settings.DEBUG:
        # `cache_response` only writes when DEBUG is off (`cache.py:30`).
        raise SystemExit("DEBUG must be 0: the view does not cache in debug mode")

    manifest = json.loads(MANIFEST.read_text(encoding="utf-8"))
    session = manifest["sessions"]["admin"]
    user_id = session["user_id"]

    client = Client()
    client.cookies[session["cookie"]] = session["value"]

    redis_client = redis.Redis.from_url(os.environ["REDIS_URL"])

    records: list[dict[str, Any]] = []
    for case, path in CASES:
        key = f"{path}:{user_id}"
        cache.delete(key)
        response = client.get(path)
        if response.status_code != 200:
            raise SystemExit(f"{path}: {response.status_code} {response.content[:200]!r}")
        raw = redis_client.get(cache.make_key(key))
        if raw is None:
            raise SystemExit(f"{path}: no cache entry at {cache.make_key(key)!r}")
        payload = pickle.loads(raw)
        if payload["status"] != 200:
            raise SystemExit(f"{path}: cached status {payload['status']}")
        if json.loads(response.content) != json.loads(
            json.dumps(payload["data"], default=str)
        ):
            # The stored tree must decode to what the miss rendered; a
            # difference means the two paths parted company.
            raise SystemExit(f"{path}: the cached tree does not match the response")
        shared = shared_objects(payload["data"])
        records.append(
            {
                "case": case,
                "path": path,
                "user_id": user_id,
                "status": payload["status"],
                "pickle_hex": raw.hex(),
                # Both endpoints are `many=True`, so `response.data` is a
                # `ReturnList`; unpickling turns it into a plain `list`, so the
                # wrapper has to be restored from the endpoint's shape.
                "payload": {
                    "t": "return_list",
                    "v": [typed(item, shared) for item in payload["data"]],
                },
            }
        )

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(
        "\n".join(json.dumps(record, ensure_ascii=False) for record in records) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"wrote {args.out} ({len(records)} cases)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
