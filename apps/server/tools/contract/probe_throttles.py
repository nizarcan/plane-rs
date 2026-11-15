# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Rate-limit identities, cache keys and the redis value format.

Nothing here connects to Redis: the key is computed with the same function
Django's cache backend uses (``default_key_func``) and the value with the same
serializer django-redis defaults to (``PickleSerializer``).

Run from ``apps/api``:

    .venv/bin/python ../server/tools/contract/probe_throttles.py
"""

import os
import pickle
import sys

API_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", "api"))
sys.path.insert(0, API_DIR)

os.environ["DJANGO_SETTINGS_MODULE"] = "plane.settings.production"
os.environ["SECRET_KEY"] = "contract-test-secret-key-not-for-deployment"
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")

import django  # noqa: E402

django.setup()

from django.core.cache import caches  # noqa: E402
from django.core.cache.backends.base import default_key_func  # noqa: E402
from django.test import override_settings  # noqa: E402
from django.test import RequestFactory  # noqa: E402
from rest_framework.settings import api_settings  # noqa: E402
from rest_framework.throttling import AnonRateThrottle, ScopedRateThrottle  # noqa: E402
from django_redis.serializers.pickle import PickleSerializer  # noqa: E402
from django_redis.compressors.identity import IdentityCompressor  # noqa: E402

from plane.api.rate_limit import ApiKeyRateThrottle  # noqa: E402
from plane.authentication.rate_limit import AuthenticationThrottle, EmailVerificationThrottle  # noqa: E402
from plane.throttles.asset import AssetRateThrottle  # noqa: E402


class FakeUser:
    def __init__(self, pk, authenticated=True):
        self.pk = pk
        self.is_authenticated = authenticated


class FakeView:
    def __init__(self, kwargs=None):
        self.kwargs = kwargs or {}


def main():
    print("Django", django.get_version())
    print()
    print("REST_FRAMEWORK throttling settings:")
    print("  DEFAULT_THROTTLE_CLASSES =", api_settings.DEFAULT_THROTTLE_CLASSES)
    print("  DEFAULT_THROTTLE_RATES   =", api_settings.DEFAULT_THROTTLE_RATES)
    print("  NUM_PROXIES              =", api_settings.NUM_PROXIES)
    print("  API_KEY_RATE_LIMIT       =", __import__("django.conf", fromlist=["settings"]).settings.API_KEY_RATE_LIMIT)
    print("  AUTHENTICATION_RATE_LIMIT=", os.environ.get("AUTHENTICATION_RATE_LIMIT", "10/minute (default)"))
    print()

    print("Cache backend:")
    cache = caches["default"]
    print("  class        =", type(cache).__module__ + "." + type(cache).__name__)
    print("  LOCATION     =", cache._server if hasattr(cache, "_server") else cache.location)
    print("  KEY_PREFIX   =", repr(cache.key_prefix))
    print("  VERSION      =", cache.version)
    print("  KEY_FUNCTION =", cache.key_func.__module__ + "." + cache.key_func.__name__)
    print("  TIMEOUT      =", cache.default_timeout)
    print("  serialized key for 'throttle_anon_1.2.3.4':", cache.make_key("throttle_anon_1.2.3.4"))
    print("  default_key_func('k', '', 1) =", default_key_func("k", "", 1))
    print()
    print("  django-redis OPTIONS in settings:", __import__("django.conf", fromlist=["settings"]).settings.CACHES["default"]["OPTIONS"])
    print("  (no SERIALIZER/COMPRESSOR/PICKLE_VERSION key -> PickleSerializer, IdentityCompressor,")
    print("   pickle.DEFAULT_PROTOCOL)")
    print()

    serializer = PickleSerializer({})
    print("Value encoding (PickleSerializer with PICKLE_VERSION unset):")
    print("  pickle.DEFAULT_PROTOCOL   =", pickle.DEFAULT_PROTOCOL)
    print("  pickle.HIGHEST_PROTOCOL   =", pickle.HIGHEST_PROTOCOL)
    for value in (
        [1789000000.123456],
        [1789000000.123456, 1788999940.0],
        [],
        [1789000000.0],
    ):
        raw = serializer.dumps(value)
        print("  %-44r -> %d bytes  %s" % (value, len(raw), raw.hex()))
    print("  IdentityCompressor.compress == identity:", IdentityCompressor({}).compress(b"ab") == b"ab")
    print()
    print("Integers are NOT pickled (DefaultClient.encode(allow_int=True) returns them raw),")
    print("but a list history is always pickled. Only list values are written by throttles.")
    print()

    print("Identities (BaseThrottle.get_ident):")
    rf = RequestFactory()
    scenarios = [
        ("no XFF, NUM_PROXIES unset", {"REMOTE_ADDR": "203.0.113.7"}, None),
        ("no XFF, NUM_PROXIES=1", {"REMOTE_ADDR": "203.0.113.7"}, 1),
        ("XFF='1.1.1.1, 2.2.2.2', NUM_PROXIES unset", {"REMOTE_ADDR": "10.0.0.1", "HTTP_X_FORWARDED_FOR": "1.1.1.1, 2.2.2.2"}, None),
        ("XFF='1.1.1.1, 2.2.2.2', NUM_PROXIES=1", {"REMOTE_ADDR": "10.0.0.1", "HTTP_X_FORWARDED_FOR": "1.1.1.1, 2.2.2.2"}, 1),
        ("XFF='1.1.1.1, 2.2.2.2', NUM_PROXIES=2", {"REMOTE_ADDR": "10.0.0.1", "HTTP_X_FORWARDED_FOR": "1.1.1.1, 2.2.2.2"}, 2),
        ("XFF=' 1.1.1.1 ,  2.2.2.2 ', NUM_PROXIES=2", {"REMOTE_ADDR": "10.0.0.1", "HTTP_X_FORWARDED_FOR": " 1.1.1.1 ,  2.2.2.2 "}, 2),
        ("XFF present, NUM_PROXIES=0", {"REMOTE_ADDR": "10.0.0.1", "HTTP_X_FORWARDED_FOR": "1.1.1.1"}, 0),
        ("XFF has more hops than NUM_PROXIES=5", {"REMOTE_ADDR": "10.0.0.1", "HTTP_X_FORWARDED_FOR": "1.1.1.1, 2.2.2.2"}, 5),
    ]
    for label, extra, num_proxies in scenarios:
        throttles = {"anon": "30/minute", "asset_id": "5/minute"}
        with override_settings(
            REST_FRAMEWORK={
                "NUM_PROXIES": num_proxies,
                "DEFAULT_THROTTLE_RATES": throttles,
            }
        ):
            request = rf.get("/", **extra)
            ident = AnonRateThrottle().get_ident(request)
        print("  %-46s NUM_PROXIES=%-4r -> %r" % (label, num_proxies, ident))
    print()

    print("Throttle classes and cache keys (cache_format = 'throttle_%(scope)s_%(ident)s'):")
    with override_settings(
        REST_FRAMEWORK={"NUM_PROXIES": None, "DEFAULT_THROTTLE_RATES": {"anon": "30/minute", "asset_id": "5/minute"}}
    ):
        request = rf.get("/", REMOTE_ADDR="203.0.113.7")
        request.user = FakeUser("u", authenticated=False)
        print("  AnonRateThrottle(anon)          key =", AnonRateThrottle().get_cache_key(request, None))
        print("    -> full redis key             =", cache.make_key(AnonRateThrottle().get_cache_key(request, None)))
        request.user = FakeUser("8f1c...")
        print("  AnonRateThrottle(authenticated)= ", AnonRateThrottle().get_cache_key(request, None))
        print("  AuthenticationThrottle          key =", AuthenticationThrottle().get_cache_key(request, None))
        print("  EmailVerificationThrottle       key =", EmailVerificationThrottle().get_cache_key(request, None))
        request.user = FakeUser("u", authenticated=False)
        print("  EmailVerificationThrottle(anon) key =", EmailVerificationThrottle().get_cache_key(request, None))
        print("  AssetRateThrottle               key =", AssetRateThrottle().get_cache_key(rf.get("/"), FakeView({"asset_id": "a1b2"})))
        print("  AssetRateThrottle(no asset_id)  key =", AssetRateThrottle().get_cache_key(rf.get("/"), FakeView({})))
        print("    -> full redis key             =", cache.make_key("throttle_asset_a1b2"))
        api_key_request = rf.get("/", HTTP_X_API_KEY="plane_api_abc123")
        print("  ApiKeyRateThrottle              key =", ApiKeyRateThrottle().get_cache_key(api_key_request, None))
        print("    -> full redis key             =", cache.make_key(ApiKeyRateThrottle().get_cache_key(api_key_request, None)))
        print("  ApiKeyRateThrottle(no header)   key =", ApiKeyRateThrottle().get_cache_key(rf.get("/"), None))

    print()
    print("ScopedRateThrottle is not used by any Plane view (no throttle_scope attribute).")
    print("  ScopedRateThrottle scope None -> allow_request returns True:", ScopedRateThrottle().allow_request(rf.get("/"), FakeView()))
    print()
    print("Rates parsed by SimpleRateThrottle.parse_rate:")
    for rate in ("30/minute", "5/minute", "10/minute", "3/hour", "60/minute"):
        throttle = AnonRateThrottle()
        print("  %-10s -> %r" % (rate, throttle.parse_rate(rate)))
    print()
    print("History window arithmetic (SimpleRateThrottle.allow_request):")
    print("  history holds timestamps, newest first; entries with ts <= now - duration are dropped;")
    print("  429 when len(history) >= num_requests; cache.set(key, history, timeout=duration).")
    print("  wait() = (duration - (now - history[-1])) / (num_requests - len(history) + 1)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
