# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record the string-parsing corpus used by `write::fields::python_float`.

Every string is pushed through the two functions the port has to match:

    float(value)                                    # CPython's PyFloat_FromString
    rest_framework.fields.FloatField().to_internal_value(value)

`FloatField` adds the `len(str) > 1000` guard, calls `float()`, and rejects a
non-finite result (`rest_framework/fields.py:965-980`). No database is touched.

Regenerate from the repository root:

    apps/api/.venv/bin/python apps/server/tools/inventory/float_corpus.py

The output is `apps/server/crates/plane-http-app/tests/golden/float-parsing.json`,
which `crates/plane-http-app/src/write/fields.rs` embeds with `include_str!`.
"""

import itertools
import json
import pathlib
import random
import struct
import sys

import django
from django.conf import settings

settings.configure(
    USE_TZ=True,
    TIME_ZONE="UTC",
    INSTALLED_APPS=[],
    DATABASES={},
    SECRET_KEY="float-corpus",
)
django.setup()

from rest_framework.fields import FloatField  # noqa: E402
from rest_framework.serializers import ValidationError  # noqa: E402

OUT = (
    pathlib.Path(__file__).resolve().parents[2]
    / "crates/plane-http-app/tests/golden/float-parsing.json"
)


def bits(value):
    """The exact IEEE-754 double, so `-0.0`, `inf` and NaN signs are compared."""
    return struct.pack(">d", value).hex()


def python_result(text):
    try:
        value = float(text)
    except (TypeError, ValueError):
        return {"ok": False}
    return {"ok": True, "bits": bits(value)}


def field_result(field, text):
    try:
        value = field.to_internal_value(text)
    except ValidationError as error:
        detail = error.detail
        if isinstance(detail, (list, tuple)):
            detail = detail[0]
        return {
            "ok": False,
            "message": str(detail),
            "code": getattr(detail, "code", None),
        }
    return {"ok": True, "bits": bits(value)}


# Characters whose classification decides whether `float()` strips them. Python's
# float parser uses `Py_UNICODE_ISSPACE` (which excludes `\x1c`-`\x1f`), not
# `str.isspace()`.
WHITESPACE = [
    " ",
    "\t",
    "\n",
    "\r",
    "\x0b",
    "\x0c",
    "\x1c",
    "\x1d",
    "\x1e",
    "\x1f",
    "\x85",
    "\xa0",
    "\u1680",
    "\u2000",
    "\u2003",
    "\u200a",
    "\u2028",
    "\u2029",
    "\u202f",
    "\u205f",
    "\u3000",
    "\u200b",
]

# Unicode decimal digits (`Nd`) from four scripts, plus one non-decimal digit.
DIGITS = ["1", "\u0661", "\u0967", "\uff11", "\u00b9"]


def build_inputs():
    families = {}

    families["hand"] = [
        "",
        " ",
        "\t",
        "\n",
        "\r\n",
        "0",
        "1",
        "-1",
        "+1",
        "1.0",
        "-1.0",
        "+1.0",
        "1.",
        ".5",
        "+.5",
        "-.5",
        "-.",
        ".",
        "..",
        "1..2",
        "1.5.5",
        "0.0",
        "-0.0",
        "+0.0",
        "1_",
        "_1",
        "__1",
        "1__0",
        "1_0",
        "1_0_0",
        "1_0_",
        "1_.0",
        "1._0",
        "1.0_",
        "1.0_0",
        "_1_0",
        "1_0_0_0",
        "1e",
        "1e+",
        "1e-",
        "1e0",
        "1E0",
        "1e+2",
        "1e-2",
        "1e+_2",
        "1e_2",
        "1_e2",
        "1e2_",
        "1e2_0",
        "1_0e1_0",
        "1_0e-1_0",
        "1_0e+1_0",
        "1_0.0_1e+2_0",
        "._5",
        ".5_",
        "0_1",
        "00_1",
        "000_1",
        "0x10",
        "0x1p3",
        "0X1P3",
        "0b101",
        "0o7",
        "1d",
        "1f",
        "1j",
        "1.0j",
        "1_000j",
        "inf",
        "INF",
        "Inf",
        "+inf",
        "-inf",
        "Infinity",
        "INFINITY",
        "infinity",
        "iNfInItY",
        "infinit",
        "infin",
        "in",
        "infi",
        "+Infinity",
        "-INFINITY",
        "nan",
        "NaN",
        "NAN",
        "+nan",
        "-nan",
        "nan(1)",
        "NaN(1)",
        "nAn",
        "nn",
        "na",
        " 1 ",
        "\t1\n",
        " 1.5e2 ",
        "1 2",
        "1\t.\t5",
        "1\u00a02",
        "1\u20032",
        "\u0661\u0662",
        "\u0661_\u0662",
        "1\u0662",
        "\u06612",
        "\uff11\uff12",
        "\uff11_\uff12",
        "1_\u0662",
        "\u0662_1",
        "\u0661e\u0662",
        "\u0661.\u0665e\u0662",
        "\u0661_",
        "_\u0662",
        "\u0661__\u0662",
        "\u0967\u0968",
        "\u0967_\u0968",
        "\u00b9",
        "1\u00b9",
        "1e400",
        "-1e400",
        "1e-400",
        "-1e-400",
        "1e308",
        "1e309",
        "1e-323",
        "2.2250738585072014e-308",
        "1.7976931348623157e308",
        "1.7976931348623159e308",
        "1234567890123456789012345678901234567890",
        "0.00000000000000000000000000000000000000001",
        "1_000.000_1",
        "1_2.3_4e5_6",
        "1_2.3_e4",
        "1_2e+3_4",
        "+1_0",
        "-1_0",
        "1_0e10",
        "1_0e1_0e",
        "1_0e1_",
        "-1_",
        "+_1",
        "+",
        "-",
        "_",
        "+_",
        "-_",
        "++1",
        "--1",
        "+-1",
        "-+1",
        "++inf",
        "--inf",
        "+-inf",
        "-+inf",
        "+-nan",
        "-+nan",
        "++1.5e2",
        "--1.5e2",
        "-+\u0661",
        "+-\u0661",
        " \u0661 ",
        "\u0661 ",
        " \u0661",
        "-0",
        "+0",
        "0000",
        "0000.0000",
        "1.e5",
        "1.E+5",
        ".5e2",
        "-.5e-2",
    ]

    families["signs"] = []
    cores = [
        "0",
        "1",
        "10",
        "1.5",
        ".5",
        "1.",
        "0.0",
        "1_0",
        "1_000.5",
        "1e5",
        "1e-5",
        "1e+5",
        "1E5",
        "\u0661\u0662",
        "\u0661.\u0665",
        "\u0661e\u0662",
        "0_1",
        "00_1",
        "1_0_0",
        "1_0.5_0",
        "1_0e1_0",
        "1_0e-1_0",
        "1.5e1_0",
    ]
    for sign, core in itertools.product(["", "+", "-", " ", "\t", "\u00a0"], cores):
        families["signs"].append(sign + core)

    families["suffixes"] = []
    for core, suffix in itertools.product(
        ["1", "1.5", "1e5", "1_0", "\u0661\u0662", "inf"],
        [" ", "\n", "\x1c", "\xa0", "\u2003", "_", "e5", "e_5", ".5", "x", "\x00"],
    ):
        families["suffixes"].append(core + suffix)

    families["whitespace"] = []
    for space in WHITESPACE:
        families["whitespace"].extend(
            [
                space + "1",
                "1" + space,
                space + "-1.5e2" + space,
                space + "inf" + space,
                "1" + space + "2",
                space + "nan" + space,
                space + "1_0" + space,
            ]
        )

    families["underscores"] = []
    atoms = ["1", "2", "\u0661", "\uff11", "x", "e", ".", "_", ""]
    for left, right in itertools.product(atoms, atoms):
        families["underscores"].append(left + "_" + right)
    for left, middle, right in itertools.product(["1", "\u0661", "x", "."], repeat=3):
        families["underscores"].append(left + "_" + middle + "_" + right)

    families["lengths"] = []
    for length in (2, 3, 10, 998, 999, 1000, 1001, 1002, 1100):
        families["lengths"].append("1" * length)
        families["lengths"].append("9" * length + ".5")
    families["lengths"].extend(
        [
            " " + "1" * 999 + " ",
            " " + "1" * 1000 + " ",
            "\u0661" * 1000,
            "\u0661" * 1001,
            "1" * 999 + "_0",
            "1" * 1001 + "_0",
            "0." + "0" * 998 + "1",
            "1e" + "9" * 999,
        ]
    )

    families["exponents"] = []
    for mantissa, exponent in itertools.product(
        ["1", "1.5", "1_0", "\u0661\u0662"],
        ["e0", "e+0", "e-0", "e1_0", "e1_", "e_1", "e", "E+308", "e-308", "e309", "e-324"],
    ):
        families["exponents"].append(mantissa + exponent)

    for block in families.values():
        assert all(isinstance(value, str) for value in block)

    values = []
    for name, block in families.items():
        cap = 180 if name != "hand" else 400
        if len(block) > cap:
            step = len(block) / cap
            block = [block[int(index * step)] for index in range(cap)]
        values.extend(block)

    random.Random(20261117).shuffle(values)
    seen = set()
    unique = []
    for value in values:
        if value in seen:
            continue
        seen.add(value)
        unique.append(value)
    return unique


def main():
    strings = build_inputs()
    field = FloatField()

    cases = []
    for value in strings:
        cases.append(
            {
                "input": value,
                "python": python_result(value),
                "field": field_result(field, value),
            }
        )

    document = {
        "generated_by": "apps/server/tools/inventory/float_corpus.py",
        "python": sys.version.split()[0],
        "django": django.get_version(),
        "drf": __import__("rest_framework").VERSION,
        "cases": cases,
    }
    header = {key: value for key, value in document.items() if key != "cases"}
    body = ",\n".join(
        json.dumps(case, ensure_ascii=False, separators=(",", ":")) for case in cases
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
