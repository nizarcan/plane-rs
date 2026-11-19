#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Run Django's real DRF throttle classes against a scripted request list.

This is the Python half of the Django/Rust throttle interop check. The Rust
test in ``crates/plane-throttle/tests/interop.rs`` writes a scenario file, runs
this script over a range of its steps, runs the same range through
``plane-throttle``, and compares the two traces: the decision, the cache key,
the raw Redis value bytes and the TTL.

The scenario format::

    {
      "num_proxies": null | 0 | n,
      "steps": [
        {"op": "request",
         "class": ["rest_framework.throttling.AnonRateThrottle"],
         "now": 1789000000.123456, "remote": "203.0.113.7", "xff": "1.1.1.1, 2.2.2.2",
         "authenticated": false, "user_pk": null, "api_key": null, "asset_id": null},
        {"op": "set_history", "key": "throttle_anon_1.2.3.4",
         "history": [1789000000.0], "timeout": 60},
        {"op": "read", "key": "throttle_anon_1.2.3.4"}
      ]
    }

Run it the way the test does::

    cd apps/api
    DJANGO_SETTINGS_MODULE=plane.settings.production \
    REDIS_URL=redis://127.0.0.1:56379/9 \
    DATABASE_URL=postgres://plane:plane@127.0.0.1:55432/plane_tpl \
    SECRET_KEY=interop .venv/bin/python ../server/tools/throttle/interop.py scenario.json \
        --from-index 0 --to-index 10 --flush

``AUTHENTICATION_RATE_LIMIT`` and ``API_KEY_RATE_LIMIT`` are read when
``plane.authentication.rate_limit`` and the settings module are imported, so
they must be set in the environment before this process starts (the test passes
them explicitly). ``NUM_PROXIES`` comes from ``REST_FRAMEWORK`` at import; this
script assigns ``api_settings.NUM_PROXIES`` per scenario so a single process can
run more than one.
"""

import argparse
import importlib
import json
import os
import sys
import uuid
from types import SimpleNamespace


def load_throttle_classes():
    """Import the classes by dotted path so the real modules are used."""
    paths = {
        "AnonRateThrottle": "rest_framework.throttling.AnonRateThrottle",
        "UserRateThrottle": "rest_framework.throttling.UserRateThrottle",
        "AuthenticationThrottle": "plane.authentication.rate_limit.AuthenticationThrottle",
        "EmailVerificationThrottle": "plane.authentication.rate_limit.EmailVerificationThrottle",
        "ApiKeyRateThrottle": "plane.api.rate_limit.ApiKeyRateThrottle",
        "AssetRateThrottle": "plane.throttles.asset.AssetRateThrottle",
    }
    classes = {}
    for dotted in paths.values():
        module_name, attribute = dotted.rsplit(".", 1)
        classes[dotted] = getattr(importlib.import_module(module_name), attribute)
    return classes


def build_request(factory, step, anonymous_user):
    """A `RequestFactory` request that carries exactly what a throttle reads."""
    request = factory.get("/")
    if step.get("remote"):
        request.META["REMOTE_ADDR"] = step["remote"]
    else:
        request.META.pop("REMOTE_ADDR", None)
    if step.get("xff") is not None:
        request.META["HTTP_X_FORWARDED_FOR"] = step["xff"]
    if step.get("api_key") is not None:
        request.META["HTTP_X_API_KEY"] = step["api_key"]
    if step.get("authenticated"):
        # `UserRateThrottle` reads `.pk`; `AnonRateThrottle` reads
        # `.is_authenticated`. A real `User` row is not needed for either.
        request.user = SimpleNamespace(
            is_authenticated=True, pk=uuid.UUID(step["user_pk"])
        )
    else:
        request.user = anonymous_user
    return request


def raw_state(client, cache, key):
    """The stored bytes and TTL of a throttle key, or nulls when there is none."""
    if key is None:
        return None, None, None
    full_key = cache.make_key(key)
    value = client.get(full_key)
    ttl = client.pttl(full_key)
    return full_key, value.hex() if value is not None else None, ttl


def run_step(step, context):
    op = step.get("op", "request")
    cache = context["cache"]
    client = context["client"]

    if op == "request":
        names = step["class"]
        if not isinstance(names, list):
            names = [names]
        throttles = [context["classes"][name]() for name in names]
        now = float(step["now"])
        for throttle in throttles:
            throttle.timer = lambda: now

        request = build_request(context["factory"], step, context["anonymous_user"])
        view = SimpleNamespace(kwargs={"asset_id": step.get("asset_id")})

        # `APIView.check_throttles` (`rest_framework/views.py:357-376`): every
        # throttle runs, even after one has failed, and the request is refused
        # with the largest non-None wait.
        failures = []
        for throttle in throttles:
            if not throttle.allow_request(request, view):
                failures.append(throttle.wait())
        durations = [wait for wait in failures if wait is not None]
        wait = max(durations, default=None) if failures else None

        buckets = []
        for throttle in throttles:
            key = getattr(throttle, "key", None)
            if key is None:
                continue
            full_key, value_hex, ttl = raw_state(client, cache, str(key))
            buckets.append(
                {
                    "key": str(key),
                    "full_key": full_key,
                    "value_hex": value_hex,
                    "ttl": ttl,
                }
            )
        first = (
            buckets[0]
            if buckets
            else {"key": None, "full_key": None, "value_hex": None, "ttl": None}
        )

        # `ApiKeyRateThrottle` reads `:1:None` when there is no X-Api-Key
        # (`plane/api/rate_limit.py:38` calls `cache.get(self.key, [])` after
        # `allow_request` returned early); record that bucket too.
        none_key_value = None
        if not buckets:
            value = client.get(cache.make_key(None))
            none_key_value = value.hex() if value is not None else None
        meta = {}
        for name in ("X-RateLimit-Remaining", "X-RateLimit-Reset"):
            if name in request.META:
                meta[name] = request.META[name]
        return {
            "op": op,
            "allowed": not failures,
            "key": first["key"],
            "full_key": first["full_key"],
            "value_hex": first["value_hex"],
            "ttl": first["ttl"],
            "wait": wait,
            "meta": meta,
            "rate": [throttle.num_requests, throttle.duration] if len(throttles) == 1 else None,
            "none_key_value_hex": none_key_value,
            "buckets": buckets,
        }

    if op == "set_history":
        # `RedisCache.set`, i.e. `SET key <pickle> PX <timeout*1000>`.
        cache.set(step["key"], [float(value) for value in step["history"]], step["timeout"])
        full_key, value_hex, ttl = raw_state(client, cache, step["key"])
        return {
            "op": op,
            "allowed": None,
            "key": step["key"],
            "full_key": full_key,
            "value_hex": value_hex,
            "ttl": ttl,
            "wait": None,
            "meta": {},
            "rate": None,
            "none_key_value_hex": None,
            "buckets": [],
        }

    if op == "read":
        full_key, value_hex, ttl = raw_state(client, cache, step["key"])
        return {
            "op": op,
            "allowed": None,
            "key": step["key"],
            "full_key": full_key,
            "value_hex": value_hex,
            "ttl": ttl,
            "wait": None,
            "meta": {},
            "rate": None,
            "none_key_value_hex": None,
            "buckets": [],
        }

    raise SystemExit(f"unknown op {op!r}")


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scenario", help="path to the scenario JSON file")
    parser.add_argument("--from-index", type=int, default=0)
    parser.add_argument("--to-index", type=int, default=None)
    parser.add_argument(
        "--flush",
        action="store_true",
        help="FLUSHDB before running (only ever against an isolated dev DB)",
    )
    arguments = parser.parse_args(argv[1:])

    # `manage.py` puts the project root on `sys.path`; a script started by path
    # does not, so add it explicitly (default: the cwd, as documented above).
    if os.environ.get("PLANE_DJANGO_ROOT"):
        django_root = os.environ["PLANE_DJANGO_ROOT"]
    else:
        # The oracle at the UPSTREAM.toml pin; see tools/upstream/oracle.py.
        sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "upstream"))
        from oracle import api_dir as _oracle_api_dir

        django_root = str(_oracle_api_dir())
    sys.path.insert(0, django_root)
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    import django

    django.setup()

    # Imports must follow `django.setup()`.
    import redis
    from django.conf import settings
    from django.contrib.auth.models import AnonymousUser
    from django.core.cache import cache
    from django.test import RequestFactory
    from rest_framework.settings import api_settings

    from plane.authentication.rate_limit import AuthenticationThrottle

    with open(arguments.scenario, encoding="utf-8") as handle:
        scenario = json.load(handle)

    # `BaseThrottle.get_ident` reads `api_settings.NUM_PROXIES` on every call;
    # the cached settings object accepts a direct assignment, which is how this
    # process can cover more than one scenario.
    api_settings.NUM_PROXIES = scenario.get("num_proxies")

    location = settings.CACHES["default"]["LOCATION"]
    client = redis.Redis.from_url(location)
    database = client.connection_pool.connection_kwargs.get("db", 0)
    if arguments.flush:
        if database not in (9, 10, 11):
            raise SystemExit(
                f"refusing to FLUSHDB database {database}: the interop test only "
                "uses 9-11 (apps/server/AGENTS.md)"
            )
        client.flushdb()

    classes = load_throttle_classes()
    context = {
        "classes": classes,
        "cache": cache,
        "client": client,
        "factory": RequestFactory(),
        "anonymous_user": AnonymousUser(),
    }

    steps = scenario["steps"]
    start = arguments.from_index
    end = len(steps) if arguments.to_index is None else arguments.to_index
    records = [run_step(step, context) for step in steps[start:end]]

    json.dump(
        {
            "redis_url": location,
            "db": database,
            "num_proxies": scenario.get("num_proxies"),
            "api_key_rate_limit": settings.API_KEY_RATE_LIMIT,
            "authentication_rate_limit": AuthenticationThrottle.rate,
            "default_throttle_rates": dict(settings.REST_FRAMEWORK["DEFAULT_THROTTLE_RATES"]),
            "start": start,
            "end": end,
            "steps": records,
        },
        sys.stdout,
        separators=(",", ":"),
    )
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
