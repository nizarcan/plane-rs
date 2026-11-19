#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Generate the golden JSON corpus for the `plane-serde` crate.

Every record is one JSON object per line:

    {"cat": "<category>", "dialects": ["drf"|"django"|"kombu", ...],
     "in": "<base64>", "out": "<base64>", "error": true?}

`in` is the canonical input for the category (see the Rust golden test) and
`out` is the exact byte sequence the real Python code path produced. The
expected bytes come from the pinned virtualenv:

* DRF:      ``rest_framework.renderers.JSONRenderer().render(value)``
* Django:   ``json.dumps(value, cls=DjangoJSONEncoder)`` (the exact call
            ``JsonResponse`` makes)
* Kombu:    ``kombu.utils.json.dumps(value)``

Float/int/string/nested-structure values are also routed through the three
encoders. Python values (datetimes, timedeltas, decimals, UUIDs, kombu
envelopes) use the specific field/encoder call that produced them in Plane.

The transcode corpus sends documents through the dev-stack Postgres and uses
``SELECT $1::json::text`` / ``SELECT $1::jsonb::text`` as the input text, with
``json.loads(pg_text)`` re-encoded for each dialect as the expected output.

Run from the repository root:

    apps/api/.venv/bin/python apps/server/tools/golden/serde_corpus.py

The generated files live in ``apps/server/crates/plane-serde/tests/golden/``.
"""

from __future__ import annotations

import base64
import datetime
import decimal
import json
import math
import os
import pathlib
import random
import struct
import sys
import uuid

import psycopg

HERE = pathlib.Path(__file__).resolve()
SERVER_DIR = HERE.parents[2]

# The Django oracle (apps/api at the UPSTREAM.toml pin); see tools/upstream/oracle.py.
sys.path.insert(0, str(SERVER_DIR / "tools" / "upstream"))
from oracle import api_dir as _oracle_api_dir  # noqa: E402

API_DIR = _oracle_api_dir()
OUT_DIR = SERVER_DIR / "crates" / "plane-serde" / "tests" / "golden"

sys.path.insert(0, str(API_DIR))
os.environ["DJANGO_SETTINGS_MODULE"] = "plane.settings.production"
os.environ["SECRET_KEY"] = "serde-corpus-secret-key-not-for-deployment"
os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")

import django  # noqa: E402

django.setup()

import kombu.utils.json as kombu_json  # noqa: E402
from django.core.serializers.json import DjangoJSONEncoder  # noqa: E402
from django.http import JsonResponse  # noqa: E402
from rest_framework import serializers  # noqa: E402
from rest_framework.renderers import JSONRenderer  # noqa: E402

PG_DSN = "postgresql://plane:plane@127.0.0.1:55432/plane"
SEED = 20261115

# ---------------------------------------------------------------------------
# Encoders
# ---------------------------------------------------------------------------


def b64(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def b64_text(text: str) -> str:
    return b64(text.encode("utf-8"))


def expected(dialect: str, value) -> bytes:
    if dialect == "drf":
        return JSONRenderer().render(value)
    if dialect == "django":
        return json.dumps(value, cls=DjangoJSONEncoder).encode("utf-8")
    if dialect == "kombu":
        return kombu_json.dumps(value).encode("utf-8")
    raise ValueError(dialect)


class Collector:
    """Collect records and write them out at the end."""

    def __init__(self, name: str) -> None:
        self.name = name
        self.records: list[dict] = []
        self.counts: dict[str, int] = {}

    def add(
        self,
        category: str,
        dialects: list[str],
        input_bytes: bytes,
        *,
        out: bytes | None = None,
        error: bool = False,
        params: dict | None = None,
    ) -> None:
        record = {
            "cat": category,
            "dialects": dialects,
            "in": b64(input_bytes),
        }
        if error:
            record["error"] = True
        else:
            assert out is not None
            record["out"] = b64(out)
        if params:
            record.update(params)
        self.records.append(record)
        self.counts[category] = self.counts.get(category, 0) + 1

    def add_value(self, category: str, value, input_bytes: bytes, **kwargs) -> None:
        """One record per dialect, using the three real encoders."""
        for dialect in ("drf", "django", "kombu"):
            try:
                out = expected(dialect, value)
            except Exception as exc:  # noqa: BLE001
                print(f"  {category}: {dialect} raised {type(exc).__name__}: {exc}", file=sys.stderr)
                self.add(category, [dialect], input_bytes, error=True, **kwargs)
                continue
            self.add(category, [dialect], input_bytes, out=out, **kwargs)

    def write(self) -> None:
        OUT_DIR.mkdir(parents=True, exist_ok=True)
        path = OUT_DIR / f"{self.name}.jsonl"
        with path.open("w", encoding="utf-8", newline="\n") as handle:
            for record in self.records:
                handle.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")))
                handle.write("\n")
        size = path.stat().st_size
        total = sum(self.counts.values())
        print(f"{path.relative_to(SERVER_DIR)}: {total} records, {size / 1_048_576:.2f} MiB")
        for category in sorted(self.counts):
            print(f"    {category:24s} {self.counts[category]:6d}")


# ---------------------------------------------------------------------------
# Value corpus
# ---------------------------------------------------------------------------


def tagged(value):
    """Describe a Python value for the Rust golden test.

    Strings, ints and floats are carried as text so arbitrary Python ints,
    exact float bits and non-UTF-8-free strings all survive; objects are
    ordered lists of (key, value) pairs so Python's dict order and non-string
    keys are preserved.
    """
    if value is None:
        return {"t": "null"}
    if isinstance(value, bool):
        return {"t": "bool", "v": value}
    if isinstance(value, int):
        return {"t": "int", "v": str(value)}
    if isinstance(value, float):
        return {"t": "float", "v": repr(value)}
    if isinstance(value, str):
        return {"t": "str", "v": value}
    if isinstance(value, (list, tuple)):
        return {"t": "arr", "v": [tagged(item) for item in value]}
    if isinstance(value, dict):
        return {
            "t": "obj",
            "v": [[tagged(key), tagged(item)] for key, item in value.items()],
        }
    raise TypeError(f"cannot describe {value!r}")


def describe_bytes(value) -> bytes:
    return json.dumps(tagged(value), ensure_ascii=False, separators=(",", ":")).encode(
        "utf-8"
    )


def float_bits(value: float, width: int) -> bytes:
    return struct.pack("<d" if width == 64 else "<f", value)


def build_float_samples(count: int) -> list[float]:
    rng = random.Random(SEED)
    seen: set[bytes] = set()
    values: list[float] = []

    def add(value: float) -> bool:
        if not math.isfinite(value):
            return False
        bits = struct.pack("<d", value)
        if bits in seen:
            return False
        seen.add(bits)
        values.append(value)
        return True

    # Random bit patterns.
    while len(values) < count // 3:
        add(struct.unpack("<d", rng.randbytes(8))[0])

    # "Human" decimals.
    while len(values) < count // 2:
        add(round(rng.uniform(-1e9, 1e9), rng.randint(0, 8)))

    # Powers of ten 1e-30 .. 1e30.
    for exponent in range(-30, 31):
        add(float(f"1e{exponent}"))

    # Boundaries around the two presentation thresholds.
    for base in (1e16, 1e-4, 1e-5):
        value = base
        for _ in range(200):
            value = math.nextafter(value, math.inf)
            add(value)
        value = base
        for _ in range(200):
            value = math.nextafter(value, 0.0)
            add(value)

    # Integral floats up to 2**63.
    while len(values) < count - count // 8:
        add(float(rng.randrange(-(2**63), 2**63)))

    # Edge cases.
    for value in (
        0.0,
        -0.0,
        1.0,
        -1.0,
        5e-324,
        -5e-324,
        sys.float_info.max,
        -sys.float_info.max,
        sys.float_info.min,
        sys.float_info.min / 2,
        0.1 + 0.2,
        1.0 / 3.0,
        1e15,
        1e16,
        1e17,
        1234567890123456.0,
        12345678901234567.0,
        9999999999999998.0,
        999999999999999.9,
        1e21,
        1e-7,
        1e-8,
        1e300,
        1e-300,
    ):
        add(value)

    # Fill up.
    while len(values) < count:
        add(struct.unpack("<d", rng.randbytes(8))[0])

    return values[:count]


def build_float32_samples(count: int) -> list[float]:
    rng = random.Random(SEED + 1)
    seen: set[int] = set()
    values: list[float] = []
    fixed = [
        0.0,
        -0.0,
        1.0,
        -1.0,
        0.1,
        float.fromhex("0x1.fffffep+127"),
        float.fromhex("0x1p-149"),
        3.4028234663852886e38,
        1.401298464324817e-45,
    ]
    for value in fixed:
        bits = struct.unpack("<I", struct.pack("<f", value))[0]
        if bits not in seen:
            seen.add(bits)
            values.append(struct.unpack("<f", struct.pack("<I", bits))[0])
    while len(values) < count:
        bits = rng.getrandbits(32)
        if bits in seen:
            continue
        value = struct.unpack("<f", struct.pack("<I", bits))[0]
        if not math.isfinite(value):
            continue
        seen.add(bits)
        values.append(value)
    return values[:count]


def build_strings() -> list[str]:
    strings = ["", "a", "plain ascii"]
    strings += ["a" + chr(code) + "b" for code in range(0x00, 0x80)]
    strings += [
        "Zo\u00eb \u2014 \u65e5\u672c\u8a9e \U0001f680",
        "\u0080",
        "\u00ff",
        "\u2028",
        "\u2029",
        "line\u2028sep\u2029end",
        "\ufeff",
        "\uffff",
        "\U0001f600",
        "\U00010ffff",
        "\u0000",
        "\u0000\u0001\u001f",
        "tab\tnl\nquote\"backslash\\slash/",
        "mixed: abc \u00e9 \u4e2d \U0001f642 xyz",
        "x" * 10_000,
        ("\u00e9\u4e2d\U0001f680" * 250),
        "".join(chr(code) for code in range(0x20)),
        "\u007f\u0080\u009f",
        "Emoji family \U0001f468\u200d\U0001f469\u200d\U0001f467\u200d\U0001f466",
    ]
    return strings


def structure_values() -> list:
    return [
        {},
        [],
        {"a": 1, "b": [1, 2, {"c": None}], "d": True, "e": False},
        {"z": 1, "a": 2, "m": 3},
        {"empty_obj": {}, "empty_arr": [], "nested": [[[]]]},
        {
            "float": 1e-5,
            "int": -7,
            "big": 2**63 - 1,
            "neg": -(2**63),
            "zero": 0,
            "minus_zero": -0.0,
        },
        {"s": "Zo\u00eb \u2014 \u65e5\u672c\u8a9e \U0001f680", "ctrl": "a\tb\nc"},
        {"arr": list(range(50))},
        {"deep": {"a": {"b": {"c": {"d": [1, {"e": "f"}]}}}}},
        [None, True, False, 0, -1, 1.5, "", [], {}],
        {"unicode_keys": {"\u00e9": 1, "\u4e2d": 2, "\U0001f680": 3}},
        {"strings": ["", "a", "\u00e9", "\U0001f680", "\\", '"', "\n"]},
    ]


def build_map_keys() -> list[dict]:
    return [
        {1: 2},
        {-2: "x"},
        {1.5: 2},
        {0.1: 2},
        {True: 2},
        {False: 2},
        {None: 2},
        {1e16: 2},
        {1: "a", "1": "b"},
        {3: {"nested": [None, True]}},
        {0: "zero", -0.0: "minus zero"},
    ]


def build_python_values(collector: Collector) -> None:
    utc = datetime.timezone.utc

    aware = [
        datetime.datetime(2026, 11, 15, 12, 34, 56, 0, tzinfo=utc),
        datetime.datetime(2026, 11, 15, 12, 34, 56, 1, tzinfo=utc),
        datetime.datetime(2026, 11, 15, 12, 34, 56, 999, tzinfo=utc),
        datetime.datetime(2026, 11, 15, 12, 34, 56, 999000, tzinfo=utc),
        datetime.datetime(2026, 11, 15, 12, 34, 56, 999999, tzinfo=utc),
        datetime.datetime(2026, 11, 15, 12, 34, 56, 123456, tzinfo=utc),
        datetime.datetime(2026, 11, 15, 12, 34, 56, 120000, tzinfo=utc),
        datetime.datetime(1, 1, 1, 0, 0, 0, 0, tzinfo=utc),
        datetime.datetime(1, 1, 1, 0, 0, 0, 1, tzinfo=utc),
        datetime.datetime(9999, 12, 31, 23, 59, 59, 999999, tzinfo=utc),
        datetime.datetime(
            2026, 11, 15, 12, 34, 56, 1,
            tzinfo=datetime.timezone(datetime.timedelta(hours=5, minutes=30)),
        ),
        datetime.datetime(
            2026, 11, 15, 12, 34, 56, 0,
            tzinfo=datetime.timezone(-datetime.timedelta(hours=3)),
        ),
    ]
    field = serializers.DateTimeField()
    for value in aware:
        # In a request the field converts to the current timezone (UTC by
        # default) and then formats identically to the encoder.
        if value.tzinfo is utc:
            assert field.to_representation(value) == expected("drf", value).decode()[1:-1]
        collector.add(
            "drf_datetime", ["drf"], value.isoformat().encode(), out=expected("drf", value)
        )
        collector.add(
            "django_datetime",
            ["django"],
            value.isoformat().encode(),
            out=expected("django", value),
        )
        collector.add(
            "kombu_datetime",
            ["kombu"],
            value.isoformat().encode(),
            out=expected("kombu", value),
        )

    naive = [
        datetime.datetime(2026, 11, 15, 12, 34, 56, 0),
        datetime.datetime(2026, 11, 15, 12, 34, 56, 1),
        datetime.datetime(2026, 11, 15, 12, 34, 56, 123456),
        datetime.datetime(2026, 11, 15, 12, 34, 56, 999000),
        datetime.datetime(1, 1, 1, 0, 0, 0, 1),
        datetime.datetime(9999, 12, 31, 23, 59, 59, 999999),
    ]
    for value in naive:
        collector.add(
            "drf_naive_datetime",
            ["drf"],
            value.isoformat().encode(),
            out=expected("drf", value),
        )
        collector.add(
            "django_naive_datetime",
            ["django"],
            value.isoformat().encode(),
            out=expected("django", value),
        )
        collector.add(
            "kombu_naive_datetime",
            ["kombu"],
            value.isoformat().encode(),
            out=expected("kombu", value),
        )

    dates = [
        datetime.date(2026, 11, 15),
        datetime.date(1, 1, 1),
        datetime.date(9999, 12, 31),
        datetime.date(1970, 1, 1),
    ]
    for value in dates:
        collector.add(
            "py_date", ["drf", "django", "kombu"], value.isoformat().encode(),
            out=expected("drf", value),
        )

    times = [
        datetime.time(0, 0, 0, 0),
        datetime.time(12, 34, 56, 1),
        datetime.time(12, 34, 56, 789012),
        datetime.time(12, 34, 56, 999000),
        datetime.time(23, 59, 59, 999999),
    ]
    for value in times:
        collector.add(
            "py_time", ["drf"], value.isoformat().encode(), out=expected("drf", value)
        )
        collector.add(
            "django_time",
            ["django"],
            value.isoformat().encode(),
            out=expected("django", value),
        )
        collector.add(
            "kombu_time",
            ["kombu"],
            value.isoformat().encode(),
            out=expected("kombu", value),
        )

    timedeltas = [
        datetime.timedelta(0),
        datetime.timedelta(microseconds=1),
        datetime.timedelta(microseconds=-1),
        datetime.timedelta(hours=2, minutes=3),
        datetime.timedelta(days=1, seconds=3661, microseconds=500000),
        datetime.timedelta(seconds=-1, microseconds=-500000),
        datetime.timedelta(days=-2, microseconds=1),
        datetime.timedelta(days=999999999, hours=23, minutes=59, seconds=59, microseconds=999999),
        datetime.timedelta(days=-999999999),
        datetime.timedelta(microseconds=863999999135999999 // 1000),
    ]
    for value in timedeltas:
        micros = value // datetime.timedelta(microseconds=1)
        collector.add(
            "py_timedelta",
            ["drf"],
            str(micros).encode(),
            out=expected("drf", value),
        )
        collector.add(
            "django_timedelta",
            ["django"],
            str(micros).encode(),
            out=expected("django", value),
        )

    decimals = [
        (12, 4, "1.1000"),
        (12, 4, "1.23455"),
        (12, 4, "1.23445"),
        (12, 2, "1.005"),
        (12, 2, "2.675"),
        (12, 2, "-0.005"),
        (3, 2, "0.005"),
        (4, 3, "0.0005"),
        (4, 3, "0.0015"),
        (4, 3, "0.0025"),
        (10, 0, "123.456"),
        (10, 0, "123.5"),
        (10, 0, "124.5"),
        (6, 3, "9.9995"),
        (12, 4, "99999999.99995"),
        (12, 4, "1E+10"),
        (5, 2, "12345.678"),
        (2, 2, "1.00"),
        (12, 4, "0"),
        (12, 4, "-0.0000"),
        (12, 4, "1E+3"),
        (12, 4, "123456789012345678901234567890"),
    ]
    for max_digits, decimal_places, text in decimals:
        field = serializers.DecimalField(
            max_digits=max_digits, decimal_places=decimal_places
        )
        params = {"max_digits": max_digits, "decimal_places": decimal_places}
        try:
            out = expected("drf", field.to_representation(decimal.Decimal(text)))
        except Exception as exc:  # noqa: BLE001
            collector.add(
                "drf_decimal", ["drf"], text.encode(), error=True, params=params
            )
            continue
        collector.add("drf_decimal", ["drf"], text.encode(), out=out, params=params)

    encoder_decimals = [
        "1.10",
        "1E+3",
        "0.000001",
        "-0.00",
        "12345.6789",
        "1.0000000000000000000005",
        "1E-7",
        "1E+20",
        "NaN",
        "Infinity",
        "-Infinity",
        "1e999",
        "-1e999",
        "1e-999999999",
        "123456789012345678901234567890",
    ]
    for text in encoder_decimals:
        value = decimal.Decimal(text)
        for category, dialect in (
            ("encoder_decimal_drf", "drf"),
            ("encoder_decimal_django", "django"),
            ("kombu_decimal", "kombu"),
        ):
            try:
                out = expected(dialect, value)
            except Exception:  # noqa: BLE001
                collector.add(category, [dialect], text.encode(), error=True)
                continue
            collector.add(category, [dialect], text.encode(), out=out)

    uuids = [
        "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
        "00000000-0000-0000-0000-000000000000",
        "ffffffff-ffff-ffff-ffff-ffffffffffff",
        "6a1b6f0e-0f9a-4b62-9a39-1b2c3d4e5f60",
    ]
    for text in uuids:
        value = uuid.UUID(text)
        collector.add(
            "uuid",
            ["drf", "django", "kombu"],
            text.encode(),
            out=expected("drf", value),
        )
        collector.add(
            "kombu_uuid",
            ["kombu"],
            text.encode(),
            out=expected("kombu", value),
        )

    byte_values = [
        b"",
        b"hello",
        b"Zo\xc3\xab",
        b"\xff\xfe",
        bytes(range(256)),
        b"\x00\x01\x02",
    ]
    for value in byte_values:
        collector.add(
            "kombu_bytes", ["kombu"], value, out=expected("kombu", value)
        )

    for value in dates[:3]:
        collector.add(
            "kombu_date", ["kombu"], value.isoformat().encode(),
            out=expected("kombu", value),
        )
    for value in times[:3]:
        collector.add(
            "kombu_time", ["kombu"], value.isoformat().encode(),
            out=expected("kombu", value),
        )


def build_scalars(collector: Collector) -> None:
    floats = build_float_samples(20_000)
    for value in floats:
        collector.add(
            "float",
            ["drf", "django", "kombu"],
            float_bits(value, 64),
            out=expected("drf", value),
        )

    float32 = build_float32_samples(2_000)
    for value in float32:
        widened = float(value)
        collector.add(
            "float32",
            ["drf", "django", "kombu"],
            float_bits(value, 32),
            out=expected("drf", widened),
        )

    for value in (float("nan"), float("inf"), float("-inf")):
        for dialect in ("drf", "django", "kombu"):
            try:
                out = expected(dialect, value)
            except Exception:  # noqa: BLE001
                collector.add(
                    "f64_nonfinite", [dialect], float_bits(value, 64), error=True
                )
                continue
            collector.add(
                "f64_nonfinite", [dialect], float_bits(value, 64), out=out
            )

    ints = {
        0,
        -1,
        1,
        2,
        -2,
        42,
        2**31 - 1,
        -(2**31),
        2**32 - 1,
        2**63 - 1,
        -(2**63),
        2**64 - 1,
        10**15,
        -(10**15),
    }
    rng = random.Random(SEED + 2)
    while len(ints) < 400:
        ints.add(rng.randrange(-(2**63), 2**63))
    while len(ints) < 600:
        ints.add(rng.randrange(0, 2**64))
    for value in sorted(ints):
        collector.add(
            "int", ["drf", "django", "kombu"], str(value).encode(),
            out=expected("drf", value),
        )

    collector.add("none", ["drf"], b"", out=b"")
    collector.add("none", ["django"], b"", out=b"null")
    collector.add("none", ["kombu"], b"", out=b"null")

    strings = build_strings()
    for value in strings:
        collector.add_value("string", value, value.encode("utf-8"))


def build_structures(collector: Collector) -> None:
    for value in structure_values():
        encoded = describe_bytes(value)
        for dialect in ("drf", "django", "kombu"):
            if dialect == "django" and isinstance(value, dict):
                # JsonResponse is json.dumps(obj, cls=DjangoJSONEncoder) with
                # the default keyword arguments.
                assert JsonResponse(value).content == expected("django", value)
            collector.add(
                "structure", [dialect], encoded, out=expected(dialect, value)
            )
    for value in build_map_keys():
        encoded = describe_bytes(value)
        for dialect in ("drf", "django", "kombu"):
            try:
                out = expected(dialect, value)
            except Exception:  # noqa: BLE001
                collector.add("mapkeys", [dialect], encoded, error=True)
                continue
            collector.add("mapkeys", [dialect], encoded, out=out)


# ---------------------------------------------------------------------------
# Transcode corpus: documents through Postgres
# ---------------------------------------------------------------------------

RAW_DOCUMENTS = [
    "-0",
    "1.50",
    "1E2",
    "1e400",
    "-1e400",
    "1e-400",
    "0.0",
    "-0.0",
    "1e16",
    "1e-5",
    "123456789012345678901234567890",
    "-123456789012345678901234567890",
    '"hello"',
    '"Zo\\u00eb \\u2014 \\u65e5\\u672c\\u8a9e \\ud83d\\ude80"',
    '"\\u0000\\u0001\\u001f\\u007f\\u0080\\u00ff\\u2028\\u2029"',
    '"quote\\" backslash\\\\ slash\\/ tab\\t nl\\n cr\\r bs\\b ff\\f"',
    "  [1, 2.0, 1e2, -0, 1.50]  ",
    '{"a": 1, "b": 2, "a": 3}',
    '{"a": {"x": 1}, "a": {"y": 2}}',
    '{"z": 1, "a": 2, "m": 3}',
    '{"a": 1, "a": 2, "b": 3, "a": 4, "b": 5}',
    '{"\\u00e9": 1, "\u00e9": 2}',
    '{"zz": 1, "a": 2, "mmm": 3, "bb": 4}',
    '{"empty": "", "none": null, "t": true, "f": false}',
    '{"nested": [[], {}, [1, [2, [3, [4]]]], {"x": {"y": {"z": null}}}]}',
    '{"big": 12345678901234567890123456789012345678901234567890}',
    '{"small": 1e-300, "tiny": 5e-324, "max": 1.7976931348623157e308}',
    '{"mixed": [0, -0, 0.0, -0.0, 1E2, 1.5e2, 150.0, 0.00015e6]}',
    '"' + "x" * 1000 + '"',
    "[" + ",".join(str(index) for index in range(100)) + "]",
    '{"repeat": ' + "[" + ",".join(["1.5"] * 50) + "]}",
    '{"deep": ' + "[" * 40 + "1" + "]" * 40 + "}",
    '{"wide": {' + ",".join(f'"k{index}": {index}' for index in range(50)) + "}}",
    '{"unicode_key": {"\\ud83d\\ude80": "rocket", "\\u4e2d": "middle"}}',
    '{"escaped_key": {"a\\tb": 1, "a\\nb": 2, "a\\"b": 3}}',
    # CPython's json.loads enforces sys.int_max_str_digits (4300) on integer
    # lexemes; both Postgres json and jsonb accept this document.
    '{"n": ' + "9" * 5000 + "}",
]


def build_transcode_documents() -> list[str]:
    rng = random.Random(SEED + 3)
    documents: list[str] = []

    # Float corpus subset, first as Python floats and then as raw lexemes.
    for value in build_float_samples(20_000)[:250]:
        documents.append(json.dumps({"v": value}))
    for lexeme in (
        "1e-5",
        "1e16",
        "0.000123456789",
        "1.2345678901234567e-300",
        "1.2345678901234567e300",
        "-1.5e-9",
        "9007199254740993.0",
    ):
        documents.append(f'{{"v": {lexeme}}}')

    # Strings from the main corpus (PG rejects \u0000 in jsonb; those are
    # skipped below).
    for value in build_strings():
        documents.append(json.dumps({"s": value}))

    # Nested structures.
    for value in structure_values():
        documents.append(json.dumps(value, ensure_ascii=rng.choice([True, False])))

    documents.extend(RAW_DOCUMENTS)

    # Random small documents with a mix of types and key orders.
    keys = ["z", "a", "mm", "b", "long_key_name", "\u00e9", "\U0001f680"]
    for _ in range(120):
        document = {}
        for key in rng.sample(keys, rng.randint(1, len(keys))):
            kind = rng.randrange(6)
            if kind == 0:
                document[key] = rng.randrange(-(2**63), 2**64)
            elif kind == 1:
                document[key] = rng.uniform(-1e6, 1e6)
            elif kind == 2:
                document[key] = rng.choice([True, False, None])
            elif kind == 3:
                document[key] = rng.choice(
                    ["", "text", "Zo\u00eb", "\u2028", "quote\"", "back\\slash", "\n"]
                )
            elif kind == 4:
                document[key] = [rng.randrange(10), rng.random(), None]
            else:
                document[key] = {"inner": rng.randrange(-100, 100)}
        documents.append(json.dumps(document, ensure_ascii=rng.choice([True, False])))

    return documents


def transcode_via_postgres(documents: list[str]) -> list[tuple[str, str]]:
    """Return (pg_text, source_type) pairs for every document Postgres accepted."""
    results: list[tuple[str, str]] = []
    rejected = 0
    with psycopg.connect(PG_DSN) as connection:
        with connection.cursor() as cursor:
            for index, text in enumerate(documents):
                for pg_type in ("json", "jsonb"):
                    try:
                        row = cursor.execute(
                            f"SELECT %s::{pg_type}::text", (text,)
                        ).fetchone()
                    except Exception:  # noqa: BLE001
                        connection.rollback()
                        rejected += 1
                        continue
                    results.append((row[0], pg_type))
                if index and index % 100 == 0:
                    print(f"  postgres: {index}/{len(documents)} documents", file=sys.stderr)
    print(
        f"  postgres: {len(documents)} documents, {rejected} rejections, "
        f"{len(results)} text outputs",
        file=sys.stderr,
    )
    return results


def build_transcode(collector: Collector) -> None:
    documents = build_transcode_documents()
    outputs = transcode_via_postgres(documents)
    seen: set[str] = set()
    for pg_text, _source in outputs:
        if pg_text in seen:
            continue
        seen.add(pg_text)
        try:
            value = json.loads(pg_text)
        except Exception:  # noqa: BLE001
            for dialect in ("drf", "django", "kombu"):
                collector.add("transcode", [dialect], pg_text.encode(), error=True)
            continue
        for dialect in ("drf", "django", "kombu"):
            try:
                out = expected(dialect, value)
            except Exception:  # noqa: BLE001
                collector.add("transcode", [dialect], pg_text.encode(), error=True)
                continue
            collector.add("transcode", [dialect], pg_text.encode(), out=out)
    print(f"  transcode: {len(seen)} unique Postgres texts", file=sys.stderr)


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    print(f"Django {django.get_version()}, Python {sys.version.split()[0]}")
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    for stale in OUT_DIR.glob("*.jsonl"):
        stale.unlink()

    floats = Collector("floats")
    build_scalars(floats)

    structures = Collector("structures")
    build_structures(structures)

    python_values = Collector("python_values")
    build_python_values(python_values)

    transcode = Collector("transcode")
    build_transcode(transcode)

    for collector in (floats, structures, python_values, transcode):
        collector.write()

    total = sum(
        sum(collector.counts.values())
        for collector in (floats, structures, python_values, transcode)
    )
    size = sum(path.stat().st_size for path in OUT_DIR.glob("*.jsonl"))
    print(f"total: {total} records, {size / 1_048_576:.2f} MiB in {OUT_DIR}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
