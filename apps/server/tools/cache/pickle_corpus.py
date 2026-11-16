# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record the exact bytes `pickle.dumps(value, 4)` writes for cached values.

The Rust codec (`crates/plane-cache/src/pickle.rs`) must reproduce CPython's
opcode stream byte for byte, including the pickler's object-identity memo, the
frame framing rules and the `__reduce__` protocols of `uuid.UUID`,
`datetime.*` and `decimal.Decimal`. This tool is the oracle: it builds a corpus
of values (including the real `.data` of the five `@cache_response` views),
records

  * `tree`  — the value annotated with object identity (`{"r": n}` for a repeat
              reference) and with DRF's `ReturnList`/`ReturnDict` wrappers;
  * `value` — the same value with identity inlined and the wrappers stripped,
              i.e. what `pickle.loads` gives back;
  * `hex`   — `binascii.hexlify(pickle.dumps(value, 4))`.

`crates/plane-cache/tests/golden.rs` encodes `tree`, compares `hex`, decodes
`hex` and compares `value`.

Usage (from `apps/api`):

    PYTHONPATH=. .venv/bin/python ../server/tools/cache/pickle_corpus.py \
        --out ../server/crates/plane-cache/tests/golden/pickle-corpus.json

The script opens no database connection and no Redis connection. It does
import Django so that the real serializers can be used; Django's model
instances are built unsaved.
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

# `plane.celery` builds a redis client at import time from REDIS_URL; nothing
# connects. The values are the dev stack's, so an accidental connection can
# only reach an isolated database.
os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:56379/13")
os.environ.setdefault("DATABASE_URL", "postgres://plane:plane@127.0.0.1:55432/plane_tpl")

PROTOCOL = pickle.DEFAULT_PROTOCOL


# ---------------------------------------------------------------------------
# Annotation
# ---------------------------------------------------------------------------

# Types CPython's pickler memoises: everything dispatched through `save_reduce`
# plus the built-in containers and `str`/`bytes`. `int`, `float`, `bool` and
# `None` are never memoised, and an empty tuple is not either (`save_tuple`
# returns before `memoize`).
MEMO_TYPES = (
    str,
    bytes,
    list,
    tuple,
    dict,
    set,
    frozenset,
    uuid.UUID,
    datetime.datetime,
    datetime.date,
    datetime.time,
    datetime.timedelta,
    datetime.timezone,
    decimal.Decimal,
)


class Annotator:
    def annotate(self, obj):
        self.next_index = 0
        self._visiting: dict[int, int] = {}
        return self._walk(obj)

    def _walk(self, obj):
        if isinstance(obj, tuple) and not obj:
            return {"t": []}
        if isinstance(obj, MEMO_TYPES):
            key = id(obj)
            if key in self._visiting:
                return {"r": self._visiting[key]}
            self._visiting[key] = self.next_index
            self.next_index += 1
            return self._emit(obj)
        return self._emit(obj)

    def _emit(self, obj):
        if obj is None:
            return None
        if obj is True:
            return True
        if obj is False:
            return False
        if isinstance(obj, bool):  # pragma: no cover - covered above
            return obj
        if isinstance(obj, int):
            return {"i": str(obj)}
        if isinstance(obj, float):
            return {"f": repr(obj)}
        if isinstance(obj, str):
            # JSON text cannot carry a lone surrogate, so such a string is
            # recorded as the exact WTF-8 bytes `save_str` writes
            # (`surrogatepass`).
            try:
                obj.encode("utf-8")
            except UnicodeEncodeError:
                return {"s8": obj.encode("utf-8", "surrogatepass").hex()}
            return {"s": obj}
        if isinstance(obj, bytes):
            return {"b": obj.hex()}
        # DRF's ReturnList / ReturnDict: `__reduce__` is `list(self)`/`dict(self)`.
        # Checked before `list`/`dict` because they subclass them.
        from rest_framework.utils.serializer_helpers import ReturnDict, ReturnList

        if isinstance(obj, ReturnList):
            return {"L": {"func": "list", "v": self._walk(list(obj))}}
        if isinstance(obj, ReturnDict):
            return {"L": {"func": "dict", "v": self._walk(dict(obj))}}
        if isinstance(obj, list):
            return {"l": [self._walk(item) for item in obj]}
        if isinstance(obj, tuple):
            return {"t": [self._walk(item) for item in obj]}
        if isinstance(obj, dict):
            return {"d": [[self._walk(k), self._walk(v)] for k, v in obj.items()]}
        if isinstance(obj, set):
            return {"e": [self._walk(item) for item in obj]}
        if isinstance(obj, frozenset):
            return {"z": [self._walk(item) for item in obj]}
        if isinstance(obj, uuid.UUID):
            return {"u": str(obj)}
        if isinstance(obj, datetime.datetime):
            return {
                "T": {
                    "n": obj.replace(tzinfo=None).isoformat(),
                    "fold": bool(obj.fold),
                    "tz": None if obj.tzinfo is None else _tz_node(obj.tzinfo),
                }
            }
        if isinstance(obj, datetime.date):
            return {"D": obj.isoformat()}
        if isinstance(obj, datetime.time):
            return {"H": obj.isoformat()}
        if isinstance(obj, datetime.timedelta):
            return {
                "X": {
                    "days": obj.days,
                    "seconds": obj.seconds,
                    "microseconds": obj.microseconds,
                }
            }
        if isinstance(obj, datetime.timezone):
            return {"Z": _tz_value(obj)}
        if isinstance(obj, decimal.Decimal):
            return {"M": str(obj)}
        raise TypeError(f"cannot annotate {type(obj)!r}")


def _tz_value(tz: datetime.timezone) -> object:
    offset = tz.utcoffset(None)
    if offset == datetime.timedelta(0) and tz is datetime.timezone.utc:
        return "utc"
    if offset == datetime.timedelta(0):
        return "utc"
    if tz.tzname(None) not in (None, "UTC", f"UTC{_offset_text(offset)}"):
        raise TypeError("named timezones are not modelled")
    return int(offset.total_seconds())


def _offset_text(offset: datetime.timedelta) -> str:
    total = int(offset.total_seconds())
    sign = "+" if total >= 0 else "-"
    total = abs(total)
    return f"{sign}{total // 3600:02d}:{total % 3600 // 60:02d}"


def _tz_node(tz) -> object:
    if not isinstance(tz, datetime.timezone):
        raise TypeError(f"unsupported tzinfo {tz!r}")
    return {"Z": _tz_value(tz)}


class Plain:
    """The decoded value: identity inlined, DRF wrappers stripped."""

    def plain(self, obj):
        if obj is None or isinstance(obj, (bool, int, float, str, bytes)):
            return obj
        from rest_framework.utils.serializer_helpers import ReturnDict, ReturnList

        if isinstance(obj, ReturnList):
            return [self.plain(item) for item in obj]
        if isinstance(obj, ReturnDict):
            return {self.plain(k): self.plain(v) for k, v in obj.items()}
        if isinstance(obj, list):
            return [self.plain(item) for item in obj]
        if isinstance(obj, tuple):
            return tuple(self.plain(item) for item in obj)
        if isinstance(obj, dict):
            return {self.plain(k): self.plain(v) for k, v in obj.items()}
        if isinstance(obj, set):
            return {self.plain(item) for item in obj}
        if isinstance(obj, frozenset):
            return frozenset(self.plain(item) for item in obj)
        if isinstance(obj, uuid.UUID):
            return obj
        if isinstance(obj, (datetime.datetime, datetime.date, datetime.time)):
            return obj
        if isinstance(obj, (datetime.timedelta, datetime.timezone, decimal.Decimal)):
            return obj
        raise TypeError(f"cannot copy {type(obj)!r}")


# ---------------------------------------------------------------------------
# Cases
# ---------------------------------------------------------------------------


def case(name, value, cases, annotator, plain, seen_names):
    assert name not in seen_names, f"duplicate case {name}"
    seen_names.add(name)
    payload = pickle.dumps(value, PROTOCOL)
    # Record the identity-annotated tree and the decoded value from *different*
    # walks: the annotated one must see the same `id()`s the pickler saw.
    tree = annotator.annotate(value)
    value_tree = Annotator().annotate(plain.plain(value))
    cases.append(
        {
            "name": name,
            "group": GROUP,
            "tree": tree,
            "value": value_tree,
            "hex": binascii.hexlify(payload).decode(),
        }
    )


GROUP = "misc"


def group(name):
    global GROUP
    GROUP = name


def scalar_cases(cases, annotator, plain, seen):
    group("scalars")
    scalars = [
        ("none", None),
        ("true", True),
        ("false", False),
        ("int-zero", 0),
        ("int-one", 1),
        ("int-255", 255),
        ("int-256", 256),
        ("int-65535", 65535),
        ("int-65536", 65536),
        ("int-neg-one", -1),
        ("int-neg-128", -128),
        ("int-neg-129", -129),
        ("int-i32-max", 2**31 - 1),
        ("int-i32-min", -(2**31)),
        ("int-i32-over", 2**31),
        ("int-i32-under", -(2**31) - 1),
        ("int-i64-max", 2**63 - 1),
        ("int-i64-min", -(2**63)),
        ("int-u64-max", 2**64 - 1),
        ("int-2-100", 2**100),
        ("int-neg-2-100", -(2**100)),
        ("int-2-256", 2**256),
        ("float-zero", 0.0),
        ("float-neg-zero", -0.0),
        ("float-one", 1.0),
        ("float-half", 0.5),
        ("float-1e-5", 1e-5),
        ("float-1e16", 1e16),
        ("float-min-subnormal", 5e-324),
        ("float-max", 1.7976931348623157e308),
        ("float-neg", -1.5e-9),
        ("float-65535", 65535.0),
        ("float-inf", float("inf")),
        ("float-neg-inf", float("-inf")),
        ("float-nan", float("nan")),
        ("str-empty", ""),
        ("str-ascii", "bug"),
        ("str-unicode", "caf\u00e9 \u2014 \u4f60\u597d \ud83d\ude00"),
        ("str-line-separators", "a\u2028b\u2029c"),
        ("str-controls", "a\x00b\nc\td"),
        ("str-255", "a" * 255),
        ("str-256", "a" * 256),
        ("str-65535", "a" * 65535),
        ("str-65536", "a" * 65536),
        ("bytes-empty", b""),
        ("bytes-short", b"\x00\x01\xfe\xff"),
        ("bytes-255", b"x" * 255),
        ("bytes-256", b"x" * 256),
        ("bytes-65535", b"y" * 65535),
        ("bytes-65536", b"z" * 65536),
        ("decimal-simple", decimal.Decimal("1.50")),
        ("decimal-negative", decimal.Decimal("-0.000")),
        ("decimal-integer", decimal.Decimal("42")),
        ("decimal-large", decimal.Decimal("1" + "0" * 40)),
        ("decimal-long-fraction", decimal.Decimal("3." + "1" * 40)),
        ("decimal-exponent", decimal.Decimal("1E+10")),
        ("uuid-int-1", uuid.UUID(int=1)),
        (
            "uuid-version-4",
            uuid.UUID("3d3f41e2-3663-409b-bc3d-5aad9e85f208"),
        ),
        (
            "uuid-top-bit",
            uuid.UUID("ffffffff-ffff-ffff-ffff-ffffffffffff"),
        ),
        ("date-simple", datetime.date(2026, 11, 16)),
        ("date-leap", datetime.date(2024, 2, 29)),
        ("date-min", datetime.date(1, 1, 1)),
        ("date-max", datetime.date(9999, 12, 31)),
        ("time-simple", datetime.time(1, 2, 3)),
        ("time-micros", datetime.time(1, 2, 3, 4000)),
        ("time-midnight", datetime.time(0, 0, 0)),
        (
            "datetime-naive",
            datetime.datetime(2026, 11, 16, 12, 0, 0),
        ),
        (
            "datetime-naive-micros",
            datetime.datetime(2026, 11, 16, 12, 0, 0, 123456),
        ),
        ("datetime-fold", datetime.datetime(2026, 11, 16, 12, 0, 0, fold=1)),
        (
            "datetime-utc",
            datetime.datetime(2026, 11, 16, 12, 0, 0, tzinfo=datetime.timezone.utc),
        ),
        (
            "datetime-utc-micros",
            datetime.datetime(
                2026, 11, 16, 12, 0, 0, 654321, tzinfo=datetime.timezone.utc
            ),
        ),
        (
            "datetime-offset",
            datetime.datetime(
                2026,
                11,
                16,
                12,
                0,
                0,
                tzinfo=datetime.timezone(datetime.timedelta(hours=5, minutes=30)),
            ),
        ),
        (
            "datetime-negative-offset",
            datetime.datetime(
                2026,
                1,
                2,
                3,
                4,
                5,
                6,
                tzinfo=datetime.timezone(-datetime.timedelta(hours=8)),
            ),
        ),
        ("timedelta-zero", datetime.timedelta(0)),
        (
            "timedelta-mixed",
            datetime.timedelta(days=2, seconds=3, microseconds=4),
        ),
        ("timezone-utc", datetime.timezone.utc),
        (
            "timezone-offset",
            datetime.timezone(datetime.timedelta(minutes=-90)),
        ),
        ("tuple-empty", ()),
        ("tuple-one", (1,)),
        ("tuple-two", (1, "a")),
        ("tuple-three", (1, "a", None)),
        ("tuple-four", (1, "a", None, True)),
        ("list-empty", []),
        ("dict-empty", {}),
        ("set-empty", set()),
        ("frozenset-empty", frozenset()),
        ("set-ints", {3, 1, 2}),
        ("frozenset-ints", frozenset({1, 2, 3})),
    ]
    for name, value in scalars:
        case(name, value, cases, annotator, plain, seen)


def container_cases(cases, annotator, plain, seen):
    group("containers")
    containers = [
        ("list-one", [1]),
        ("list-two", [1, 2]),
        ("list-three", [1, 2, 3]),
        ("list-nested", [[1, [2, [3]]]]),
        ("list-mixed", [None, True, False, 0, 1.5, "x", b"y", (1,), {1: 2}]),
        ("list-1000", list(range(1000))),
        ("list-1001", list(range(1001))),
        ("list-2500", list(range(2500))),
        ("dict-one", {"a": 1}),
        ("dict-two", {"a": 1, "b": 2}),
        ("dict-1000", {f"k{index}": index for index in range(1000)}),
        ("dict-nested", {"a": {"b": {"c": [1, 2, {"d": None}]}}}),
        ("dict-int-keys", {1: "one", 2.5: "two-five", None: "none", True: "true"}),
        ("tuple-nested", ((1, 2), (3, (4, 5)))),
        ("set-mixed", {1, 2, 3, 4, 5}),
        ("set-frozenset", {frozenset({1, 2})}),  # order of one
        ("frozenset-nested", frozenset({frozenset({1}), frozenset({2})})),
        ("list-of-empty-tuples", [(), (), ()]),
        ("list-of-empty-lists", [[], [], []]),
        ("dict-of-empty-dicts", {"a": {}, "b": {}}),
        ("long-list", [f"item-{index}" for index in range(1200)]),
        ("deep-nesting", _deep(40)),
    ]
    for name, value in containers:
        case(name, value, cases, annotator, plain, seen)


def _deep(depth):
    value = "bottom"
    for _ in range(depth):
        value = {"next": value}
    return value


def memo_cases(cases, annotator, plain, seen):
    """Object identity: the pickler writes `BINGET` for a repeat reference."""
    group("memo")
    shared_str = "shared"
    shared_list = [1, 2]
    shared_dict = {"k": "v"}
    shared_uuid = uuid.UUID("12345678-1234-5678-1234-567812345678")
    shared_date = datetime.date(2026, 11, 16)
    shared_datetime = datetime.datetime(
        2026, 11, 16, 12, 0, 0, tzinfo=datetime.timezone.utc
    )
    shared_tuple = (1, 2, 3)
    shared_decimal = decimal.Decimal("2.50")
    cases_ = [
        ("memo-str", [shared_str, shared_str]),
        ("memo-str-equal-not-shared", ["shared", "".join(["sha", "red"])]),
        ("memo-list", [shared_list, shared_list]),
        ("memo-dict", {"a": shared_dict, "b": shared_dict}),
        ("memo-uuid", [shared_uuid, shared_uuid]),
        ("memo-date", [shared_date, shared_date]),
        ("memo-datetime", [shared_datetime, shared_datetime]),
        ("memo-tuple", [shared_tuple, shared_tuple]),
        ("memo-decimal", [shared_decimal, shared_decimal]),
        ("memo-empty-tuple-not-shared", [(), ()]),
        (
            "memo-across-nesting",
            {"outer": [shared_list, {"inner": shared_list}]},
        ),
        (
            "memo-uuid-as-parent",
            {
                "id": shared_uuid,
                "parent": shared_uuid,
            },
        ),
    ]
    for name, value in cases_:
        case(name, value, cases, annotator, plain, seen)


# A pool of `str` objects shared across every row, the way a DRF serializer
# shares `field.field_name` between rows.
FIELD_NAMES: dict[str, str] = {}


def field(name: str) -> str:
    return FIELD_NAMES.setdefault(name, sys.intern(name))


def keyed(**values):
    """A dict whose keys are the shared field-name objects."""
    return {field(name): value for name, value in values.items()}


def uuid_for(counter: int) -> uuid.UUID:
    return uuid.UUID(int=0x0102030405060708090A0B0C0D0E0F00 + counter)


def realistic_cases(cases, annotator, plain, seen):
    """Payloads shaped like the five `@cache_response` views."""
    group("drf-shapes")
    for rows in (0, 1, 2, 3, 5, 20):
        payload = _labels_payload(rows, base=1000)
        case(f"labels-{rows}-rows", payload, cases, annotator, plain, seen)
        payload = _estimates_payload(rows, base=2000)
        case(f"estimates-{rows}-rows", payload, cases, annotator, plain, seen)
        payload = _configurations_payload(rows, base=3000)
        case(f"configurations-{rows}-rows", payload, cases, annotator, plain, seen)
        payload = _admins_payload(rows, base=4000)
        case(f"admins-{rows}-rows", payload, cases, annotator, plain, seen)
    for name, payload in (
        ("instances-no-instance", _instance_payload(None)),
        ("instances-configured", _instance_payload(_instance_object(5000))),
    ):
        case(name, payload, cases, annotator, plain, seen)

    # Field-value variations for the same shape.
    variants = [
        ("empty-string", ""),
        ("unicode", "caf\u00e9 \u2014 \u4f60\u597d"),
        ("line-separators", "a\u2028b"),
        ("long", "x" * 300),
        ("hex-color", "#abcdef"),
        ("spaces", "a b c"),
    ]
    for index, (name, text) in enumerate(variants):
        payload = _labels_payload(2, base=6000 + index * 100, name=text, color=text)
        case(f"labels-value-{name}", payload, cases, annotator, plain, seen)


def _labels_payload(rows: int, *, base: int, name="bug", color="#ff0000"):
    """`WorkspaceLabelsEndpoint` → `LabelSerializer(labels, many=True).data`."""
    items = []
    for index in range(rows):
        parent = uuid_for(base + index - 1) if index else None
        items.append(
            keyed(
                parent=parent,
                name=f"{name}-{index}" if rows > 1 else name,
                color=color,
                id=uuid_for(base + index),
                project_id=uuid_for(base + 900),
                workspace_id=uuid_for(base + 901),
                sort_order=float(65535 - index),
            )
        )
    return _envelope(items, wrapped=True)


def _estimates_payload(rows: int, *, base: int):
    """`WorkspaceEstimatesEndpoint` → `WorkspaceEstimateSerializer(..., many=True)`."""
    items = []
    for index in range(rows):
        points = [
            keyed(
                id=uuid_for(base + 500 + index * 10 + point),
                created_at="2026-01-01T00:00:00Z",
                updated_at="2026-01-02T03:04:05.123456Z",
                deleted_at=None,
                key=point,
                description="",
                value=str(point + 1),
                created_by=None,
                updated_by=None,
                project_id=uuid_for(base + 900),
                workspace_id=uuid_for(base + 901),
                estimate_id=uuid_for(base + index),
            )
            for point in range(2)
        ]
        items.append(
            keyed(
                id=uuid_for(base + index),
                points=points,
                created_at="2026-01-01T00:00:00Z",
                updated_at="2026-01-01T00:00:00Z",
                deleted_at=None,
                name=f"Estimate {index}",
                description="Sizing",
                type="categories",
                last_used=bool(index % 2),
                created_by=None,
                updated_by=uuid_for(base + 800) if index == 0 else None,
                project_id=uuid_for(base + 900),
                workspace_id=uuid_for(base + 901),
            )
        )
    return _envelope(items, wrapped=True)


def _configurations_payload(rows: int, *, base: int):
    """`InstanceConfigurationEndpoint` → `InstanceConfigurationSerializer(..., many=True)`."""
    items = []
    for index in range(rows):
        items.append(
            keyed(
                id=uuid_for(base + index),
                created_at="2026-01-01T00:00:00Z",
                updated_at="2026-01-01T00:00:00Z",
                deleted_at=None,
                key=f"CONFIG_{index}",
                value=str(index),
                category="general",
                is_encrypted=False,
                created_by=None,
                updated_by=None,
            )
        )
    return _envelope(items, wrapped=True)


def _admins_payload(rows: int, *, base: int):
    """`InstanceAdminEndpoint` → `InstanceAdminSerializer(..., many=True)`."""
    items = []
    for index in range(rows):
        items.append(
            keyed(
                id=uuid_for(base + index),
                user_detail={
                    field("id"): uuid_for(base + 500 + index),
                    field("first_name"): "Ada",
                    field("last_name"): "Lovelace",
                    field("avatar"): "",
                    field("avatar_url"): None,
                    field("is_bot"): False,
                    field("display_name"): "Ada Lovelace",
                    field("email"): f"admin{index}@example.com",
                    field("last_login_medium"): "email",
                },
                created_at="2026-01-01T00:00:00Z",
                updated_at="2026-01-01T00:00:00Z",
                deleted_at=None,
                role=20,
                is_verified=True,
                created_by=None,
                updated_by=None,
                user=uuid_for(base + 500 + index),
                instance=uuid_for(base + 900),
            )
        )
    return _envelope(items, wrapped=True)


def _instance_object(base: int):
    return {
        "id": uuid_for(base),
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "deleted_at": None,
        "instance_name": "Acme",
        "whitelist_emails": "",
        "instance_id": str(uuid_for(base + 1)),
        "current_version": "1.2.3",
        "latest_version": "1.3.0",
        "edition": "CE",
        "domain": "",
        "last_checked_at": "2026-01-01T00:00:00Z",
        "namespace": "",
        "is_telemetry_enabled": False,
        "is_support_required": False,
        "is_setup_done": True,
        "is_signup_screen_visited": True,
        "is_verified": False,
        "is_test": False,
        "is_current_version_deprecated": False,
        "created_by": None,
        "updated_by": None,
    }


def _instance_payload(instance):
    """`InstanceEndpoint.get` → `{"config": {...}, "instance": ReturnDict}`."""
    from rest_framework.utils.serializer_helpers import ReturnDict

    if instance is None:
        return {"is_activated": False, "is_setup_done": False}
    instance = dict(instance)
    instance[field("workspaces_exist")] = True
    config = {
        "enable_signup": False,
        "is_workspace_creation_disabled": False,
        "is_google_enabled": False,
        "is_github_enabled": None,
        "is_gitlab_enabled": False,
        "is_gitea_enabled": False,
        "is_magic_login_enabled": True,
        "is_email_password_enabled": True,
        "github_app_name": "",
        "slack_client_id": None,
        "has_unsplash_configured": True,
        "has_llm_configured": False,
        "file_size_limit": 5242880.0,
        "is_smtp_configured": False,
        "admin_base_url": "http://localhost:3001",
        "space_base_url": "",
        "app_base_url": "http://localhost:3000",
        "instance_changelog_url": None,
        "is_self_managed": True,
    }
    for key in list(config):
        config[field(key)] = config.pop(key)
    return {
        field("config"): config,
        field("instance"): ReturnDict(instance, serializer=None),
    }


def _envelope(items, *, wrapped: bool):
    """`cache.set(key, {"data": response.data, "status": 200}, timeout)`."""
    from rest_framework.utils.serializer_helpers import ReturnList

    data = ReturnList(items, serializer=None) if wrapped else list(items)
    return {field("data"): data, field("status"): 200}


def history_cases(cases, annotator, plain, seen):
    group("throttle-histories")
    histories = [
        ("history-empty", []),
        ("history-one", [1789000000.123456]),
        ("history-two", [1789000000.123456, 1788999940.0]),
        (
            "history-thirty",
            [1789000000.0 - 60 * index for index in range(30)],
        ),
    ]
    for name, value in histories:
        case(name, value, cases, annotator, plain, seen)


def status_cases(cases, annotator, plain, seen):
    group("envelopes")
    for status in (200, 201, 204, 400, 403, 404, 500):
        case(
            f"envelope-status-{status}",
            {"data": {"detail": "x"}, "status": status},
            cases,
            annotator,
            plain,
            seen,
        )
    case(
        "envelope-none-data",
        {"data": None, "status": 200},
        cases,
        annotator,
        plain,
        seen,
    )
    case(
        "envelope-empty-list",
        {"data": [], "status": 200},
        cases,
        annotator,
        plain,
        seen,
    )


def real_serializer_cases(cases, annotator, plain, seen):
    """The actual `.data` objects, pickled through the real serializers."""
    group("drf-serializers")
    from plane.app.serializers import LabelSerializer, WorkspaceEstimateSerializer
    from plane.db.models import Estimate, EstimatePoint, Label, User
    from plane.license.api.serializers import (
        InstanceAdminSerializer,
        InstanceConfigurationSerializer,
        InstanceSerializer,
    )
    from plane.license.models import Instance, InstanceAdmin, InstanceConfiguration

    def make(model, **overrides):
        obj = model()
        for model_field in model._meta.concrete_fields:
            if model_field.name in overrides:
                continue
            kind = model_field.get_internal_type()
            if model_field.primary_key or kind == "UUIDField":
                setattr(obj, model_field.attname, uuid.uuid4())
            elif kind in ("CharField", "TextField", "SlugField", "EmailField"):
                setattr(obj, model_field.attname, "")
            elif kind == "BooleanField":
                setattr(obj, model_field.attname, False)
            elif kind in (
                "IntegerField",
                "BigIntegerField",
                "PositiveIntegerField",
                "SmallIntegerField",
                "FloatField",
            ):
                setattr(obj, model_field.attname, 0)
            elif kind == "DateTimeField":
                setattr(
                    obj,
                    model_field.attname,
                    datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc),
                )
            else:
                setattr(obj, model_field.attname, None)
        for name, value in overrides.items():
            setattr(obj, name, value)
        return obj

    # Deterministic primary keys: `uuid.uuid4()` above is only a placeholder for
    # fields the serializer does not emit.
    identity = iter(range(9000, 9999))

    def ident():
        return uuid.UUID(int=next(identity))

    workspace_id = ident()
    project_id = ident()

    labels = [
        make(
            Label,
            id=ident(),
            name="bug",
            color="#ff0000",
            project_id=project_id,
            workspace_id=workspace_id,
            sort_order=65535.0,
            parent_id=None,
        ),
        make(
            Label,
            id=ident(),
            name="feature",
            color="#00ff00",
            project_id=project_id,
            workspace_id=workspace_id,
            sort_order=1.0,
            parent_id=None,
        ),
    ]
    label_data = LabelSerializer(labels, many=True).data
    case("drf-labels-two-rows", _envelope(label_data, wrapped=False), cases, annotator, plain, seen)
    # The envelope helper wraps a plain list; the real object is a ReturnList.
    case(
        "drf-labels-return-list",
        {field("data"): label_data, field("status"): 200},
        cases,
        annotator,
        plain,
        seen,
    )
    case("drf-labels-bare", label_data, cases, annotator, plain, seen)

    estimate = make(
        Estimate,
        id=ident(),
        name="Story points",
        description="",
        workspace_id=workspace_id,
        project_id=project_id,
    )
    point = make(
        EstimatePoint,
        id=ident(),
        estimate_id=estimate.id,
        workspace_id=workspace_id,
        project_id=project_id,
        key=1,
        value="1",
    )
    estimate._prefetched_objects_cache = {"points": [point]}
    estimate_data = WorkspaceEstimateSerializer([estimate], many=True).data
    case(
        "drf-estimates-one-row",
        {field("data"): estimate_data, field("status"): 200},
        cases,
        annotator,
        plain,
        seen,
    )

    configuration = make(
        InstanceConfiguration,
        id=ident(),
        key="ENABLE_SIGNUP",
        value="1",
    )
    configuration_data = InstanceConfigurationSerializer([configuration], many=True).data
    case(
        "drf-configurations-one-row",
        {field("data"): configuration_data, field("status"): 200},
        cases,
        annotator,
        plain,
        seen,
    )

    instance = make(Instance, id=ident(), instance_name="Acme", current_version="1.0")
    instance_data = InstanceSerializer(instance).data
    instance_data[field("workspaces_exist")] = True
    case(
        "drf-instance-serializer",
        {field("data"): instance_data, field("status"): 200},
        cases,
        annotator,
        plain,
        seen,
    )

    admin = make(
        InstanceAdmin,
        id=ident(),
        instance=instance,
        user=make(
            User,
            id=ident(),
            email="admin@example.com",
            first_name="Ada",
            last_name="Lovelace",
            display_name="Ada Lovelace",
        ),
        role=20,
    )
    admin_data = InstanceAdminSerializer([admin], many=True).data
    case(
        "drf-instance-admins-one-row",
        {field("data"): admin_data, field("status"): 200},
        cases,
        annotator,
        plain,
        seen,
    )


def combinatorial_cases(cases, annotator, plain, seen):
    """Systematic coverage of the shapes a payload can take."""
    group("scalar-positions")
    scalars = [
        None,
        True,
        False,
        0,
        1,
        -1,
        255,
        256,
        65535,
        65536,
        2**31 - 1,
        2**31,
        -(2**31) - 1,
        2**63,
        2**100,
        0.0,
        -0.0,
        1.5,
        1e-5,
        1e16,
        "",
        "a",
        "x" * 255,
        "x" * 256,
        b"",
        b"\x00\xff",
        decimal.Decimal("1.5"),
        uuid_for(7000),
        datetime.date(2026, 11, 16),
        datetime.datetime(2026, 11, 16, 12, 0, 0),
    ]
    for index, value in enumerate(scalars):
        case(f"position-list-{index}", [value], cases, annotator, plain, seen)
        case(
            f"position-dict-{index}",
            {field("a"): value, field("b"): None},
            cases,
            annotator,
            plain,
            seen,
        )
        case(
            f"position-tuple-{index}",
            (value, "tail"),
            cases,
            annotator,
            plain,
            seen,
        )

    group("container-sizes")
    for size in range(0, 11):
        case(f"size-list-{size}", list(range(size)), cases, annotator, plain, seen)
        case(
            f"size-dict-{size}",
            {field(f"k{index}"): index for index in range(size)},
            cases,
            annotator,
            plain,
            seen,
        )
        case(f"size-tuple-{size}", tuple(range(size)), cases, annotator, plain, seen)
        case(f"size-set-{size}", set(range(size)), cases, annotator, plain, seen)
        case(
            f"size-frozenset-{size}",
            frozenset(range(size)),
            cases,
            annotator,
            plain,
            seen,
        )
        case(
            f"size-history-{size}",
            [1789000000.0 + index for index in range(size)],
            cases,
            annotator,
            plain,
            seen,
        )

    group("int-boundaries")
    for index, value in enumerate(
        [
            0,
            1,
            126,
            127,
            128,
            129,
            254,
            255,
            256,
            257,
            32767,
            32768,
            65534,
            65535,
            65536,
            65537,
            2**31 - 2,
            2**31 - 1,
            2**31,
            2**31 + 1,
            -(2**31),
            -(2**31) - 1,
            2**63 - 1,
            2**63,
            2**64 - 1,
            2**64,
            2**127,
            2**128 - 1,
            2**128,
            -(2**63),
            -(2**64),
            -(2**128),
        ]
    ):
        case(f"int-boundary-{index}", value, cases, annotator, plain, seen)

    group("uuid-bits")
    for index, value in enumerate(
        [0, 1, 2**63, 2**64 - 1, 2**127, 2**128 - 1, 2**127 + 1, 0xFF, 0x100]
    ):
        case(
            f"uuid-bits-{index}",
            [uuid.UUID(int=value)],
            cases,
            annotator,
            plain,
            seen,
        )

    group("datetime-parts")
    for index, value in enumerate(
        [
            datetime.datetime(1, 1, 1, 0, 0, 0),
            datetime.datetime(9999, 12, 31, 23, 59, 59, 999999),
            datetime.datetime(2026, 2, 28, 23, 59, 59, 1),
            datetime.datetime(2024, 2, 29, 12, 30, 45, 999999),
            datetime.datetime(2026, 6, 15, 0, 0, 0, 1000),
            datetime.datetime(2026, 6, 15, 0, 0, 0, 999000),
            datetime.datetime(
                2026, 6, 15, 0, 0, 0, 1, tzinfo=datetime.timezone.utc
            ),
        ]
    ):
        case(f"datetime-part-{index}", value, cases, annotator, plain, seen)

    group("view-row-counts")
    for rows in range(0, 13):
        case(
            f"labels-rows-{rows}",
            _labels_payload(rows, base=8000 + rows * 100),
            cases,
            annotator,
            plain,
            seen,
        )
    for rows in range(0, 6):
        case(
            f"estimates-rows-{rows}",
            _estimates_payload(rows, base=9000 + rows * 100),
            cases,
            annotator,
            plain,
            seen,
        )
        case(
            f"configurations-rows-{rows}",
            _configurations_payload(rows, base=9500 + rows * 100),
            cases,
            annotator,
            plain,
            seen,
        )
        case(
            f"admins-rows-{rows}",
            _admins_payload(rows, base=9800 + rows * 100),
            cases,
            annotator,
            plain,
            seen,
        )

    group("envelope-x-status")
    for status in (200, 201, 400, 500):
        case(
            f"labels-envelope-{status}",
            {
                field("data"): _labels_payload(1, base=11000),
                field("status"): status,
            },
            cases,
            annotator,
            plain,
            seen,
        )


def extra_cases(cases, annotator, plain, seen):
    """More scalar precision and nesting variety."""
    group("float-precision")
    floats = [
        1.0,
        2.0,
        0.5,
        0.1,
        1 / 3,
        1e-4,
        1e-5,
        1e15,
        1e16,
        1e17,
        2.2250738585072014e-308,
        5e-324,
        1.7976931348623157e308,
        -1.7976931348623157e308,
        -5e-324,
        123456789.0,
        1234567890123456.0,
        1234567890123456789.0,
        0.000001,
        0.000001234,
        1e100,
        1e-100,
        6.02214076e23,
        3.141592653589793,
        2.718281828459045,
        -2.718281828459045,
        100.0,
        65535.0,
        9007199254740992.0,
        9007199254740993.0,
        1.0000000000000002,
        -0.5,
    ]
    for index, value in enumerate(floats):
        case(f"float-{index}", value, cases, annotator, plain, seen)

    group("string-precision")
    strings = [
        "ascii",
        "with space",
        "tab\there",
        "newline\nhere",
        "carriage\rhere",
        "quote\"here",
        "backslash\\here",
        "nul\x00here",
        "\x01\x02\x03",
        "caf\u00e9",
        "\u00e9\u00e8\u00ea",
        "\u4f60\u597d\u4e16\u754c",
        "\u3042\u3044\u3046",
        "\uac00\ud55c\uae00",
        "\ud83d\ude00\ud83d\ude01",
        "\u2028\u2029",
        "\u200bzero-width",
        "\ufeffbom",
        "e\u0301combining",
        "\u0645\u0631\u062d\u0628\u0627",
        "\u05e9\u05dc\u05d5\u05dd",
        "a" * 1000,
        "\x7f",
        "\u00ff",
    ]
    for index, value in enumerate(strings):
        case(f"string-{index}", value, cases, annotator, plain, seen)

    group("nesting")
    nestings = [
        [[[[[[[[[[1]]]]]]]]]],
        [{"a": (1, 2)}, {"b": frozenset({3})}],
        {"a": [{}, [], (), set(), frozenset()]},
        ([1, [2, [3, [4]]]], {"x": {"y": {"z": None}}}),
        {"data": [{"id": uuid_for(12000), "n": None}], "status": 200},
        {"a": True, "b": False, "c": None, "d": 0, "e": 0.0},
        [{}, {}, {}],
        [[], [], []],
        ((1,), (2, (3,))),
        {(1, 2), (3, 4)},
        [frozenset({1, 2}), frozenset({3})],
    ]
    for index, value in enumerate(nestings):
        case(f"nesting-{index}", value, cases, annotator, plain, seen)

    group("estimate-points")
    for points in range(0, 6):
        case(
            f"estimates-points-{points}",
            _estimates_with_points(points, base=15000 + points * 100),
            cases,
            annotator,
            plain,
            seen,
        )

    group("mixed-envelopes")
    mixed = [
        {"data": {"detail": "Not found."}, "status": 404},
        {"data": {"error": "x", "error_code": 5900}, "status": 400},
        {"data": [1, 2.5, "three", None, True], "status": 200},
        {"data": {"nested": {"deep": [1, {"x": None}]}}, "status": 200},
        {"data": {"uuid_value": uuid_for(13000)}, "status": 200},
        {"data": {"decimal_value": decimal.Decimal("9.99")}, "status": 200},
        {"data": {"date_value": datetime.date(2026, 1, 1)}, "status": 200},
        {
            "data": {"dt_value": datetime.datetime(2026, 1, 1, tzinfo=datetime.timezone.utc)},
            "status": 200,
        },
    ]
    for index, value in enumerate(mixed):
        case(f"mixed-envelope-{index}", value, cases, annotator, plain, seen)

    group("labels-variants")
    for index, (sort_order, active) in enumerate(
        [(0.0, True), (-1.0, False), (65535.0, True), (12345.5, False)]
    ):
        items = [
            keyed(
                parent=None if active else uuid_for(14000 + index),
                name=f"label-{name}",
                color="#000000",
                id=uuid_for(14100 + index),
                project_id=uuid_for(14200),
                workspace_id=uuid_for(14201),
                sort_order=sort_order,
            )
            for name in ("a", "b", "c")
        ]
        case(
            f"labels-variant-{index}",
            _envelope(items, wrapped=True),
            cases,
            annotator,
            plain,
            seen,
        )

    group("assorted")
    assorted = [
        {"status": 200, "data": []},
        {"status": 200, "data": {}},
        {"status": 200, "data": [None]},
        [[True, False], [False, True]],
        [0.0, -0.0, 1.0],
        {"a": b"", "b": b"\x00"},
        (None,),
        {0},
        frozenset({0}),
        {field("only"): 1},
        [field("x"), field("x"), field("y")],
    ]
    for index, value in enumerate(assorted):
        case(f"assorted-{index}", value, cases, annotator, plain, seen)


def _estimates_with_points(points: int, *, base: int):
    """`WorkspaceEstimatesEndpoint` with a varying number of nested points."""
    point_items = [
        keyed(
            id=uuid_for(base + 500 + point),
            created_at="2026-01-01T00:00:00Z",
            updated_at="2026-01-01T00:00:00Z",
            deleted_at=None,
            key=point,
            description="",
            value=str(point + 1),
            created_by=None,
            updated_by=None,
            project_id=uuid_for(base + 900),
            workspace_id=uuid_for(base + 901),
            estimate_id=uuid_for(base),
        )
        for point in range(points)
    ]
    item = keyed(
        id=uuid_for(base),
        points=point_items,
        created_at="2026-01-01T00:00:00Z",
        updated_at="2026-01-01T00:00:00Z",
        deleted_at=None,
        name="Estimate",
        description="",
        type="categories",
        last_used=False,
        created_by=None,
        updated_by=None,
        project_id=uuid_for(base + 900),
        workspace_id=uuid_for(base + 901),
    )
    return _envelope([item], wrapped=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, help="output JSON path")
    arguments = parser.parse_args()

    import django

    django.setup()

    cases: list[dict] = []
    annotator = Annotator()
    plain = Plain()
    seen: set[str] = set()

    scalar_cases(cases, annotator, plain, seen)
    container_cases(cases, annotator, plain, seen)
    memo_cases(cases, annotator, plain, seen)
    realistic_cases(cases, annotator, plain, seen)
    history_cases(cases, annotator, plain, seen)
    status_cases(cases, annotator, plain, seen)
    real_serializer_cases(cases, annotator, plain, seen)
    combinatorial_cases(cases, annotator, plain, seen)
    extra_cases(cases, annotator, plain, seen)

    document = {
        "python": sys.version.split()[0],
        "pickle_default_protocol": PROTOCOL,
        "cases": cases,
    }
    with open(arguments.out, "w", encoding="utf-8") as handle:
        json.dump(document, handle, ensure_ascii=False, indent=1, sort_keys=False)
        handle.write("\n")
    print(f"wrote {len(cases)} cases to {arguments.out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
