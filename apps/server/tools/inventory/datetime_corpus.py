# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record the datetime/date parsing corpus used by `write::fields`.

Every case is pushed through the real chain a Plane serializer uses:

    rest_framework.fields.DateTimeField().to_internal_value(value)
    rest_framework.fields.DateField().to_internal_value(value)

which is DRF's `to_internal_value` -> `django.utils.dateparse.parse_datetime`
(CPython `datetime.fromisoformat`, then Django's `datetime_re` fallback) ->
`DateTimeField.enforce_timezone` with the active timezone. No database is
touched.

Regenerate from the repository root:

    apps/api/.venv/bin/python apps/server/tools/inventory/datetime_corpus.py

The output is `apps/server/crates/plane-http-app/tests/golden/datetime-parsing.json`,
which `crates/plane-http-app/src/write/fields.rs` embeds with `include_str!`.
"""

import datetime
import json
import pathlib
import random
import sys
import unicodedata

import django
from django.conf import settings

settings.configure(
    USE_TZ=True,
    TIME_ZONE="UTC",
    INSTALLED_APPS=[],
    DATABASES={},
    SECRET_KEY="datetime-corpus",
)
django.setup()

from django.utils import timezone  # noqa: E402
from django.utils.dateparse import parse_date, parse_datetime  # noqa: E402
from rest_framework.fields import DateField, DateTimeField  # noqa: E402
from rest_framework.serializers import ValidationError  # noqa: E402

OUT = (
    pathlib.Path(__file__).resolve().parents[2]
    / "crates/plane-http-app/tests/golden/datetime-parsing.json"
)

ZONES = ["UTC", "Asia/Kolkata"]
# A few cases exercise the DST transitions in a zone that has them, because
# `make_aware`/`valid_datetime` decide what an ambiguous or imaginary wall time
# means.
EXTRA_ZONES = {"2026-03-08 02:30", "2026-11-01 01:30", "2026-03-29 02:30", "2026-10-25 02:30"}


def _detail(error):
    """The message and code of a field's `ValidationError`."""
    detail = error.detail
    if isinstance(detail, (list, tuple)):
        detail = detail[0]
    return str(detail), getattr(detail, "code", None)


def parse_result(parser, value):
    """Django's `parse_datetime`/`parse_date` alone: the value it returns, or
    "no value". A `None` (the regex did not match) and a `ValueError` (the
    regex matched but the components were out of range) both end up as the
    field's own error, so they are recorded the same way."""
    try:
        parsed = parser(value)
    except (ValueError, TypeError):
        return {"ok": False}
    if parsed is None:
        return {"ok": False}
    return {"ok": True, "value": parsed.isoformat()}


def datetime_result(field, value):
    try:
        parsed = field.to_internal_value(value)
    except ValidationError as error:
        message, code = _detail(error)
        return {"ok": False, "message": message, "code": code}
    except OverflowError:
        # `DateTimeField.enforce_timezone` -> `valid_datetime` -> `astimezone`
        # raises an uncaught OverflowError for a year-1 wall time east of UTC,
        # which Django answers with a 500. The Rust port cannot raise from
        # `fields::datetime`; the corpus marks these so the test can accept any
        # error instead of a specific detail.
        return {"ok": False, "server_error": "OverflowError"}
    utc = parsed.astimezone(datetime.timezone.utc)
    return {"ok": True, "value": isoformat_utc(utc)}


def isoformat_utc(value):
    text = value.strftime("%Y-%m-%dT%H:%M:%S")
    if value.microsecond:
        text += ".%06d" % value.microsecond
    return text + "Z"


def date_result(field, value):
    try:
        parsed = field.to_internal_value(value)
    except ValidationError as error:
        message, code = _detail(error)
        return {"ok": False, "message": message, "code": code}
    return {"ok": True, "value": parsed.isoformat()}


def build_inputs():
    """Deterministically build the corpus, family by family."""
    families = {}

    families["hand"] = [
        "",
        " ",
        "  ",
        "\n",
        "\t",
        "iso-8601",
        "ISO-8601",
        "Iso-8601",
        "iso_8601",
        "iso-8601 ",
        " iso-8601",
        "iso--8601",
        "not-a-date",
        "2026",
        "2026-",
        "2026-11",
        "2026-W1",
        "2026-W00-1",
        "2026-W01-0",
        "2026-W01-8",
        "2026-W53-1",
        "2026-W54-1",
        "9999-W52-5",
        "9999-W52-7",
        "0001-W01-1",
        "2026-11-20",
        " 2026-11-20",
        "2026-11-20 ",
        "2026-11-2",
        "2026-1-2",
        "2026-01-02",
        "2026-11-20\n",
        "2026-11-20\r\n",
        "2026-11-20\t",
        "2026-11-20T12:00:00Z",
        "2026-11-20T12:00:00",
        "2026-11-20 12:00:00",
        "2026-11-20t12:00:00",
        "2026-11-20X12:00:00",
        "2026-11-20-12:00:00",
        "2026-11-20_12:00:00",
        "2026-11-20\u00e912:00:00",
        "2026-11-20T",
        "2026-11-20TZ",
        "2026-11-20T12",
        "2026-11-20T1",
        "2026-11-20T12:3",
        "2026-11-20T12:30:5",
        "2026-11-20T12:34:56.123456",
        "2026-11-20T12:34:56.123456789",
        "2026-11-20T12:34:56.1234567890123",
        "2026-11-20T12:00:00.",
        "2026-11-20T12:00:00,",
        "2026-11-20T12:00:",
        "2026-11-20T12:",
        "2026-11-20T:12",
        "2026-11-20T12::00",
        "2026-11-20T24:00",
        "2026-11-20T24:00:00",
        "2026-11-20T23:59:59",
        "2026-11-20T12:00:60",
        "2026-11-20T12:60:00",
        "2026-11-20T25:00:00",
        "2026-11-20T12:00:00.Z",
        "2026-11-20T12:00:00.1Z",
        "2026-11-20T12:00:00,5Z",
        "2026-11-20T12:00:00+03",
        "2026-11-20T12:00:00+0300",
        "2026-11-20T12:00:00+03:00",
        "2026-11-20T12:00:00+03:00:30",
        "2026-11-20T12:00:00+03:00:30.5",
        "2026-11-20T12:00:00+00:00:00.5",
        "2026-11-20T12:00:00-00:00:00.5",
        "2026-11-20T12:00:00+23:59",
        "2026-11-20T12:00:00-23:59",
        "2026-11-20T12:00:00+24:00",
        "2026-11-20T12:00:00+99:99",
        "2026-11-20T12:00:00+03:99",
        "2026-11-20T12:00:00-03:99",
        "2026-11-20T12:00Z",
        "2026-11-20T12:00z",
        "2026-11-20T12 Z",
        "2026-11-20T12:00 Z",
        "2026-11-20T12:00  Z",
        "2026-11-20T12:00\tZ",
        "2026-11-20T12:00\nZ",
        "2026-11-20T12:00\u00a0Z",
        "2026-11-20T12:00\u3000Z",
        "2026-11-20T12:00+03:00 ",
        "2026-11-20T12:00Z ",
        "2026-11-20T12:00Z\n",
        "2026-11-20T12-00",
        "2026-11-20T12+00",
        "2026-11-20T12:00-00",
        "2026-11-20T12:00+00",
        "2026-11-20T123456",
        "2026-11-20T123456Z",
        "2026-11-20T123456789",
        "2026-11-20T1234567890123456",
        "2026-11-20T12:3456",
        "2026-11-20T12.5",
        "2026-11-20T12,5",
        "20261120",
        "2026112",
        "20261120ab",
        "20261120T120000",
        "20261120 120000",
        "20261120T120000Z",
        "20261120T1200",
        "2026-1120",
        "202611-20",
        "2026W47",
        "2026W475",
        "2026W47512",
        "2026W4751",
        "2026W47x12:00",
        "2026-W47",
        "2026-W47-5",
        "2026-W47-15",
        "2026-W47x12:00",
        "2026-W47T12:00",
        "2026-W47-5T12:00",
        "2026-W47-1X9",
        "2026-W011",
        "2026W01-1",
        "2026-11-20 12:00",
        "2026-11-20 12:00:00.123456",
        "2026-11-20 12:00:00.1234567",
        "2026-11-20 12:00:00.1234567890123",
        "2026-11-20 12:00:00,5",
        "2026-11-20 12:00+03",
        "2026-11-20 12:00+0300",
        "2026-11-20 12:00+03:00",
        "2026-11-20 12:00 +03:00",
        "2026-11-20 12:00  +03:00",
        "2026-11-20 12:00+03:99",
        "2026-11-20 12:00+99:99",
        "2026-11-20 12:00+24:00",
        "2026-11-20 12:00Z",
        "2026-11-20 12:00z",
        "2026-11-20 12:00\n",
        "2026-11-20 12:00Z\n",
        "2026-11-20 12:00\x1cZ",
        "2026-11-20 12:00\x0bZ",
        "2026-11-20\t12:00",
        "2026-11-20\x0b",
        "2026-11-20\u00a0",
        "2026-1-2 1:2",
        "2026-1-2 1:2:3.4",
        "2026-11-20 12",
        "2026-11-20 12:00:60",
        "2026-11-20 24:00",
        "2026-11-20 25:00",
        "2026-13-01 12:00",
        "2026-00-01 12:00",
        "2026-11-32 12:00",
        "2026-11-31 12:00",
        "2026-02-29 12:00",
        "2024-02-29 12:00",
        "0000-01-01 12:00",
        "9999-12-31 23:59",
        "0001-01-01T00:00:00+05:00",
        "0001-01-01T00:00:00-05:00",
        "9999-12-31T23:59:59-05:00",
        "9999-12-31T23:59:59+05:00",
        "0001-01-01 00:00",
        "9999-12-31 23:59",
        "\u0662\u0660\u0662\u0666-\u0661\u0661-\u0662\u0660",
        "\u0662\u0660\u0662\u0666-\u0661\u0661-\u0662\u0660 \u0661\u0662:\u0660\u0660",
        "\uff12\uff10\uff12\uff16-\uff11\uff11-\uff12\uff10 \uff11\uff12:\uff10\uff10",
        "\u0968\u0966\u0968\u096c-\u0967\u0967-\u0968\u0966",
        "-2026-11-20",
        "+2026-11-20",
        "2026-11-20T-12:00",
        "2026-11-20T+12:00",
        "2026-11-20T12:00:00+",
        "2026-11-20T12:00:00-",
        "2026-11-20T12:00:00Z+",
        "2026-11-20T12:00:00ZZ",
        "2026-11-20T12:00:00Za",
        "2026-11-20T12:00:00Z\x00",
        "2026-11-20T12:00:00Z\x00junk",
        "2026-11-20T12:00:00\x00junk",
        "2026-11-20T12:00:00+03:00Z",
        "2026-11-20T12:00:00+03:00:30Z",
    ]

    dates = [
        "2026-11-20",
        "2026-11-2",
        "2026-1-2",
        "2026-1-20",
        "2026-12-31",
        "2026-13-01",
        "2026-00-10",
        "2026-01-00",
        "2026-01-32",
        "2026-02-28",
        "2026-02-29",
        "2024-02-29",
        "2023-02-29",
        "2026-04-31",
        "0001-01-01",
        "9999-12-31",
        "0000-01-01",
        "20261120",
        "2026W47",
        "2026W475",
        "2026W535",
        "2026-W47",
        "2026-W47-5",
        "2026-W47-05",
        "2026-W53-1",
        "2026-W00-1",
    ]
    times = [
        "0:00",
        "00:00",
        "1:2",
        "01:02",
        "12:00",
        "12:0",
        "12:000",
        "1200",
        "1234",
        "123456",
        "1234567",
        "12:34:56",
        "12:34:5",
        "12:34:56.1",
        "12:34:56.123456",
        "12:34:56.1234567",
        "12:34:56,5",
        "12",
        "1",
        "123",
        "24:00",
        "23:59:59",
        "12:60",
        "12:00:60",
        "99:99",
        "12:00:00.1234567890123",
    ]
    offsets = [
        "",
        "Z",
        "z",
        "+03",
        "-03",
        "+0",
        "+0300",
        "-0300",
        "+030000",
        "+03:00",
        "-03:00",
        "+03:0",
        "+03:",
        "+3",
        "+030",
        "+03:00:30",
        "+03:00:30.5",
        "-03:00:30.5",
        "+00:00:00.5",
        "+23:59",
        "-23:59",
        "+23:59:59.999999",
        "+24:00",
        "-24:00",
        "+00:00",
        "-00:00",
        "+03:99",
        "+99:99",
        "+24:01",
    ]
    separators = ["T", " ", "t", "T", " ", "X", "-", "_", "\t", "  "]

    families["date_forms"] = []
    for date in dates:
        families["date_forms"].append(date)
        for separator in separators:
            families["date_forms"].append(date + separator + "12:00")
    for date in dates:
        families["date_forms"].append(date + "T12:00:00.5Z")
        families["date_forms"].append(" " + date)
        families["date_forms"].append(date + " ")
        families["date_forms"].append(date + "\n")

    families["times"] = []
    for date in ("2026-11-20", "20261120", "2026-11-2", "2026-W47-5", "2026-W47"):
        for separator in ("T", " ", "X", ""):
            for time in times:
                families["times"].append(date + separator + time)

    families["offsets"] = []
    for date in ("2026-11-20T12:00", "2026-11-20 12:00", "20261120T120000", "2026-11-20T12"):
        for offset in offsets:
            families["offsets"].append(date + offset)
        for offset in offsets:
            families["offsets"].append(date + " " + offset)

    families["fractions"] = []
    for digits in range(0, 10):
        fraction = "".join(str(digit % 10) for digit in range(1, digits + 1))
        for date in ("2026-11-20T12:00:00", "2026-11-20 12:00:00", "20261120T120000"):
            families["fractions"].append(date + "." + fraction)
            families["fractions"].append(date + "," + fraction)
            families["fractions"].append(date + "." + fraction + "Z")
            families["fractions"].append(date + "." + fraction + "+03:00")
        families["fractions"].append("2026-11-20T12:00." + fraction)
        families["fractions"].append("2026-11-20T12." + fraction)

    families["whitespace"] = []
    spaces = [" ", "\t", "\n", "\r", "\x0b", "\x0c", "\x1c", "\x1d", "\x1e", "\x1f",
              "\x85", "\xa0", "\u1680", "\u2000", "\u2003", "\u200a", "\u2028",
              "\u2029", "\u202f", "\u205f", "\u3000"]
    for space in spaces:
        families["whitespace"].extend(
            [
                space + "2026-11-20",
                "2026-11-20" + space,
                "2026-11-20" + space + "12:00",
                "2026-11-20 12:00" + space,
                "2026-11-20 12:00" + space + "Z",
                "2026-11-20 12:00" + space + space + "Z",
                "2026-11-20 12:00" + space + "+03:00",
                "2026-11-20" + space + space + "12:00",
                "2026-11-20T12:00:00" + space + "Z",
                "2026-11-20T12:00:00.5" + space + "Z",
            ]
        )

    families["unicode"] = []
    digits = {
        "arabic": "\u0660\u0661\u0662\u0663\u0664\u0665\u0666\u0667\u0668\u0669",
        "devanagari": "\u0966\u0967\u0968\u0969\u096a\u096b\u096c\u096d\u096e\u096f",
        "fullwidth": "\uff10\uff11\uff12\uff13\uff14\uff15\uff16\uff17\uff18\uff19",
        "bengali": "\u09e6\u09e7\u09e8\u09e9\u09ea\u09eb\u09ec\u09ed\u09ee\u09ef",
    }
    for name, table in digits.items():
        def number(value, table=table):
            return "".join(table[int(character)] for character in str(value))

        families["unicode"].extend(
            [
                number(2026) + "-" + number(11) + "-" + number(20),
                number(2026) + "-" + number(1) + "-" + number(2),
                number(2026) + "-" + number(11) + "-" + number(20)
                + " " + number(12) + ":" + number(0),
                number(2026) + "-" + number(11) + "-" + number(20)
                + "T" + number(12) + ":" + number(34) + ":" + number(56),
                number(20261120),
                number(2026) + "W" + number(47),
                number(2026) + "W" + number(47) + number(5),
                number(2026) + "-W" + number(47),
                number(2026) + "-W" + number(47) + "-" + number(5),
                number(2026) + "-" + number(11) + "-" + number(20)
                + "T" + number(12) + ":" + number(0) + ":" + number(0)
                + "+" + number(3) + ":" + number(0),
                "2026-" + number(11) + "-20",
                number(2026) + "-11-20",
                "2026-11-20 " + number(12) + ":00",
                "2026-11-20T" + number(12) + ":00",
                "2026-11-20T12:00:" + number(56),
                "2026-11-20T12:00:00." + number(123456),
                "2026-11-20T12:00:00+" + number(3) + ":" + number(0) + ":" + number(30),
            ]
        )

    families["garbage"] = []
    for base in ("2026-11-20", "2026-11-20T12:00:00", "20261120", "2026-W47-5"):
        for extra in ("x", "?", "\x00", "\x7f", "\u00e9", "\u20ac", "0x", "--", "..",
                      "++", "ZZ", "T", ":", ",", "."):
            families["garbage"].append(base + extra)
            families["garbage"].append(extra + base)
    families["garbage"].extend(
        ["x" * 30, "0" * 30, "-" * 30, ":" * 30, "T" * 30, "2026-11-20T" + "1" * 30,
         "2026-11-20T12:00:00." + "1" * 30, "2026-11-20T12:00:00+" + "1" * 30]
    )

    families["dst"] = sorted(EXTRA_ZONES)

    # Deterministic, family-balanced sampling: every family contributes up to
    # `cap` inputs taken at a fixed stride, so a shrunken family is visible in
    # the fixture rather than silently dropped.
    sampled = []
    for name, block in families.items():
        cap = 300 if name == "hand" else 200
        if len(block) > cap:
            step = len(block) / cap
            block = [block[int(index * step)] for index in range(cap)]
        sampled.extend(block)

    random.Random(20261117).shuffle(sampled)
    seen = set()
    unique = []
    for value in sampled:
        marker = json.dumps(value)
        if marker in seen:
            continue
        seen.add(marker)
        unique.append(value)

    # Non-string JSON values: DRF's field receives them as-is.
    extras = [None, True, False, 0, 1, -1, 1.5, [], [1], {}, {"a": 1}]
    return unique, extras


def main():
    strings, extras = build_inputs()
    datetime_field = DateTimeField()
    date_field = DateField()

    cases = []
    for value in strings:
        zones = dict.fromkeys(ZONES)
        if value in EXTRA_ZONES:
            zones["America/New_York"] = None
        datetimes = {}
        for zone in zones:
            with timezone.override(zone):
                datetimes[zone] = datetime_result(datetime_field, value)
        cases.append(
            {
                "input": value,
                "datetime": datetimes,
                "date": date_result(date_field, value),
                "parse": {
                    "datetime": parse_result(parse_datetime, value),
                    "date": parse_result(parse_date, value),
                },
            }
        )
    for value in extras:
        # The serializer's `field_value` handles `null` before the parser; the
        # corpus only pins what the field itself does with a non-string.
        cases.append(
            {
                "input": value,
                "datetime": {"UTC": datetime_result(datetime_field, value)},
                "date": date_result(date_field, value),
                "parse": {
                    "datetime": parse_result(parse_datetime, value),
                    "date": parse_result(parse_date, value),
                },
            }
        )

    document = {
        "generated_by": "apps/server/tools/inventory/datetime_corpus.py",
        "python": sys.version.split()[0],
        "django": django.get_version(),
        "drf": __import__("rest_framework").VERSION,
        "unicode": unicodedata.unidata_version,
        "zones": ZONES + ["America/New_York"],
        "cases": cases,
    }
    header = {
        key: value for key, value in document.items() if key != "cases"
    }
    # One case per line: the file is large, and a per-case line keeps a
    # regenerated diff readable.
    body = ",\n".join(
        json.dumps(case, ensure_ascii=False, separators=(",", ":"))
        for case in document["cases"]
    )
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(
        json.dumps(header, ensure_ascii=False, separators=(",", ":"))[:-1]
        + ',"cases":[\n'
        + body
        + "\n]}\n",
        encoding="utf-8",
    )
    print("wrote %d cases to %s" % (len(cases), OUT))


if __name__ == "__main__":
    main()
