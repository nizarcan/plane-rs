#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Record DRF's bytes for the serializer golden corpus.

Usage (from the repository root)::

    apps/server/dev/mkdb.sh t_ser_golden --replace
    PLANE_DB=t_ser_golden apps/api/.venv/bin/python \\
        apps/server/tools/golden/serializers_record.py

Writes ``apps/server/crates/plane-serializers/tests/golden/serializers.jsonl``:
one JSON object per line::

    {"case": "user_lite/no_args/utc",
     "serializer": "plane.app.serializers.user.UserLiteSerializer",
     "subject": "user1", "many": false, "tz": "UTC",
     "fields": null, "expand": null,
     "out": "{\"id\":\"…\",…}"}

``out`` is `JSONRenderer().render(serializer.data)` decoded as UTF-8; the Rust
test re-encodes its own output and compares byte for byte. ``fields`` keeps the
list form (including the dict-item form); ``expand`` is the list of names.
``subject`` selects a fixture instance, exactly as named in
``serializers_fixture.build()``.

The cases cover, for every serializer in ``GOLDEN_SERIALIZERS``:

* no arguments, in UTC, Asia/Kolkata and America/New_York (the last two change
  every datetime's offset; 2026-11-01 is a US DST boundary);
* ``fields=`` with one, several and an unknown name, plus the dict-item form;
* ``expand=`` for an injected relation, a base field that is in the mapper
  (value replaced), a base field that is not (replaced by ``<name>_id``), a
  name that is neither (no key), and ``issue_attachments``;
* ``fields=`` and ``expand=`` together;
* ``many=True`` for a list, which has its own output path.

``plane.api.serializers`` serializers use the other base class
(`FilterMode::RemoveFields`); their cases are here too.

The "probe" serializer at the bottom defines, inside this script, a
`ModelSerializer` over a `managed = False` model. Plane's models never use
`DecimalField`, `TimeField` or `BinaryField` in a readable position together,
and DRF's formatting for those (quantise, `isoformat` with microseconds,
`repr(bytes)`) is part of the contract the Rust field renderers reproduce.
"""

from __future__ import annotations

import argparse
import decimal
import json
import os
import sys
import zoneinfo
from dataclasses import dataclass
from datetime import date, datetime, time, timezone
from pathlib import Path
from typing import Any
from uuid import UUID

HERE = Path(__file__).resolve().parent
SERVER_DIR = HERE.parents[1]
REPO_ROOT = HERE.parents[3]
API_DIR = REPO_ROOT / "apps" / "api"
GOLDEN_DIR = SERVER_DIR / "crates" / "plane-serializers" / "tests" / "golden"
GOLDEN_FILE = GOLDEN_DIR / "serializers.jsonl"

sys.path.insert(0, str(HERE))
sys.path.insert(0, str(API_DIR))


@dataclass
class Case:
    case: str
    serializer: str
    subject: str
    tz: str = "UTC"
    fields: list[Any] | None = None
    expand: list[str] | None = None
    many: bool = False


def timezone_cases(name: str, serializer: str, subject: str, *, many: bool = False) -> list[Case]:
    return [
        Case(f"{name}/no_args/utc", serializer, subject, many=many),
        Case(f"{name}/no_args/kolkata", serializer, subject, tz="Asia/Kolkata", many=many),
        Case(
            f"{name}/no_args/new_york",
            serializer,
            subject,
            tz="America/New_York",
            many=many,
        ),
    ]


APP_USER = "plane.app.serializers.user"
APP_WORKSPACE = "plane.app.serializers.workspace"
APP_PROJECT = "plane.app.serializers.project"
APP_STATE = "plane.app.serializers.state"
APP_ISSUE = "plane.app.serializers.issue"
APP_CYCLE = "plane.app.serializers.cycle"
APP_MODULE = "plane.app.serializers.module"
V1_ISSUE = "plane.api.serializers.issue"
V1_WORKSPACE = "plane.api.serializers.workspace"

USER_LITE = f"{APP_USER}.UserLiteSerializer"
USER_ME = f"{APP_USER}.UserMeSerializer"
WORKSPACE_LITE = f"{APP_WORKSPACE}.WorkspaceLiteSerializer"
WORKSPACE = f"{APP_WORKSPACE}.WorkSpaceSerializer"
WORKSPACE_MEMBER = f"{APP_WORKSPACE}.WorkSpaceMemberSerializer"
PROJECT_LITE = f"{APP_PROJECT}.ProjectLiteSerializer"
PROJECT_DETAIL = f"{APP_PROJECT}.ProjectDetailSerializer"
STATE = f"{APP_STATE}.StateSerializer"
STATE_LITE = f"{APP_STATE}.StateLiteSerializer"
LABEL = f"{APP_ISSUE}.LabelSerializer"
ISSUE = f"{APP_ISSUE}.IssueSerializer"
ISSUE_FLAT = f"{APP_ISSUE}.IssueFlatSerializer"
ISSUE_DETAIL = f"{APP_ISSUE}.IssueDetailSerializer"
ISSUE_STATE = f"{APP_ISSUE}.IssueStateSerializer"
CYCLE = f"{APP_CYCLE}.CycleSerializer"
MODULE = f"{APP_MODULE}.ModuleSerializer"
V1_ISSUE_LITE = f"{V1_ISSUE}.IssueLiteSerializer"
V1_WORKSPACE_LITE = f"{V1_WORKSPACE}.WorkspaceLiteSerializer"


CASES: list[Case] = [
    # UserLiteSerializer: the shape every nested user uses.
    *timezone_cases("user_lite", USER_LITE, "user1"),
    Case("user_lite/empty_user", USER_LITE, "user_empty"),
    Case("user_lite/bot", USER_LITE, "user_bot"),
    Case("user_lite/many", USER_LITE, "user_pair", many=True),
    # UserMeSerializer.
    *timezone_cases("user_me", USER_ME, "user1"),
    # WorkspaceLiteSerializer: name, slug, id, logo_url.
    *timezone_cases("workspace_lite", WORKSPACE_LITE, "workspace1"),
    # WorkSpaceSerializer: fields="__all__", so `owner` is a base field that is
    # not in the mapper -> expand replaces it with `owner_id`.
    *timezone_cases("workspace", WORKSPACE, "workspace1"),
    Case("workspace/fields", WORKSPACE, "workspace1", fields=["id", "name", "slug"]),
    Case(
        "workspace/fields_computed",
        WORKSPACE,
        "workspace1",
        fields=["total_members", "role", "logo_url"],
    ),
    Case("workspace/expand_owner", WORKSPACE, "workspace1", expand=["owner"]),
    Case(
        "workspace/fields_and_expand_owner",
        WORKSPACE,
        "workspace1",
        fields=["id", "name"],
        expand=["owner"],
    ),
    Case(
        "workspace/fields_and_expand_unknown",
        WORKSPACE,
        "workspace1",
        fields=["id"],
        expand=["not_a_relation"],
    ),
    # WorkSpaceMemberSerializer: `member` is a declared nested serializer and a
    # base field; expand=member is not in the mapper -> member_id.
    *timezone_cases("workspace_member", WORKSPACE_MEMBER, "member1"),
    Case(
        "workspace_member/fields",
        WORKSPACE_MEMBER,
        "member1",
        fields=["id", "member", "role"],
    ),
    Case(
        "workspace_member/expand_member",
        WORKSPACE_MEMBER,
        "member1",
        expand=["member"],
    ),
    Case(
        "workspace_member/expand_workspace",
        WORKSPACE_MEMBER,
        "member1",
        expand=["workspace"],
    ),
    Case(
        "workspace_member/fields_and_expand_workspace",
        WORKSPACE_MEMBER,
        "member1",
        fields=["id", "role"],
        expand=["workspace"],
    ),
    # ProjectLiteSerializer.
    *timezone_cases("project_lite", PROJECT_LITE, "project1"),
    # ProjectDetailSerializer: the four skippable annotations present with
    # values, and present and None. `null` is the DRF output for the latter;
    # only a missing attribute omits the key.
    Case("project_detail/annotations", PROJECT_DETAIL, "project_annotated"),
    Case("project_detail/annotations_null", PROJECT_DETAIL, "project_null"),
    # StateSerializer: `order` is required=False over an annotation, so the
    # plain instance omits it (SkipField) and an instance whose attribute is
    # None writes `"order": null`.
    *timezone_cases("state", STATE, "state1"),
    Case("state/with_order", STATE, "state1_with_order"),
    Case("state/order_null", STATE, "state1_with_order_none"),
    Case("state/many", STATE, "state_pair", many=True),
    # StateLiteSerializer.
    *timezone_cases("state_lite", STATE_LITE, "state1"),
    # LabelSerializer.
    *timezone_cases("label", LABEL, "label1"),
    # IssueSerializer: the annotation-driven fields, every expansion shape and
    # the issue_attachments block.
    *timezone_cases("issue", ISSUE, "issue1"),
    Case("issue/fields_id_name", ISSUE, "issue1", fields=["id", "name"]),
    Case(
        "issue/fields_annotations",
        ISSUE,
        "issue1",
        fields=["label_ids", "assignee_ids", "module_ids", "cycle_id"],
    ),
    Case(
        "issue/fields_counts",
        ISSUE,
        "issue1",
        fields=["sub_issues_count", "attachment_count", "link_count"],
    ),
    Case("issue/fields_unknown", ISSUE, "issue1", fields=["id", "nope"]),
    Case("issue/fields_dict", ISSUE, "issue1", fields=[{"name": []}]),
    Case(
        "issue/fields_dict_two_keys",
        ISSUE,
        "issue1",
        fields=[{"priority": [], "name": []}],
    ),
    Case("issue/fields_empty", ISSUE, "issue1", fields=[]),
    Case(
        "issue/fields_attachments_without_expand",
        ISSUE,
        "issue1",
        fields=["id", "issue_attachments"],
    ),
    Case("issue/expand_state", ISSUE, "issue1", expand=["state"]),
    Case("issue/expand_created_by", ISSUE, "issue1", expand=["created_by"]),
    Case("issue/expand_labels", ISSUE, "issue1", expand=["labels"]),
    Case("issue/expand_assignees", ISSUE, "issue1", expand=["assignees"]),
    Case("issue/expand_parent_null", ISSUE, "issue1", expand=["parent"]),
    Case("issue/expand_cycle_not_a_field", ISSUE, "issue1", expand=["cycle"]),
    Case(
        "issue/expand_attachments",
        ISSUE,
        "issue1",
        expand=["issue_attachments"],
    ),
    Case(
        "issue/expand_state_and_created_by",
        ISSUE,
        "issue1",
        expand=["state", "created_by"],
    ),
    Case(
        "issue/fields_and_expand_state",
        ISSUE,
        "issue1",
        fields=["id", "name", "state_id"],
        expand=["state"],
    ),
    Case(
        "issue/fields_and_expand_attachments",
        ISSUE,
        "issue1",
        fields=["id"],
        expand=["issue_attachments"],
    ),
    Case(
        "issue/expand_injected_and_replaced",
        ISSUE,
        "issue1",
        fields=["id", "state_id"],
        expand=["state", "labels"],
    ),
    # The skippable annotations exist but are None: every one is `null` in the
    # output, and the two-state `ListField`s with `allow_null=True` are null
    # too (a nullable value is never a SkipField).
    Case("issue/annotations_null", ISSUE, "issue_null_annotations"),
    Case("issue/many", ISSUE, "issue_pair", many=True),
    # IssueFlatSerializer.
    *timezone_cases("issue_flat", ISSUE_FLAT, "issue1"),
    # IssueDetailSerializer adds description_html/is_subscribed/is_intake.
    *timezone_cases("issue_detail", ISSUE_DETAIL, "issue1"),
    Case(
        "issue_detail/fields",
        ISSUE_DETAIL,
        "issue1",
        fields=["id", "description_html", "is_subscribed", "is_intake"],
    ),
    Case(
        "issue_detail/fields_and_expand",
        ISSUE_DETAIL,
        "issue1",
        fields=["id", "description_html"],
        expand=["state", "issue_attachments"],
    ),
    # IssueStateSerializer: fields="__all__", so state/assignees/labels are base
    # fields *and* mapper names -> expansion replaces them in place.
    *timezone_cases("issue_state", ISSUE_STATE, "issue1"),
    Case(
        "issue_state/fields",
        ISSUE_STATE,
        "issue1",
        fields=["id", "state", "assignees", "labels"],
    ),
    Case("issue_state/expand_state", ISSUE_STATE, "issue1", expand=["state"]),
    Case("issue_state/expand_labels", ISSUE_STATE, "issue1", expand=["labels"]),
    Case("issue_state/expand_project", ISSUE_STATE, "issue1", expand=["project"]),
    # CycleSerializer.
    *timezone_cases("cycle", CYCLE, "cycle1"),
    # ModuleSerializer.
    *timezone_cases("module", MODULE, "module1"),
    Case("module/fields", MODULE, "module1", fields=["id", "name", "member_ids"]),
    Case(
        "module/fields_computed",
        MODULE,
        "module1",
        fields=["total_estimate_points", "completed_estimate_points", "status"],
    ),
    Case("module/expand_project", MODULE, "module1", expand=["project"]),
    Case("module/expand_created_by", MODULE, "module1", expand=["created_by"]),
    # The /api/v1/ base class: fields= removes, expand= never injects.
    *timezone_cases("v1_issue_lite", V1_ISSUE_LITE, "issue1"),
    Case("v1_issue_lite/fields_id", V1_ISSUE_LITE, "issue1", fields=["id"]),
    Case("v1_issue_lite/fields_empty", V1_ISSUE_LITE, "issue1", fields=[]),
    Case(
        "v1_issue_lite/fields_project",
        V1_ISSUE_LITE,
        "issue1",
        fields=["project_id", "id"],
    ),
    Case(
        "v1_issue_lite/expand_project_id",
        V1_ISSUE_LITE,
        "issue1",
        expand=["project_id"],
    ),
    Case(
        "v1_issue_lite/expand_workspace",
        V1_ISSUE_LITE,
        "issue1",
        expand=["workspace"],
    ),
    *timezone_cases("v1_workspace_lite", V1_WORKSPACE_LITE, "workspace1"),
    Case(
        "v1_workspace_lite/fields",
        V1_WORKSPACE_LITE,
        "workspace1",
        fields=["name", "slug"],
    ),
    Case(
        "v1_workspace_lite/expand_name",
        V1_WORKSPACE_LITE,
        "workspace1",
        expand=["name"],
    ),
    Case(
        "v1_workspace_lite/fields_and_expand",
        V1_WORKSPACE_LITE,
        "workspace1",
        fields=["id", "name"],
        expand=["name", "workspace"],
    ),
]


# ---------------------------------------------------------------------------
# Probe serializer: field classes Plane's models do not carry.
# ---------------------------------------------------------------------------


def probe_cases() -> list[Case]:
    serializer = "golden.GoldenProbeSerializer"
    return [
        *timezone_cases("probe", serializer, "probe1"),
    ]


def define_probe() -> Any:
    """Define the probe model and serializer, then return an instance.

    `managed = False` and `app_label = "db"`: the model is registered so DRF's
    `ModelSerializer` can introspect it, and Django never creates or queries a
    table for it. The instance is unsaved and every attribute is set.
    """
    from django.db import models
    from rest_framework import serializers

    global GoldenProbeModel

    class GoldenProbeModel(models.Model):  # noqa: N801 - Django model style
        id = models.UUIDField(primary_key=True)
        count = models.IntegerField()
        ratio = models.DecimalField(max_digits=6, decimal_places=2)
        flag = models.BooleanField()
        name = models.CharField(max_length=200)
        date = models.DateField()
        time = models.TimeField()
        when = models.DateTimeField()
        payload = models.JSONField()
        blob = models.BinaryField()
        nullable_text = models.CharField(max_length=200, null=True)

        class Meta:
            app_label = "db"
            managed = False
            db_table = "golden_probe"

    class GoldenProbeSerializer(serializers.ModelSerializer):
        ratio_float = serializers.DecimalField(
            max_digits=6, decimal_places=2, coerce_to_string=False
        )

        class Meta:
            model = GoldenProbeModel
            fields = [
                "id",
                "count",
                "ratio",
                "ratio_float",
                "flag",
                "name",
                "date",
                "time",
                "when",
                "payload",
                "blob",
                "nullable_text",
            ]

    define_probe.serializer = GoldenProbeSerializer
    instance = GoldenProbeModel(
        id=UUID("99999999-9999-4999-8999-999999999999"),
        count=7,
        ratio=decimal.Decimal("12.30"),
        flag=True,
        name='Zo\u00eb "quoted" \\ backslash \n newline \u2028 line',
        date=date(2026, 11, 1),
        time=time(1, 2, 3, 456789),
        when=datetime(2026, 11, 1, 6, 30, 0, 123400, tzinfo=timezone.utc),
        payload={"z": 1, "a": [1, 2.5e-05, "\u00e9"], "m": {"y": True, "b": None}},
        blob=b"\x00\xffit's",
        nullable_text=None,
    )
    # `ratio_float` is a declared field (coerce_to_string=False); its source is
    # the same attribute name on the unsaved instance.
    instance.ratio_float = decimal.Decimal("12.30")
    return instance


# ---------------------------------------------------------------------------
# Recording
# ---------------------------------------------------------------------------


def render_case(case: Case, subjects: dict[str, Any], probe: Any) -> str:
    from rest_framework.renderers import JSONRenderer
    from rest_framework.serializers import BaseSerializer

    if case.serializer == "golden.GoldenProbeSerializer":
        serializer_class = define_probe.serializer
        instance = probe
    else:
        module_name, _, class_name = case.serializer.rpartition(".")
        serializer_class = getattr(__import__(module_name, fromlist=[class_name]), class_name)
        instance = subjects[case.subject]
    kwargs: dict[str, Any] = {}
    if case.fields is not None:
        kwargs["fields"] = case.fields
    if case.expand is not None:
        kwargs["expand"] = case.expand
    serializer = serializer_class(instance, many=case.many, **kwargs)
    assert isinstance(serializer, BaseSerializer)
    return JSONRenderer().render(serializer.data).decode("utf-8")


def main(argv: list[str] | None = None) -> int:
    from django.utils import timezone as dj_timezone

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=GOLDEN_FILE)
    args = parser.parse_args(argv)

    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    import django

    django.setup()

    import serializers_fixture

    subjects = serializers_fixture.build()
    probe = define_probe()

    records: list[dict[str, Any]] = []
    for case in CASES + probe_cases():
        zone = zoneinfo.ZoneInfo(case.tz)
        dj_timezone.activate(zone)
        try:
            out = render_case(case, subjects, probe)
        finally:
            dj_timezone.deactivate()
        records.append(
            {
                "case": case.case,
                "serializer": case.serializer,
                "subject": case.subject,
                "many": case.many,
                "tz": case.tz,
                "fields": case.fields,
                "expand": case.expand,
                "out": out,
            }
        )

    records.sort(key=lambda record: record["case"])
    names = [record["case"] for record in records]
    duplicates = {name for name in names if names.count(name) > 1}
    if duplicates:
        raise SystemExit(f"duplicate case names: {sorted(duplicates)}")

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(
        "\n".join(json.dumps(record, ensure_ascii=False) for record in records) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"wrote {args.out} ({len(records)} cases)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
