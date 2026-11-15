#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Generate the golden corpus for `plane_serde::pyjson`.

Every record is one JSON object per line:

    {"cat": "<category>", "in": "<base64 body>", "enc": "utf-8",
     "ok": true,
     "value": <tagged value>,            # absent for `deep` records
     "deep": {"kinds": "aao...", "leaf": <tagged>},
     "drf": "<base64>",                  # JSONRenderer().render(value)
     "drf_error": {"type": ..., "msg": ...},   # when rendering raises
     "django": "<base64>"}               # json.dumps(value, cls=DjangoJSONEncoder)

    {"cat": "...", "in": ..., "enc": ...,
     "ok": false,
     "error": {"type": "ParseError", "detail": "JSON parse error - ...",
               "kind": "json"|"decode"|"charset"},
     "exc": "JSONDecodeError"}

The expected bytes come from the pinned virtualenv, through the real DRF
parser (`rest_framework.parsers.JSONParser`), exactly as a request would:

    JSONParser().parse(io.BytesIO(body), media_type="application/json",
                       parser_context={"encoding": enc})

A tagged value describes the Python object `json.loads` returned so that
`int` vs `float`, negative zero and lone surrogates are checked, not just the
rendered bytes:

    {"t": "null"} | {"t": "bool", "v": true}
    {"t": "int", "v": "<decimal>"}
    {"t": "float", "v": "<8 bytes, little endian, hex>"}
    {"t": "str", "v": "<base64 of utf-8 with surrogatepass>"}
    {"t": "arr", "v": [...]}
    {"t": "obj", "v": [[<key>, <value>], ...]}

`kinds` in a `deep` record is one character per nesting level, `a` for a list
and `o` for an object; the tagged leaf is the innermost value. Those records
are too deep to describe as a tagged value (and too deep for a recursive
reader), so the Rust test walks them iteratively.

Run from the repository root:

    apps/api/.venv/bin/python apps/server/tools/golden/pyjson_corpus.py

The generated file lives in
``apps/server/crates/plane-serde/tests/pyjson/corpus.jsonl``.
"""

from __future__ import annotations

import base64
import codecs
import io
import json
import os
import pathlib
import random
import struct
import sys

HERE = pathlib.Path(__file__).resolve()
SERVER_DIR = HERE.parents[2]
API_DIR = HERE.parents[3] / "api"
OUT_DIR = SERVER_DIR / "crates" / "plane-serde" / "tests" / "pyjson"
OUT_FILE = OUT_DIR / "corpus.jsonl"

sys.path.insert(0, str(API_DIR))

# The parser only needs DRF's settings, not a database. `settings.configure`
# is enough: `api_settings.STRICT_JSON` falls back to DRF's default (True),
# which is what Plane runs with (it does not override REST_FRAMEWORK's
# STRICT_JSON), and DEFAULT_CHARSET is Django's "utf-8".
from django.conf import settings  # noqa: E402

settings.configure(
    DEBUG=False,
    USE_TZ=True,
    DEFAULT_CHARSET="utf-8",
    REST_FRAMEWORK={},
    SECRET_KEY="pyjson-corpus-secret-key-not-for-deployment",
)

import django  # noqa: E402

django.setup()

from django.core.serializers.json import DjangoJSONEncoder  # noqa: E402
from rest_framework.exceptions import ParseError  # noqa: E402
from rest_framework.parsers import JSONParser  # noqa: E402
from rest_framework.renderers import JSONRenderer  # noqa: E402

# Deterministic; override with PYJSON_SEED to fuzz a different corpus locally,
# and PYJSON_FUZZ_SCALE to generate more fuzz cases (for a local soak run).
SEED = int(os.environ.get("PYJSON_SEED", "20261116"))
FUZZ_SCALE = int(os.environ.get("PYJSON_FUZZ_SCALE", "1"))

# ---------------------------------------------------------------------------
# Records
# ---------------------------------------------------------------------------


def b64(data: bytes) -> str:
    return base64.b64encode(data).decode("ascii")


def tagged(value):
    if value is None:
        return {"t": "null"}
    if isinstance(value, bool):
        return {"t": "bool", "v": value}
    if isinstance(value, int):
        return {"t": "int", "v": str(value)}
    if isinstance(value, float):
        return {"t": "float", "v": struct.pack("<d", value).hex()}
    if isinstance(value, str):
        return {"t": "str", "v": b64(value.encode("utf-8", "surrogatepass"))}
    if isinstance(value, list):
        return {"t": "arr", "v": [tagged(item) for item in value]}
    if isinstance(value, dict):
        return {"t": "obj", "v": [[tagged(key), tagged(item)] for key, item in value.items()]}
    raise TypeError(f"cannot describe {value!r}")


def shape(value):
    """Describe a parseable but deep value: level kinds and the leaf."""
    kinds = []
    current = value
    while True:
        if isinstance(current, list) and len(current) == 1:
            kinds.append("a")
            current = current[0]
        elif isinstance(current, dict) and len(current) == 1:
            kinds.append("o")
            current = next(iter(current.values()))
        else:
            return {"kinds": "".join(kinds), "leaf": tagged(current)}


DEEP_THRESHOLD = 500


def nesting_depth(value) -> int:
    """Depth along the single-child path, which is how the deep cases are
    built. Deeper values are described by `shape` instead of a tagged value,
    because neither this program nor the Rust test can recurse that far."""
    depth = 0
    current = value
    while isinstance(current, (list, dict)) and len(current) == 1:
        depth += 1
        current = next(iter(current.values())) if isinstance(current, dict) else current[0]
    return depth


class Collector:
    def __init__(self) -> None:
        self.records: list[dict] = []
        self.counts: dict[str, int] = {}

    def add(self, case, record: dict) -> None:
        category, body, encoding = case
        record = {
            "cat": category,
            "in": b64(body),
            "enc": encoding,
            **record,
        }
        self.records.append(record)
        self.counts[category] = self.counts.get(category, 0) + 1

    def write(self) -> None:
        OUT_DIR.mkdir(parents=True, exist_ok=True)
        with OUT_FILE.open("w", encoding="utf-8", newline="\n") as handle:
            for record in self.records:
                handle.write(json.dumps(record, ensure_ascii=False, separators=(",", ":")))
                handle.write("\n")
        size = OUT_FILE.stat().st_size
        total = len(self.records)
        print(f"{OUT_FILE.relative_to(SERVER_DIR)}: {total} records, {size / 1_048_576:.2f} MiB")
        for category in sorted(self.counts):
            print(f"    {category:28s} {self.counts[category]:6d}")


PARSER = JSONParser()


def run(case, collector: Collector) -> None:
    category, body, encoding = case

    # Classify the failure the way DRF's two layers do: get_encoding() first,
    # then the codec reader, then json.load.
    try:
        "".encode(encoding)
    except (LookupError, UnicodeError):
        kind = "charset"
    else:
        try:
            codecs.getreader(encoding)(io.BytesIO(body)).read()
        except UnicodeError:
            kind = "decode"
        else:
            kind = "json"

    try:
        value = PARSER.parse(
            io.BytesIO(body),
            media_type="application/json",
            parser_context={"encoding": encoding},
        )
    except ParseError as exc:
        assert kind in ("charset", "decode", "json"), (case, kind)
        collector.add(
            case,
            {
                "ok": False,
                "error": {"type": "ParseError", "detail": str(exc.detail), "kind": kind},
                "exc": getattr(exc.__cause__, "__class__", type(exc)).__name__,
            },
        )
        return
    except RecursionError as exc:
        collector.add(
            case,
            {
                "ok": False,
                "error": {"type": "RecursionError", "msg": str(exc), "kind": "recursion"},
                "exc": "RecursionError",
            },
        )
        return

    record: dict = {"ok": True}
    if category == "deep" or nesting_depth(value) > DEEP_THRESHOLD:
        record["deep"] = shape(value)
    else:
        record["value"] = tagged(value)

    try:
        record["drf"] = b64(JSONRenderer().render(value))
    except Exception as exc:  # noqa: BLE001
        record["drf_error"] = {"type": type(exc).__name__, "msg": str(exc)}
    record["django"] = b64(json.dumps(value, cls=DjangoJSONEncoder).encode("utf-8"))
    collector.add(case, record)


# ---------------------------------------------------------------------------
# Case builders
# ---------------------------------------------------------------------------


def cases():
    """Yield (category, body, encoding) tuples."""

    utf8 = "utf-8"

    def add(category, body, encoding=utf8):
        if isinstance(body, str):
            body = body.encode("utf-8")
        return (category, body, encoding)

    # --- empty and whitespace-only -----------------------------------------
    for body in [b"", b" ", b"\n", b"\r", b"\t", b"  \t\r\n ", b"\r\n\r\n", b"\n \n"]:
        yield add("empty", body)
    for body in [b"\x0b", b"\x0c", b"\xc2\xa0", b"\xe2\x80\xa8", b"\x00"]:
        yield add("empty", body)

    # --- valid scalars ------------------------------------------------------
    for body in [
        "null", "true", "false",
        "0", "-0", "1", "-1", "42", "-42", "9007199254740993",
        "9223372036854775807", "-9223372036854775808", "9223372036854775808",
        "-9223372036854775809", "18446744073709551615", "18446744073709551616",
        "1234567890123456789012345678901234567890",
        "-1234567890123456789012345678901234567890",
        "0.0", "-0.0", "1.0", "-1.0", "0.1", "0.30000000000000004", "1.5",
        "-1.5e-9", "1e-5", "1e-4", "1e15", "1e16", "1e17", "1e21", "1e-7",
        "5e-324", "1.7976931348623157e308", "1234567890123456.0",
        "12345678901234567.0", "9999999999999998.0", "999999999999999.9",
        "1E2", "1e+2", "1e-2", "1e002", "-0e-0", "1e400", "-1e400", "1e-400",
        "2.2250738585072014e-308", "4.9e-324",
        '""', '"a"', '"hello"', '"Zoë — 日本語 🚀"', '"\\u0000"', '"\\u2028"',
        '"a\\tb"', '"\\ud83d\\ude80"',
    ]:
        yield add("valid", body)

    # --- valid strings: every escape, every code point shape ----------------
    for escape in ['\\"', "\\\\", "\\/", "\\b", "\\f", "\\n", "\\r", "\\t",
                   "\\u0000", "\\u001f", "\\u0020", "\\u007f", "\\u0080",
                   "\\u00e9", "\\u2028", "\\u2029", "\\uffff", "\\ufffd",
                   "\\ud83d\\ude80", "\\uD834\\uDD1E", "\\u0001"]:
        yield add("strings", '"a' + escape + 'b"')
    for code in range(0x20):
        yield add("strings", '"' + chr(code) + '"')  # raw control -> error
        yield add("strings", '"\\u%04x"' % code)
    yield add("strings", '"' + "".join(chr(c) for c in range(0x20, 0x7F)) + '"')
    yield add("strings", '"' + "\u007f\u0080\u009f" + '"')
    yield add("strings", '"' + "é中🚀" * 200 + '"')
    yield add("strings", '"' + "x" * 10_000 + '"')
    yield add("strings", '"' + "a" * 65_530 + "é" + '"')
    yield add("strings", '"\\u0000\\u0001\\u001f\\u007f\\u0080\\u00ff\\u2028\\u2029"')
    yield add("strings", '"\\uD800\\uDC00"')
    yield add("strings", '"\\uDBFF\\uDFFF"')
    yield add("strings", '"\\uD7FF\\uDC00"')
    yield add("strings", '"\\uD800\\uD800"')
    yield add("strings", '"\\uDC00\\uDC00"')
    yield add("strings", '"\\uDC00\\uD800"')
    yield add("strings", '"\\uD800"')
    yield add("strings", '"\\uDC00"')
    yield add("strings", '"\\uD800x"')
    yield add("strings", '"x\\uD800"')
    yield add("strings", '"\\uD800 \\uDC00"')
    yield add("strings", '"\\uD800\\u0041"')
    yield add("strings", '"\\uD800\\uD800\\uD800"')
    yield add("strings", '"\\uDC00\\uD800\\uDC00"')
    yield add("strings", '["\\uD800", "\\uD800"]')
    yield add("strings", '{"\\uD800": "\\uDC00"}')
    yield add("strings", '{"\\uD800\\uD800": 1}')
    yield add("strings", '"a\\uD800b\\uDC00c"')
    # Escape errors at every position.
    for body in ['"\\q"', '"\\u"', '"\\u1"', '"\\u12"', '"\\u123"', '"\\u123g"',
                 '"\\uZZZZ"', '"\\u12Z4"', '"a\\u123z"', '"\\ud800\\uzzzz"',
                 '"\\ud800\\u12"', '"\\ud800\\u"', '"\\ud800\\udc0"',
                 '"\\uD800\\uDC00"', '"\\ud800\\n"', '"\\ud800\\ue000"',
                 '"\\ud800\\udbff"', '"\\udc00\\ud800"', '"\\ud800 \\udc00"',
                 '"\\ud800\\', '"\\u1234', '"a\\u1234', '"\\', '"abc\\',
                 '"unterminated', '"', '"a', '["a]', '{"a": "b}']:
        yield add("escapes", body)
    for body in ['"\\u0041\\u0042"', '"\\u00e9\\u00e8"', '"\\ud800\\udc00\\ud800"',
                 '"\\ud800\\udbff"', '"\\udc00\\udc01"', '"\\ud800\\ud800x"',
                 '"x\\ud800\\ud800"', '"\\ud800\\ud800\\ud800"',
                 '"\\ud800\\udc00\\ud800\\udc00"',
                 '"\\ud800\\udc00\\ud800"', '"a\\ud800b"', '"\\uD7FF\\uE000"',
                 '"\\uDBFF\\uDFFF"', '"\\uDBFF\\uDBFF"',
                 '{"\\ud800": 1, "\\udc00": 2}', '["\\udc00\\udc00"]']:
        yield add("escapes", body)

    # --- valid structures ---------------------------------------------------
    for body in [
        "{}", "[]", "[[]]", "[{}]", "[[[]]]", '{"a":{}}', '{"a":[]}',
        '{"a": 1, "b": [1, 2, {"c": null}], "d": true, "e": false}',
        '{"z": 1, "a": 2, "m": 3}',
        '{"a": 1, "a": 2}', '{"a": 1, "b": 2, "a": 3}',
        '{"a": 1, "a": 2, "b": 3, "a": 4, "b": 5}',
        '{"x": {"a": 1, "a": 2}, "x": 3}',
        '{"é": 1, "é": 2}',
        '{"\\u00e9": 1, "é": 2}',
        '{"\\ud83d\\ude80": "rocket"}',
        '{"empty": "", "none": null, "t": true, "f": false}',
        '{ "a" : 1 , "b" : 2 }',
        ' \t\r\n [ \t\r\n 1 \t\r\n ] \t\r\n ',
        '[1,2,3]', '[[], {}, [1, [2, [3, [4]]]]]',
        '{"nested": {"a": {"b": {"c": [1, {"d": "e"}]}}}}',
        '[' + ",".join(str(i) for i in range(100)) + "]",
        '{"wide": {' + ",".join(f'"k{i}": {i}' for i in range(100)) + "}}",
        '[1.0, 2, "3", null, true, false, {}, []]',
    ]:
        yield add("structures", body)
    for keys in ["a", "b", "c"]:
        for order in [
            ["a", "b", "a"], ["a", "a", "b"], ["b", "a", "a"],
            ["a", "b", "c", "a", "b", "c"], ["c", "b", "a", "c", "b", "a"],
        ]:
            yield add(
                "structures",
                "{" + ",".join(f'"{key}": {index}' for index, key in enumerate(order)) + "}",
            )
    for body in ['{"a":1,"b":2,"c":3}', '{"a":{"b":1},"a":{"b":2}}',
                 '[{"a":1},{"a":2}]', '{"":1,"":2}', '{"a":1,"A":2}',
                 '{"é":1,"e\\u0301":2}', '{"\\ud83d\\ude80":1,"🚀":2}']:
        yield add("structures", body)

    # --- realistic issue-update bodies --------------------------------------
    realistic = {
        "name": "Fix flaky pagination cursor when filters change",
        "description_html": "<p>When the filter is applied twice the cursor points at the "
        "previous page.</p><ul><li>Open the view</li><li>Change the filter</li></ul>",
        "priority": "high",
        "state": "6f9c2c1e-4a44-4d3e-9b6f-6d8b0f9f3a11",
        "assignees": ["8a1b2c3d-4e5f-4a6b-8c9d-0e1f2a3b4c5d"],
        "labels": ["bug", "pagination", "backend"],
        "start_date": "2026-11-01",
        "target_date": "2026-12-01",
        "estimate": 2.5,
        "points": 3,
        "is_draft": False,
        "parent": None,
        "extra": {"nested": {"count": 12, "ratio": 0.125, "tags": ["a", "b"]}},
    }
    text = json.dumps(realistic)
    yield add("realistic", text)
    yield add("realistic", json.dumps(realistic, indent=2))
    yield add("realistic", json.dumps(realistic, ensure_ascii=False, separators=(",", ":")))
    yield add("realistic", json.dumps([realistic, realistic]))
    # Near misses of the same body.
    for index, replacement in [
        (3, "_"), (3, ""), (len(text) - 1, ""), (0, " "), (0, ""),
    ]:
        yield add("realistic", text[:index] + replacement + text[index + 1:])
    yield add("realistic", text[:-1])
    yield add("realistic", text[:-1] + ",}")

    # --- syntax errors ------------------------------------------------------
    malformed = [
        "{", "[", "}", "]", "{,}", "[,]", "[1,]", '{"a":1,}', '{"a"}',
        '{"a" 1}', '{"a":}', '{"a":1 "b":2}', "[1 2]", '{"a":1,,"b":2}',
        "tru", "truex", "nul", "nullx", "fals", "falsex", "True", "NULL",
        "01", "-01", "1.", "1.e5", "1e", "1e+", ".5", "+1", "-", "--1", "1e5x",
        "1.5e", "1.5e+", "1.5e+x", "1e1.5", "1.5.5", "0x10", "1_000", "- 1",
        "nan", "inf", "-inf", "Infinityx", "NaNx", "truefalse", "[true false]",
        "null,", '"a":1', "{}{}", "1 2", "null null", "[]x", "{}x", '"a"x',
        '"a" "b"', "[" * 100, "{" * 100, '{"a":' * 100,
        "{1:2}", "[,1]", "[1,,2]", '{"a":1}{', "[[]", '{"a":[}', '{"a":[1}',
        '{"a":{"b":1]}', "[1,2", '{"a":1', '{"a":1,', '{"a"', '{"a":',
        '{"a":1 ', "[1,\n2\n,\n]", "[\n1,\n]", '{"a":1}\n\n  x', "[1]x",
        '[1] [2]', "0 0", "1,2", "\t[1]\t2", "[", '{"":}', '{"":1}',
        '{"a":1,"":2}', "[-]", "[-.]", "[0.]", "[0e]", "[0e+]", "[00]",
        "[1e+1]", "[1E-1]", "[-0.0e-0]", "[-0e0]", "[-0e]", "[-.1]", "[- 0]",
        "[.1]", "[/]", "[\\]", '["\\"]', '["\\x"]',
    ]
    for body in malformed:
        yield add("syntax", body)

    # Multi-line, CRLF, tabs and non-ASCII before the error: the message must
    # count code points and lines, not bytes.
    for body in [
        '{"a":\n}', '{"a":\r\n}', '{\r\n"a"\r\n:\r\n1\r\n,\r\n}',
        '[\n\n\n]', '[1,\n\n2\n,\n]', '{"a":1\n\n\nx}',
        '["ééé", ]', '{"é": , }', '["中",\n "x",]', '["🚀",1,]',
        '"é\\q"', '"中中" x', '{"a":"éé" x}', '[\r\n"é"\r\n1]', '\n["é" x]',
        '{"日本":\n  "語", "bad": }', '"\u2028", ]', '{"a":"\\u2028", ]',
        '\n\n\n\n{"deep": [1, 2, {"x": }]}', '[\t1,\t]', '{\t"a"\t:\t}',
    ]:
        yield add("positions", body)
    for newlines in [0, 1, 2, 3, 7, 20]:
        for prefix in ['', ' ', '\t', '  \t', '\r\n', 'é', '中🚀']:
            yield add(
                "positions",
                "\n" * newlines + '{"key": "' + prefix + '", "bad": }',
            )
            yield add("positions", "\n" * newlines + "[" + prefix + " 1, ]")
            yield add("positions", "\n" * newlines + prefix + "}")

    # --- numbers ------------------------------------------------------------
    for body in ["0", "-0", "0.0", "-0.0", "1E2", "1e+2", "1e-2", "1e002",
                 "-0e-0", "1e400", "-1e400", "1e-400", "-1e-400",
                 "0.0000000000000000000001", "123456789012345678901234567890",
                 "1.", "-1.", "1.e5", "1e", "1e+", "1e-", "1e1.5", "1.5.5",
                 "01", "00", "-01", "1_000", "1,0", "0x10", "1d", "0b1"]:
        yield add("numbers", body)
    yield add("numbers", "9" * 4300)
    yield add("numbers", "-" + "9" * 4300)
    yield add("numbers", "9" * 4301)
    yield add("numbers", "-" + "9" * 4301)
    yield add("numbers", "1" * 5000)
    yield add("numbers", "-" + "7" * 6000)
    yield add("numbers", "[" + "9" * 4301 + "]")
    yield add("numbers", '{"a":' + "-" + "2" * 4301 + "}")
    yield add("numbers", '{"a": 1, "b": ' + "3" * 4302 + "}")
    yield add("numbers", "1" * 4300 + ".0")
    yield add("numbers", "1" * 4301 + ".0")
    yield add("numbers", "1" * 4301 + "e1")
    yield add("numbers", "0." + "1" * 5000)
    yield add("numbers", "1e" + "9" * 400)
    yield add("numbers", "1e-" + "9" * 400)

    # --- strict constants ---------------------------------------------------
    for body in ["NaN", "Infinity", "-Infinity", "[NaN]", "[Infinity]",
                 "[-Infinity]", '{"x": NaN}', '{"x": -Infinity}', "NaNx",
                 "Infinityx", "-Infinityx", '{"x": NaN, "y": }',
                 "[" + "NaN" + ", NaN]", '{"a": {"b": Infinity}}',
                 "trueNaN", "nullInfinity"]:
        yield add("constants", body)

    # --- BOM ----------------------------------------------------------------
    bom = b"\xef\xbb\xbf"
    for body in [bom, bom + b"{}", bom + b" ", bom + b"\n{}", b" " + bom + b"{}",
                 bom + bom + b"{}", bom + b"\xef\xbb\xbf\xff", b'"\xef\xbb\xbf"',
                 b'"' + bom + b'"', b"{}" + bom, bom + b'"\\u2028"']:
        yield add("bom", body)
    yield add("bom", bom + b"{", encoding="utf-8-sig")
    yield add("bom", bom + bom + b"{}", encoding="utf-8-sig")
    yield add("bom", bom + b"\xff", encoding="utf-8-sig")
    yield add("bom", b"\xef\xbb\xbf\xef\xbb\xbf\xff", encoding="utf-8-sig")
    yield add("bom", b"\xef\xbb\xbf\xef\xbb\xbf", encoding="utf-8-sig")
    yield add("bom", b"\xef\xbb\xbf ", encoding="UTF-8-SIG")
    yield add("bom", b"\xef\xbb", encoding="utf-8-sig")
    yield add("bom", b"\xef{}", encoding="utf-8-sig")

    # --- UTF-8 decoding -----------------------------------------------------
    bad_utf8 = [
        b"\xff", b"\x80", b"\xc0\x80", b"\xc1\xbf", b"\xc2", b"\xc2A",
        b"\xc2\x80", b"\xe0\x80\x80", b"\xe0\xa0", b"\xe2\x82", b'"\xe2\x82"',
        b"\xed\xa0\x80", b"\xed\xbf\xbf", b"\xf0\x80\x80\x80", b"\xf0\x9f\x98",
        b"\xf4\x90\x80\x80", b"\xf5\x80\x80\x80", b"\xfe", b"\xfe\xff",
        b"ab\xff", b"\x00\xff", b'"\xff"', b'["\xff"]', b'{"a": "\xff"}',
        b'{"a": 1}\xff', b'{"\xff": 1}', b'"abc\xc3"', b'"abc\xc3\xa9"',
        b'123\xc3', b'"\xf0\x9f', b'"\xf0\x9f\x98', b'"\xc3', b'["\xc3"',
        b'["\xe2\x82\xac"]', b'{"\xf0\x9f\x9a\x80": 1}', b'\xc3\xa9{}',
        b'"caf\xe9"', b'"caf\xc3\xa9"', b'"\\u00e9"',
    ]
    for body in bad_utf8:
        yield add("utf8", body)
    # The same bytes at different offsets, after valid multi-byte text.
    for prefix in [b"", b"[", b'["a",', b'{"k":', '["é",'.encode(), b"\n\n\t ",
                   b'{"a": "', b'[1, 2, ']:
        for bad in [b"\xff", b"\x80", b"\xc2A", b'"\xe2\x82"', b"\xe0\x80",
                    b"\xf0\x8f", b"\xed\xa0", b"\xf4\x90", b"\xc0\xaf",
                    b"\xf5", b"\xfe"]:
            yield add("utf8", prefix + bad)
    for length in [1, 2, 3]:
        for tail in [b"\xc3", b"\xe2", b"\xe2\x82", b"\xf0", b"\xf0\x9f",
                     b"\xf0\x9f\x98", b"\xed\xa0"]:
            yield add("utf8", b'{"ok": 1}' + tail * length)

    # --- other charsets -----------------------------------------------------
    latin = b'{"s": "caf\xe9", "n": 1}'
    for encoding in ["latin-1", "latin1", "iso-8859-1", "ISO8859-1", "l1",
                     "cp819", "8859", "latin", "iso_8859_1"]:
        yield add("latin1", latin, encoding)
    yield add("latin1", b"\xe9\xe8\xea", "latin-1")
    yield add("latin1", b'"\xff\x80\x9f"', "latin-1")
    yield add("latin1", b'{"a": "\xe9", "b": }', "latin-1")
    yield add("latin1", b'"\xe9\\q"', "latin-1")
    yield add("latin1", b'"caf\xe9"', "LATIN-1")
    yield add("latin1", b'{"\xe9": "\xe9"}', "latin-1")

    for encoding in ["cp1252", "windows-1252", "1252"]:
        yield add("cp1252", b'{"s": "\x80\x93hi\x94"}', encoding)
    for body in [b"\x81", b"\x8d", b"\x8f", b"\x90", b"\x9d", b'{"s": "\x81"}',
                 b'"a\x9d"', b"\x80{}", b'{"s": "\x80"}']:
        yield add("cp1252", body, "cp1252")

    for encoding in ["ascii", "us-ascii", "646", "ANSI_X3.4-1968"]:
        yield add("ascii", b'{"a": 1}', encoding)
    for body in [b"\xff", b"\x80", b'"\x80"', b'{"a": "\xe9"}', b"caf\xe9"]:
        yield add("ascii", body, "ascii")

    for encoding in ["utf-16", "UTF16", "u16"]:
        yield add("utf16", '{"a": 1}'.encode("utf-16"), encoding)
        yield add("utf16", "[\"é\", 1]".encode("utf-16"), encoding)
        yield add("utf16", b'{"a": 1}', encoding)
        yield add("utf16", b"\xff\xfe", encoding)
        yield add("utf16", b"\xff", encoding)
        yield add("utf16", b"\xff\xfe{\x00", encoding)
        yield add("utf16", b"\xff\xfe\x00\xd8A\x00", encoding)
        yield add("utf16", b"\xff\xfeA\x00\x00\xd8B\x00", encoding)
        yield add("utf16", b"\xff\xfeA\x00\x00\xd8", encoding)
        yield add("utf16", b"\xfe\xff\xd8\x00A\x00", encoding)
        yield add("utf16", b"\xef\xbb\xbf{}", encoding)
        yield add("utf16", b"\x00\xd8", encoding)
        yield add("utf16", b"A\x00\x00\xd8", encoding)
        yield add("utf16", b"A\x00B\x00", encoding)
        yield add("utf16", b"A\x00\x00\xd8\x00\xdc", encoding)
    for encoding in ["utf-16-le", "utf-16le", "unicodelittleunmarked"]:
        yield add("utf16", '{"a": 1}'.encode("utf-16-le"), encoding)
        yield add("utf16", b'{"a": 1}', encoding)
    for encoding in ["utf-16-be", "utf-16be", "unicodebigunmarked"]:
        yield add("utf16", '{"a": 1}'.encode("utf-16-be"), encoding)
        yield add("utf16", b'{"a": 1}', encoding)
    for body in [b"\x00\xd8\x00\xdc", b"\x00\xd8A\x00", b"\x00\xdc\x00\xd8",
                 b"\x00\xdcA\x00", b"\x00\xd8\x00\xd8", b"A\x00\x00\xd8",
                 b"A\x00\x00\xd8B\x00", b"A", b"\x41\x00\x42"]:
        yield add("utf16", body, "utf-16-le")

    for encoding in ["utf-32", "UTF32", "u32"]:
        yield add("utf32", '{"a": 1}'.encode("utf-32"), encoding)
        yield add("utf32", b'{"a": 1}', encoding)
        yield add("utf32", b"\xff\xfe\x00\x00", encoding)
        yield add("utf32", b"\xff\xfe", encoding)
        yield add("utf32", b"\xff\xfe\x00\x00A\x00\x00\x00\x00\x00\xd8\x00", encoding)
    for encoding in ["utf-32-le", "utf-32le"]:
        yield add("utf32", '{"a": 1}'.encode("utf-32-le"), encoding)
    for encoding in ["utf-32-be", "utf-32be"]:
        yield add("utf32", '{"a": 1}'.encode("utf-32-be"), encoding)
    for body in [b"\x00\xd8\x00\x00", b"\x00\xdc\x00\x00", b"\x00\x00\x11\x00",
                 b"A\x00\x00\x00B", b"\x00\xd8\x00", b"\x00\xd8"]:
        yield add("utf32", body, "utf-32-le")

    # --- unsupported charsets ----------------------------------------------
    # Only codecs Python itself would refuse (non-text) or names it cannot
    # resolve. A text codec this crate does not implement (cp437, koi8-r, the
    # other ISO-8859 parts) would decode in Django but is reported as an
    # unsupported charset here; see the crate docs.
    for encoding in ["bz2", "zlib", "base64", "rot13", "hex", "quopri",
                     "unknown-thing", ""]:
        yield add("charset", b'{"a": 1}', encoding)
    yield add("charset", b"\x00\x01\x02", "bz2")

    # --- deep nesting -------------------------------------------------------
    for depth in [1, 2, 10, 500, 1000, 3000, 9000, 9990, 9996, 9997]:
        yield add("deep", "[" * depth + "]" * depth)
        yield add("deep", '{"a":' * min(depth, 9997) + "1" + "}" * min(depth, 9997))
    for depth in [9998, 9999, 10_000]:
        yield add("recursion", "[" * depth + "]" * depth)
        yield add("recursion", '{"a":' * depth + "1" + "}" * depth)
    yield add("recursion", "[" * 9996 + "{}" + "]" * 9996)
    yield add("recursion", "[" * 9997 + "{}" + "]" * 9997)
    yield add("recursion", "[" * 9990 + '{"a":' * 4 + "1" + "}" * 4 + "]" * 9990)
    yield add("recursion", "[" * 9990 + '{"a":' * 9 + "1" + "}" * 9 + "]" * 9990)
    yield add("recursion", "[" * 50_000)
    yield add("recursion", '{"a":' * 20_000)
    yield add("recursion", ("[" * 9997) + ("]" * 9996))
    yield add("recursion", ('[' * 9997) + "1" + ("]" * 9997))

    # --- scanner corners ----------------------------------------------------
    for body in [
        # escapes at the very end of the document
        '"\\u1234', '"a\\u1234', '"\\ud800\\udc00', '"\\ud800\\udc00"',
        '"\\ud800\\u', '"\\ud800"', '"\\u"',
        # keys
        '{"a\\u12": 1}', '{"a\\q": 1}', '{"a', '{"a\\', '{"a\\u12',
        '{"\\ud800": 1}', '{"a": 1, "\\ud800": 2}',
        # numbers at the end of the document
        '-', '1e', '1e+', '1.', '-0.', '0e', '0e+', '0e-', '-0e',
        '1e999999999999999999999', '-1e999999999999999999999',
        '1e-999999999999999999999', '123456789012345678901234567890e-5',
        # constants
        'NaN,', 'Infinity ', '-Infinity\n', ' [ Infinity ] ',
        '{"a": Infinity, "b": NaN}', '[-Infinity, Infinity, NaN]',
        # extra data
        'true false', 'false true', 'null 0', '0 null', '"a" "b" "c"',
        '[] []', '{} {}', '[1][2]', '{"a":1}{"b":2}', '1.5 2.5',
        '[1] \n\t\r ', '{"a":1} \n', '  [1]  x  ',
        # whitespace between every token
        ' [ \n { \t "a" \r : \n [ \t 1 , \n 2 \t ] \r } \n ] ',
        '{\n\t"a"\n:\n1\n,\n"b"\n:\n2\n}',
        # latin-1 control and high bytes
        b'"\x00"', b'"\x1f"', b'"\x7f"', b'"\x80\x9f\xff"',
        b'{"\xe9": "\xe9"}', b'["\x00"]', b'"a\x00b"',
    ]:
        yield add("scanner", body)
    for body in [b'"\x00"', b'"\x1f"', b'"\x7f"', b'"\x80\x9f\xff"',
                 b'{"\xe9": "\xe9"}', b'["\x00"]', b'"a\x00b"',
                 b'{"\xe9": }']:
        yield add("scanner", body, "latin-1")

    # --- utf-16 documents that are not ASCII --------------------------------
    for text in ['{"é": "日本語"}', '["🚀", 1, null]', '{"a": "\\ud800"}',
                 '{"a": "\\ud800\\udc00"}', '{"a": 1e400}',
                 '{"a": NaN}', '{"a": }']:
        yield add("utf16", text.encode("utf-16"), "utf-16")
        yield add("utf16", text.encode("utf-16-le"), "utf-16-le")
        yield add("utf16", text.encode("utf-16-be"), "utf-16-be")
        yield add("utf32", text.encode("utf-32"), "utf-32")
        yield add("utf32", text.encode("utf-32-le"), "utf-32-le")

    # --- fuzz ---------------------------------------------------------------
    rng = random.Random(SEED)
    seeds = [
        text,
        json.dumps(realistic, indent=2),
        '{"a": 1, "b": [1, 2, {"c": "d"}], "e": null}',
        '[1, 2.5, "three", true, false, null, {}, []]',
        '{"unicode": "Zoë — 日本語 🚀", "escaped": "a\\tb\\u00e9\\ud83d\\ude80"}',
        '{"list": [' + ",".join(str(i) for i in range(20)) + "]}",
    ]
    alphabet = list('{}[],:"\\ \t\r\n0123456789.eE+-truefalsnNIy') + ["é", "中", "🚀"]
    for _ in range(1700 * FUZZ_SCALE):
        base = rng.choice(seeds)
        mutated = base
        for _ in range(rng.randint(1, 3)):
            kind = rng.randrange(5)
            if not mutated:
                break
            position = rng.randrange(len(mutated))
            if kind == 0:
                mutated = mutated[:position] + mutated[position + 1:]
            elif kind == 1:
                mutated = (
                    mutated[:position]
                    + rng.choice(alphabet)
                    + mutated[position:]
                )
            elif kind == 2:
                mutated = mutated[:position] + rng.choice(alphabet) + mutated[position + 1:]
            elif kind == 3:
                mutated = mutated[:position]
            else:
                width = rng.randint(1, 8)
                mutated = mutated[:position] + mutated[position:position + width] + mutated[position:]
        yield add("fuzz", mutated)

    for _ in range(500 * FUZZ_SCALE):
        length = rng.randrange(0, 60)
        body = bytes(rng.randrange(0, 256) for _ in range(length))
        yield add("fuzz", body)

    for _ in range(300 * FUZZ_SCALE):
        length = rng.randrange(0, 40)
        body = "".join(rng.choice(alphabet) for _ in range(length)).encode()
        yield add("fuzz", body)


def main() -> int:
    print(f"Django {django.get_version()}, Python {sys.version.split()[0]}")
    collector = Collector()
    for case in cases():
        run(case, collector)
    collector.write()
    print(f"total: {len(collector.records)} records")
    return 0


if __name__ == "__main__":
    sys.exit(main())
