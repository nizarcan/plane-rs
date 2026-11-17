# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Run Plane's real Django cache against the Rust implementation.

`crates/plane-cache/tests/interop.rs` drives this tool with a job file and
compares the results:

* `write` — build each value from the shared corpus, `cache.set(key, value,
  timeout)` through `django.core.cache.cache` with Plane's `CACHES` settings,
  and report the raw Redis bytes;
* `read` — `cache.get(key)` for every key and report the value as the corpus's
  annotated tree (so the Rust side can compare structurally), plus the JSON
  `JSONRenderer` would produce for it;
* `invalidate` — seed keys, then call `plane.utils.cache.invalidate_cache_directly`
  and report how many keys were deleted and which survived;
* `add_incr` — `cache.add`/`cache.incr` on a caller-seeded database, with the
  post-call raw bytes and `PTTL` so the Rust side can prove the bytes and that
  an increment does not reset the TTL.

Everything runs against the database in `REDIS_URL` (only 13 and 14 are
accepted: the tool calls `FLUSHDB`).

    REDIS_URL=redis://127.0.0.1:56379/13 PYTHONPATH=. \
        .venv/bin/python ../server/tools/cache/interop.py --job job.json --out out.json
"""

from __future__ import annotations

import argparse
import binascii
import datetime
import decimal
import json
import os
import pickle
import sys
import uuid

os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
os.environ.setdefault("DATABASE_URL", "postgres://plane:plane@127.0.0.1:55432/plane_tpl")

ALLOWED_DATABASES = (13, 14)


# ---------------------------------------------------------------------------
# The corpus's annotated tree <-> Python values
# ---------------------------------------------------------------------------


class Builder:
    """Inverse of `pickle_corpus.Annotator`."""

    def __init__(self):
        self.table: list = []
        self._building: dict[int, object] = {}

    def build(self, node):
        if node is None:
            return None
        if node is True or node is False:
            return node
        assert isinstance(node, dict) and len(node) == 1, node
        (tag, payload), = node.items()
        if tag == "r":
            return self.table[payload]
        index = None
        if tag in (
            "s",
            "s8",
            "b",
            "l",
            "d",
            "e",
            "z",
            "u",
            "D",
            "T",
            "H",
            "X",
            "Z",
            "M",
            "L",
        ) or (tag == "t" and payload):
            index = len(self.table)
            self.table.append(None)
        value = self._emit(tag, payload)
        if index is not None:
            self.table[index] = value
        return value

    def _emit(self, tag, payload):
        if tag == "i":
            return int(payload)
        if tag == "f":
            return float(payload)
        if tag == "s":
            return payload
        if tag == "s8":
            return binascii.unhexlify(payload).decode("utf-8", "surrogatepass")
        if tag == "b":
            return binascii.unhexlify(payload)
        if tag == "l":
            return [self.build(item) for item in payload]
        if tag == "t":
            return tuple(self.build(item) for item in payload)
        if tag == "e":
            return {self.build(item) for item in payload}
        if tag == "z":
            return frozenset(self.build(item) for item in payload)
        if tag == "d":
            return {self.build(k): self.build(v) for k, v in payload}
        if tag == "u":
            return uuid.UUID(payload)
        if tag == "D":
            return datetime.date.fromisoformat(payload)
        if tag == "T":
            naive = datetime.datetime.fromisoformat(payload["n"])
            tz = payload["tz"]
            if tz is not None:
                naive = naive.replace(tzinfo=_timezone(tz["Z"]))
            if payload["fold"]:
                naive = naive.replace(fold=1)
            return naive
        if tag == "H":
            return datetime.time.fromisoformat(payload)
        if tag == "X":
            return datetime.timedelta(
                days=payload["days"],
                seconds=payload["seconds"],
                microseconds=payload["microseconds"],
            )
        if tag == "Z":
            return _timezone(payload)
        if tag == "M":
            return decimal.Decimal(payload)
        if tag == "L":
            from rest_framework.utils.serializer_helpers import ReturnDict, ReturnList

            inner = self.build(payload["v"])
            if payload["func"] == "list":
                return ReturnList(inner, serializer=None)
            return ReturnDict(inner, serializer=None)
        raise ValueError(f"unknown tag {tag}")


def _timezone(value):
    if value == "utc":
        return datetime.timezone.utc
    return datetime.timezone(datetime.timedelta(seconds=value))


# The annotator is shared with the corpus generator: the two formats must not
# drift apart.
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pickle_corpus import Annotator  # noqa: E402


def annotate(value):
    return Annotator().annotate(value)


# ---------------------------------------------------------------------------
# Operations
# ---------------------------------------------------------------------------


def database_of(url: str) -> int:
    from django_redis.util import CacheKey  # noqa: F401  (import check)
    import redis

    return redis.connection.parse_url(url).get("db", 0)


def write(job, cache):
    written = []
    for case in job["cases"]:
        value = Builder().build(case["tree"])
        cache.set(case["key"], value, case.get("timeout"))
        from django_redis import get_redis_connection

        raw = get_redis_connection("default").get(f":1:{case['key']}")
        written.append({"key": case["key"], "hex": binascii.hexlify(raw).decode()})
    return {"written": written}


def add_incr(job, cache):
    """`cache.add`/`cache.incr`, with the raw bytes and remaining TTL.

    Each case is one call against a database the caller seeded; the result
    includes the post-call raw value (`hex`, `None` for a miss) and `pttl_ms`
    so the Rust side can prove the TTL is written and **not** reset by an
    increment.
    """
    from django_redis import get_redis_connection

    connection = get_redis_connection("default")
    results = []
    for case in job["cases"]:
        key = case["key"]
        full = f":1:{key}"
        entry = {"key": key, "op": case["op"]}
        if case["op"] == "add":
            entry["result"] = cache.add(
                key, Builder().build(case["tree"]), case.get("timeout")
            )
        elif case["op"] == "incr":
            try:
                entry["value"] = cache.incr(key, case.get("delta", 1))
            except ValueError as error:
                entry["error"] = str(error)
        else:  # pragma: no cover - the Rust side never sends one
            raise ValueError(f"unknown op {case['op']!r}")
        raw = connection.get(full)
        entry["hex"] = None if raw is None else binascii.hexlify(raw).decode()
        entry["pttl_ms"] = connection.pttl(full)
        results.append(entry)
    return {"results": results}


def read(job, cache):
    from rest_framework.renderers import JSONRenderer

    renderer = JSONRenderer()
    values = []
    for key in job["keys"]:
        value = cache.get(key)
        if value is None:
            values.append({"key": key, "found": False})
            continue
        try:
            rendered = binascii.hexlify(renderer.render(value)).decode()
        except Exception as error:  # a value DRF cannot render
            rendered = f"error:{type(error).__name__}"
        values.append(
            {
                "key": key,
                "found": True,
                "tree": annotate(value),
                "rendered": rendered,
            }
        )
    return {"values": values}


def invalidate(job, cache):
    from django_redis import get_redis_connection

    from plane.utils.cache import invalidate_cache_directly

    connection = get_redis_connection("default")
    cache.clear()
    for seed in job["seeds"]:
        cache.set(seed["key"], Builder().build(seed["tree"]), seed.get("timeout"))

    class User:
        def __init__(self, pk):
            self.id = pk
            self.pk = pk
            self.is_anonymous = pk is None

    class ResolverMatch:
        def __init__(self, kwargs):
            self.kwargs = kwargs

    class Request:
        def __init__(self, spec):
            self.user = User(spec["user_id"])
            self.resolver_match = ResolverMatch(spec["kwargs"])
            self._full_path = spec["full_path"]

        def get_full_path(self):
            return self._full_path

    spec = job["spec"]
    request = None if spec.get("request") is None else Request(spec["request"])
    before = sorted(
        key.decode() if isinstance(key, bytes) else key
        for key in connection.keys(":1:*")
    )
    invalidate_cache_directly(
        path=spec["path"],
        url_params=spec["url_params"],
        user=spec["user"],
        request=request,
        multiple=spec["multiple"],
    )
    after = sorted(
        key.decode() if isinstance(key, bytes) else key
        for key in connection.keys(":1:*")
    )
    return {
        "before": before,
        "after": after,
        "deleted": [key for key in before if key not in after],
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--job", required=True)
    parser.add_argument("--out", required=True)
    arguments = parser.parse_args()

    with open(arguments.job, encoding="utf-8") as handle:
        job = json.load(handle)

    url = os.environ.get("REDIS_URL", "")
    database = database_of(url)
    if database not in ALLOWED_DATABASES:
        raise SystemExit(
            f"refusing to FLUSHDB database {database}: only {ALLOWED_DATABASES} are allowed"
        )

    import django

    django.setup()

    from django.core.cache import cache

    operations = {
        "write": write,
        "read": read,
        "invalidate": invalidate,
        "add_incr": add_incr,
    }
    result = operations[job["op"]](job, cache)
    result["python"] = sys.version.split()[0]
    result["protocol"] = pickle.DEFAULT_PROTOCOL
    with open(arguments.out, "w", encoding="utf-8") as handle:
        json.dump(result, handle, ensure_ascii=False)
        handle.write("\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
