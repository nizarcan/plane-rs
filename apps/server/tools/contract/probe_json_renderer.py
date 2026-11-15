# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""The exact JSON bytes each renderer produces.

Two encoders matter:

* DRF's ``JSONRenderer`` + ``rest_framework.utils.encoders.JSONEncoder`` — used
  by every ``Response``.
* Django's ``JsonResponse`` + ``DjangoJSONEncoder`` — used by the handful of
  views that do not go through DRF.

They differ in separators, ``ensure_ascii``, ``timedelta`` and ``Decimal``
handling.  Python's ``float.__repr__`` (shortest round-trip, exponent form
outside ``1e-4 <= |x| < 1e16``) is what both produce for floats; it is not what
``ryu``/``serde_json`` produces for every value, so the table below is the
contract.

Run from ``apps/api``:

    .venv/bin/python ../server/tools/contract/probe_json_renderer.py
"""

import datetime
import decimal
import os
import sys
import uuid

API_DIR = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", "..", "api"))
sys.path.insert(0, API_DIR)

os.environ["DJANGO_SETTINGS_MODULE"] = "plane.settings.production"
os.environ["SECRET_KEY"] = "contract-test-secret-key-not-for-deployment"
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")

import django  # noqa: E402

django.setup()

from django.core.serializers.json import DjangoJSONEncoder  # noqa: E402
from django.http import JsonResponse  # noqa: E402
from django.test import override_settings  # noqa: E402
from django.utils import timezone  # noqa: E402
from rest_framework.renderers import JSONRenderer  # noqa: E402
from rest_framework.utils.encoders import JSONEncoder as DRFJSONEncoder  # noqa: E402

from plane.app.serializers import UserSerializer  # noqa: E402  (import check only)
from rest_framework import serializers  # noqa: E402


def drf(value):
    return JSONRenderer().render(value)


def django_json(value):
    return JsonResponse(value).content


def renderer_floats():
    print("Float representation, Python 3.12 `float.__repr__` -> JSON text")
    values = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.1,
        0.1 + 0.2,
        1.0 / 3.0,
        1e-4,
        1e-5,
        1.234567890123e-5,
        1e-7,
        1e15,
        1e16,
        1e17,
        1234567890123456.0,
        12345678901234567.0,
        1e21,
        1e22,
        1.7976931348623157e308,
        5e-324,
        2.2250738585072014e-308,
        -1.5e-9,
        100.0,
        1e300,
    ]
    for v in values:
        print("  %-24r -> %-24s drf=%s" % (v, repr(v), drf(v).decode()))
    print()


def renderer_specials():
    print("Values JSONRenderer/JSONEncoder special-case")
    now = datetime.datetime(2026, 11, 15, 12, 34, 56, 789012, tzinfo=datetime.timezone.utc)
    naive = datetime.datetime(2026, 11, 15, 12, 34, 56, 123456)
    nonutc = datetime.datetime(2026, 11, 15, 12, 34, 56, 1, tzinfo=datetime.timezone(datetime.timedelta(hours=5, minutes=30)))
    cases = [
        ("datetime UTC + microseconds", now),
        ("datetime UTC no microseconds", now.replace(microsecond=0)),
        ("datetime naive + microseconds", naive),
        ("datetime +05:30 tz", nonutc),
        ("date", datetime.date(2026, 11, 15)),
        ("time", datetime.time(12, 34, 56, 789012)),
        ("timedelta", datetime.timedelta(days=1, seconds=3661, microseconds=500000)),
        ("Decimal('1.10')", decimal.Decimal("1.10")),
        ("Decimal('1E+3')", decimal.Decimal("1E+3")),
        ("Decimal('0.000001')", decimal.Decimal("0.000001")),
        ("UUID", uuid.UUID("0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0")),
        ("bytes", b"abc"),
        ("set", {"b", "a"}),
        ("frozenset", frozenset(["z"])),
        ("generator", (x for x in range(3))),
        ("tuple", (1, 2)),
        ("nested", {"a": [1, {"b": None}], "c": True}),
        ("U+2028 in a string", "line\u2028sep\u2029end"),
        ("non-ASCII", "Zoë — 日本語 🚀"),
        ("control chars", "tab\tnl\nquote\"backslash\\"),
    ]
    for label, value in cases:
        try:
            out = drf(value)
        except Exception as exc:  # noqa: BLE001
            out = ("RAISED %s: %s" % (type(exc).__name__, exc)).encode()
        try:
            dj = django_json({"k": value}) if not isinstance(value, (set, frozenset)) else b"(unhashable/set)"
        except Exception as exc:  # noqa: BLE001
            dj = ("RAISED %s: %s" % (type(exc).__name__, exc)).encode()
        print("  %-32s drf=%-46s django=%s" % (label, out.decode("utf-8", "backslashreplace"), dj.decode("utf-8", "backslashreplace")))
    print()

    print("NaN / Infinity (STRICT_JSON=True -> allow_nan=False):")
    for label, value in [("NaN", float("nan")), ("+inf", float("inf")), ("-inf", float("-inf"))]:
        try:
            out = drf({"v": value})
        except ValueError as exc:
            out = ("ValueError: %s" % exc).encode()
        print("  %-6s -> %s" % (label, out.decode()))
    print()


def renderer_serializer_fields():
    class Demo(serializers.Serializer):
        dt = serializers.DateTimeField()
        d = serializers.DateField()
        t = serializers.TimeField()
        dur = serializers.DurationField()
        dec = serializers.DecimalField(max_digits=12, decimal_places=4)
        uid = serializers.UUIDField()
        f = serializers.FloatField()
        i = serializers.IntegerField()
        b = serializers.BooleanField()
        s = serializers.CharField()

    print("Serializer field to_representation (Plane uses the defaults from PLANE REST_FRAMEWORK)")
    instance = {
        "dt": datetime.datetime(2026, 11, 15, 12, 34, 56, 120000, tzinfo=datetime.timezone.utc),
        "d": datetime.date(2026, 11, 15),
        "t": datetime.time(12, 34, 56, 789012),
        "dur": datetime.timedelta(hours=2, minutes=3),
        "dec": decimal.Decimal("1.1000"),
        "uid": uuid.UUID("0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0"),
        "f": 1e-05,
        "i": 7,
        "b": False,
        "s": "x",
    }
    serializer = Demo(instance)
    print("  serialized =", drf(serializer.data).decode())
    print()
    print("  DateTimeField ISO_8601 -> isoformat() with '+00:00' replaced by 'Z';")
    print("  microseconds are included only when non-zero (isoformat default).")
    print("  DurationField -> 'HH:MM:SS[.ffffff]'.")
    print("  DecimalField(coerce_to_string=True) -> f'{quantized:f}' -> trailing zeros kept.")
    print()

    for label, value in [
        ("aware UTC, microseconds", datetime.datetime(2026, 11, 15, 12, 34, 56, 120000, tzinfo=datetime.timezone.utc)),
        ("aware UTC, no microseconds", datetime.datetime(2026, 11, 15, 12, 34, 56, tzinfo=datetime.timezone.utc)),
        ("aware +05:30", datetime.datetime(2026, 11, 15, 12, 34, 56, 1, tzinfo=datetime.timezone(datetime.timedelta(hours=5, minutes=30)))),
        ("naive (made aware in current tz)", datetime.datetime(2026, 11, 15, 12, 34, 56)),
        ("None", None),
    ]:
        field = serializers.DateTimeField()
        print("  DateTimeField %-34s -> %r" % (label, field.to_representation(value)))
    print()


def renderer_ordering():
    print("Key order: dict insertion order, never sorted.")
    print("  drf =", drf({"z": 1, "a": 2, "m": 3}).decode())
    print("  django =", django_json({"z": 1, "a": 2, "m": 3}).decode())
    print()
    print("Empty structures:")
    for label, value in [("{}", {}), ("[]", []), ("None", None), ("''", ""), ("0", 0), ("False", False)]:
        print("  %-6s drf=%-8r django=%r" % (label, drf(value), django_json({"k": value})))
    print()


def main():
    print("Django", django.get_version())
    print("ensure_ascii (DRF) =", JSONRenderer.ensure_ascii, " compact =", JSONRenderer.compact, " strict =", JSONRenderer.strict)
    print("separators         =", (",", ":"))
    print("JSONEncoder classes: drf =", DRFJSONEncoder.__module__ + "." + DRFJSONEncoder.__name__)
    print("                     django =", DjangoJSONEncoder.__module__ + "." + DjangoJSONEncoder.__name__)
    print()
    renderer_floats()
    renderer_specials()
    renderer_ordering()

    class Demo(serializers.Serializer):
        dt = serializers.DateTimeField()

    current = django.utils.timezone.get_current_timezone() if False else None  # noqa: F841
    print("TIME_ZONE =", __import__("django.conf", fromlist=["settings"]).settings.TIME_ZONE)
    print("USE_TZ    =", __import__("django.conf", fromlist=["settings"]).settings.USE_TZ)
    print()

    renderer_serializer_fields()
    return 0


if __name__ == "__main__":
    sys.exit(main())
