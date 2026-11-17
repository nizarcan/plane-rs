# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record the `many=True` error corpus used by `write::list_of`.

`BulkEstimatePointEndpoint.create` validates `EstimatePointSerializer(
data=request.data.get("estimate_points"), many=True)`
(`apps/api/plane/app/views/estimate/base.py:78-80`). The list envelope — the
`data=None` special case, the non-list message, the index-keyed error object
and the `null` element — is DRF's `ListSerializer`, so the corpus is recorded
from the **real** `plane.app.serializers.estimate.EstimatePointSerializer`
(DRF 3.18.3 from `apps/api/.venv`), with Django set up the way
`tools/inventory/serializers.py` does and no database connection:

    DJANGO_SETTINGS_MODULE=plane.settings.production, dummy service URLs

`LIST_SERIALIZER_ERRORS_AS_DICT` is DRF 3.18.3's shipped `True`
(`rest_framework/settings.py:89`); Plane does not override it.

Each case records:

  * `absent` — the call site's key is missing, so `data=None` is passed;
  * `input` — otherwise, the JSON text handed over as `data`;
  * `valid` — `serializer.is_valid()`;
  * `body` — `JSONRenderer().render(serializer.errors)`, byte for byte
    (compact separators, `ensure_ascii=False`);
  * `children` — for a list input, one entry per element: the JSON text of
    `exc.detail` from `serializer.child.run_validation(item)`, or `null` when
    that element validated. `write::list_of` composes exactly these into the
    index-keyed body, so the Rust test feeds them back in and compares the
    result with `body`; the two shapes that never reach a child (`data=None`,
    a non-list) run with a child validator that must not be called.

Regenerate from the repository root:

    apps/api/.venv/bin/python apps/server/tools/inventory/estimate_list_corpus.py

`--check` exits 1 when the committed file is stale.
"""

import json
import os
import pathlib
import sys

API_DIR = pathlib.Path(__file__).resolve().parents[3] / "api"
OUT = (
    pathlib.Path(__file__).resolve().parents[2]
    / "crates/plane-http-app/tests/golden/estimate-list-errors.json"
)


def setup_django():
    """Import Django with production settings, without touching a service."""
    if str(API_DIR) not in sys.path:
        sys.path.insert(0, str(API_DIR))

    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    os.environ.setdefault("SECRET_KEY", "inventory")
    os.environ.setdefault("DATABASE_URL", "postgresql://x:x@127.0.0.1:1/x")
    os.environ.setdefault("REDIS_URL", "redis://127.0.0.1:1/0")
    os.environ.setdefault("ENABLE_DRF_SPECTACULAR", "0")

    import django

    django.setup()
    return django


django = setup_django()

from rest_framework import serializers  # noqa: E402
from rest_framework.renderers import JSONRenderer  # noqa: E402

from plane.app.serializers import EstimatePointSerializer  # noqa: E402

RENDERER = JSONRenderer()

# `(name, input)` where `input` is the Python value the call site passes as
# `data`; `ABSENT` stands for the missing key (`request.data.get(…)` → None).
ABSENT = object()

CASES = [
    ("absent", ABSENT),
    ("explicit-null", None),
    ("empty-dict", {}),
    ("empty-list", []),
    ("one-empty-dict", [{}]),
    ("one-valid", [{"value": "a"}]),
    ("blank-value", [{"value": ""}]),
    ("null-value", [{"value": None}]),
    ("string-item", ["x"]),
    ("null-item", [None]),
    ("int-item", [1]),
    ("float-item", [1.5]),
    ("bool-item", [True]),
    ("list-item", [[]]),
    ("dict-value", [{"value": {"a": 1}}]),
    ("list-value", [{"value": ["a"]}]),
    ("bool-value", [{"value": True}]),
    ("int-value", [{"value": 7}]),
    ("bad-key", [{"key": "abc"}]),
    ("negative-key", [{"key": -1}]),
    ("key-as-string", [{"key": "1", "value": "ok"}]),
    ("value-21", [{"value": "123456789012345678901"}]),
    ("value-20", [{"value": "12345678901234567890"}]),
    ("value-256", [{"value": "x" * 256}]),
    (
        "middle-item-fails",
        [{"value": "1"}, {"value": ""}, {"value": "3"}],
    ),
    ("first-item-fails", [{}, {"value": "ok"}]),
    ("two-field-errors", [{"key": "abc", "value": ""}]),
    ("bad-datetime", [{"deleted_at": "nope", "value": "x"}]),
    ("null-datetime", [{"deleted_at": None, "value": "x"}]),
    ("null-description", [{"description": None}]),
    ("unknown-key", [{"extra": 1, "value": "x"}]),
    (
        "read-only-keys",
        [
            {
                "value": "x",
                "id": "0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f",
                "project": "0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f",
                "workspace": "0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f",
                "estimate": "0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f",
                "created_by": "0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f",
                "updated_by": "0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f",
                "created_at": "nope",
            }
        ],
    ),
    ("non-list-string", "abc"),
    ("non-list-int", 5),
    ("non-list-float", 1.5),
    ("non-list-bool", True),
    ("non-list-dict", {"a": 1}),
    (
        "many-failures",
        [
            {"value": ""},
            {"value": "ok"},
            {"key": "abc"},
            None,
            {"value": "x" * 21},
        ],
    ),
]


def render(data) -> str:
    return RENDERER.render(data).decode("utf-8")


def child_details(serializer, data):
    """`serializer.child.run_validation(item)`'s `exc.detail`, rendered."""
    details = []
    for item in data:
        try:
            serializer.child.run_validation(item)
        except serializers.ValidationError as error:
            details.append(render(error.detail))
        else:
            details.append(None)
    return details


def build_cases():
    cases = []
    for name, data in CASES:
        serializer = EstimatePointSerializer(
            data=None if data is ABSENT else data, many=True
        )
        valid = serializer.is_valid()
        case = {"name": name}
        if data is ABSENT:
            case["absent"] = True
        else:
            case["input"] = json.dumps(data, ensure_ascii=False, separators=(",", ":"))
        case["valid"] = valid
        case["body"] = None if valid else render(serializer.errors)
        if isinstance(data, list):
            # The child details are the same whether the list failed or not;
            # `list_of` only consumes them when it did.
            case["children"] = child_details(serializer, data)
        cases.append(case)
    return cases


def document():
    cases = build_cases()
    return {
        "generated_by": "apps/server/tools/inventory/estimate_list_corpus.py",
        "python": sys.version.split()[0],
        "django": django.get_version(),
        "drf": __import__("rest_framework").VERSION,
        "serializer": "plane.app.serializers.estimate.EstimatePointSerializer",
        "cases": cases,
    }


def render_file(payload) -> str:
    header = {key: value for key, value in payload.items() if key != "cases"}
    body = ",\n".join(
        json.dumps(case, ensure_ascii=False, separators=(",", ":"))
        for case in payload["cases"]
    )
    return (
        json.dumps(header, ensure_ascii=False, separators=(",", ":"))[:-1]
        + ',"cases":[\n'
        + body
        + "\n]}\n"
    )


def main(argv):
    text = render_file(document())
    if "--check" in argv:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != text:
            print("stale: %s" % OUT, file=sys.stderr)
            return 1
        print("corpus is up to date")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8")
    print("wrote %d cases to %s" % (len(document()["cases"]), OUT))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
