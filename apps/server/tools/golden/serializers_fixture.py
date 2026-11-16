#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Build the serializer golden fixture in a throwaway database.

Usage (from the repository root)::

    apps/server/dev/mkdb.sh t_ser_golden --replace
    PLANE_DB=t_ser_golden apps/api/.venv/bin/python \\
        apps/server/tools/golden/serializers_fixture.py

``serializers_record.py`` calls :func:`build` itself, so the two commands
above are only useful for inspection. The database must be a fresh clone of
``plane_tpl`` (``dev/mkdb.sh``): the fixture refuses a name that does not start
with ``t_`` and refuses a clone that already has users, because a second run
would collide on the fixed ids.

Why a database at all: several serializers read more than their own row.
``IssueSerializer`` renders `label_ids`/`assignee_ids`/`module_ids` from view
annotations and `expand=labels`/`expand=assignees`/`expand=issue_attachments`
walk reverse relations, and `DynamicBaseSerializer` resolves expansions against
managers. The values are fixed (ids, timestamps, JSON blobs), and the fixture
overwrites the auto audit columns after creation so the recorded bytes do not
depend on when the fixture ran.

Every value here is mirrored in
``apps/server/crates/plane-serializers/tests/golden.rs`` under
``mod.fixture``; keep the two in step. The constants at the top of this file
are the single source of the ids and timestamps in Python.
"""

from __future__ import annotations

import sys
from datetime import date, datetime, timezone
from pathlib import Path
from uuid import UUID

HERE = Path(__file__).resolve().parent
SERVER_DIR = HERE.parents[1]

# ---------------------------------------------------------------------------
# Fixed values. Mirror these in tests/golden.rs.
# ---------------------------------------------------------------------------

USER_ALICE = UUID("11111111-1111-4111-8111-111111111111")
USER_BOB = UUID("22222222-2222-4222-8222-222222222222")
USER_BOT = UUID("33333333-3333-4333-8333-333333333333")
USER_EMPTY = UUID("44444444-4444-4444-8444-444444444444")
WORKSPACE_ID = UUID("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
PROJECT_ID = UUID("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")
STATE_ID = UUID("cccccccc-cccc-4ccc-8ccc-cccccccccccc")
STATE_TRIAGE_ID = UUID("c1c1c1c1-c1c1-41c1-81c1-c1c1c1c1c1c1")
LABEL_ID = UUID("dddddddd-dddd-4ddd-8ddd-dddddddddddd")
ISSUE_ID = UUID("eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee")
ISSUE_CHILD_ID = UUID("e1e1e1e1-e1e1-41e1-81e1-e1e1e1e1e1e1")
CYCLE_ID = UUID("ffffffff-ffff-4fff-8fff-ffffffffffff")
MODULE_ID = UUID("a1b2c3d4-a1b2-41c3-81d4-a1b2c3d4a1b2")
MEMBER_ROW_ID = UUID("b2c3d4e5-b2c3-42d4-83e5-b2c3d4e5b2c3")
ATTACHMENT_ID = UUID("c3d4e5f6-c3d4-43e5-84f6-c3d4e5f6c3d4")

# 2026-11-01 is the US DST fall-back date: 05:30Z is 01:30 EDT (-04:00) and
# 06:30Z is 01:30 EST (-05:00). A serializer rendered in America/New_York on
# both values must show the two offsets.
T_CREATED = datetime(2026, 11, 1, 5, 30, 0, tzinfo=timezone.utc)
T_UPDATED = datetime(2026, 11, 1, 6, 30, 0, tzinfo=timezone.utc)
T_USER_JOINED = datetime(2024, 1, 2, 3, 4, 5, 678901, tzinfo=timezone.utc)
T_LOGIN = datetime(2025, 12, 31, 23, 59, 59, tzinfo=timezone.utc)
T_BOB_JOINED = datetime(2023, 6, 7, 8, 9, 10, tzinfo=timezone.utc)

DESCRIPTION_JSON = {
    "type": "doc",
    "content": [
        {
            "type": "paragraph",
            "content": [
                {"type": "text", "text": "Gear \U0001f6e0\ufe0f \u2028 line"},
            ],
        }
    ],
}
LOGO_PROPS = {"emoji": "\U0001f3a8", "color": "#abcdef"}
VIEW_PROPS = {"filters": {"priority": "high"}, "count": 3}
PROGRESS_SNAPSHOT = {"completed": 1, "total": 2}
MODULE_JSON = {"a": 1, "nested": {"z": True, "b": None}}
BINARY = b"\x00\xffit's"


def die(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def utc(*args: int) -> datetime:
    return datetime(*args, tzinfo=timezone.utc)


def build() -> dict[str, object]:
    """Create the fixture rows and return the instances the goldens render.

    The returned mapping is keyed by the ``subject`` names used in
    ``serializers_record.CASES`` and in the Rust golden test.
    """
    from django.conf import settings
    from django.contrib.auth.hashers import make_password
    from django.db import transaction

    from plane.db.models import (
        Cycle,
        FileAsset,
        Issue,
        IssueAssignee,
        IssueLabel,
        Label,
        Module,
        Project,
        State,
        User,
        Workspace,
        WorkspaceMember,
    )

    db_name = settings.DATABASES["default"]["NAME"]
    if not str(db_name).startswith("t_"):
        die(f"refusing to write to database {db_name!r}: expected a t_* clone (dev/mkdb.sh)")
    if User.objects.exists():
        die(f"database {db_name!r} already has users; re-clone it with dev/mkdb.sh --replace")

    with transaction.atomic():
        password = make_password("golden-password")
        alice = User.objects.create(
            id=USER_ALICE,
            username="alice",
            email="alice@example.com",
            password=password,
            display_name="Alice A.",
            first_name="Alice",
            last_name="Anderson",
            avatar="https://cdn.example.com/avatars/alice.png",
            cover_image="https://cdn.example.com/covers/alice.png",
            user_timezone="Asia/Kolkata",
            date_joined=T_USER_JOINED,
            last_login_time=T_LOGIN,
            last_login_medium="password",
            last_location="Bengaluru",
            created_location="Bengaluru",
            token="token-alice",
            last_login_ip="203.0.113.10",
            last_logout_ip="203.0.113.11",
            last_login_uagent="golden-agent",
        )
        bob = User.objects.create(
            id=USER_BOB,
            username="bob",
            email="bob@example.com",
            password=password,
            display_name="Bob B.",
            first_name="Bob",
            last_name="",
            avatar="",
            cover_image="",
            user_timezone="America/New_York",
            date_joined=T_BOB_JOINED,
            last_location="",
            created_location="",
            token="token-bob",
            last_login_ip="",
            last_logout_ip="",
            last_login_uagent="",
        )
        bot = User.objects.create(
            id=USER_BOT,
            username="plane-bot",
            email="bot@example.com",
            password=password,
            display_name="Plane Bot",
            first_name="Plane",
            last_name="Bot",
            avatar="",
            cover_image="",
            user_timezone="UTC",
            date_joined=T_BOB_JOINED,
            is_bot=True,
            last_location="",
            created_location="",
            token="token-bot",
            last_login_ip="",
            last_logout_ip="",
            last_login_uagent="",
        )
        empty = User.objects.create(
            id=USER_EMPTY,
            username="nobody",
            email="nobody@example.com",
            password=password,
            display_name="",
            first_name="",
            last_name="",
            avatar="",
            cover_image="",
            user_timezone="UTC",
            date_joined=T_BOB_JOINED,
            last_location="",
            created_location="",
            token="token-empty",
            last_login_ip="",
            last_logout_ip="",
            last_login_uagent="",
        )

        workspace = Workspace.objects.create(
            id=WORKSPACE_ID,
            name="Acme",
            slug="acme",
            owner=alice,
            logo="https://cdn.example.com/logos/acme.png",
            timezone="America/New_York",
            organization_size="10",
            background_color="#123456",
            created_by=alice,
            updated_by=alice,
        )
        project = Project.objects.create(
            id=PROJECT_ID,
            name="Apollo",
            identifier="APL",
            workspace=workspace,
            description="Landing on the moon",
            description_text={"text": "Landing on the moon"},
            logo_props=LOGO_PROPS,
            cover_image="https://cdn.example.com/covers/apollo.png",
            default_assignee=bob,
            project_lead=alice,
            timezone="America/New_York",
            created_by=alice,
            updated_by=alice,
        )
        state = State.objects.create(
            id=STATE_ID,
            project=project,
            workspace=workspace,
            name="In Progress",
            description="Work in progress",
            color="#f59e0b",
            slug="in-progress",
            sequence=2.5,
            group="started",
            default=False,
            created_by=alice,
            updated_by=alice,
        )
        triage = State.objects.create(
            id=STATE_TRIAGE_ID,
            project=project,
            workspace=workspace,
            name="Triage",
            description="",
            color="#6b7280",
            slug="triage",
            sequence=0.5,
            group="triage",
            default=False,
            is_triage=True,
            created_by=alice,
            updated_by=alice,
        )
        label = Label.objects.create(
            id=LABEL_ID,
            project=project,
            workspace=workspace,
            name="backend",
            description="",
            color="#00ff00",
            sort_order=65535,
            created_by=alice,
            updated_by=alice,
        )
        cycle = Cycle.objects.create(
            id=CYCLE_ID,
            project=project,
            workspace=workspace,
            name="Sprint 1",
            description="First sprint",
            start_date=T_CREATED,
            end_date=datetime(2026, 11, 8, 6, 30, tzinfo=timezone.utc),
            owned_by=alice,
            view_props=VIEW_PROPS,
            progress_snapshot=PROGRESS_SNAPSHOT,
            logo_props=LOGO_PROPS,
            timezone="America/New_York",
            created_by=alice,
            updated_by=alice,
        )
        module = Module.objects.create(
            id=MODULE_ID,
            project=project,
            workspace=workspace,
            name="Checkout",
            description="Checkout revamp",
            description_text=MODULE_JSON,
            description_html=MODULE_JSON,
            start_date=date(2026, 10, 27),
            target_date=date(2026, 11, 20),
            status="in-progress",
            lead=alice,
            view_props=VIEW_PROPS,
            logo_props=LOGO_PROPS,
            created_by=alice,
            updated_by=alice,
        )
        issue = Issue.objects.create(
            id=ISSUE_ID,
            project=project,
            workspace=workspace,
            name="Fix the landing gear",
            description_json=DESCRIPTION_JSON,
            description_html="<p>Gear</p>",
            description_stripped="Gear",
            description_binary=BINARY,
            priority="high",
            start_date=date(2026, 10, 30),
            target_date=date(2026, 11, 5),
            sequence_id=42,
            sort_order=65535,
            state=state,
            created_by=alice,
            updated_by=bob,
        )
        child = Issue.objects.create(
            id=ISSUE_CHILD_ID,
            project=project,
            workspace=workspace,
            name="Inspect the strut",
            description_json={},
            description_html="<p>Strut</p>",
            description_stripped="Strut",
            description_binary=b"child",
            priority="low",
            sequence_id=43,
            sort_order=65535,
            state=state,
            parent=issue,
            created_by=bob,
            updated_by=bob,
        )
        WorkspaceMember.objects.create(
            id=MEMBER_ROW_ID,
            workspace=workspace,
            member=alice,
            role=20,
            is_active=True,
            created_by=alice,
            updated_by=alice,
        )
        IssueLabel.objects.create(
            project=project,
            workspace=workspace,
            issue=issue,
            label=label,
            created_by=alice,
            updated_by=alice,
        )
        IssueAssignee.objects.create(
            project=project,
            workspace=workspace,
            issue=issue,
            assignee=alice,
            created_by=alice,
            updated_by=alice,
        )
        attachment = FileAsset.objects.create(
            id=ATTACHMENT_ID,
            asset="uploads/issues/gear.png",
            issue=issue,
            workspace=workspace,
            project=project,
            entity_type=FileAsset.EntityTypeContext.ISSUE_ATTACHMENT,
            attributes={"name": "gear.png", "size": 1234},
            size=1234,
            is_uploaded=True,
            storage_metadata={"ContentType": "image/png"},
            created_by=alice,
            updated_by=alice,
        )

        # Auto audit columns are set by the ORM at insert time. Overwrite them
        # with fixed values so the recorded bytes do not depend on the clock.
        for model in (
            User,
            Workspace,
            Project,
            State,
            Label,
            Cycle,
            Module,
            Issue,
            WorkspaceMember,
            IssueLabel,
            IssueAssignee,
            FileAsset,
        ):
            model.objects.filter(
                id__in=[
                    USER_ALICE,
                    USER_BOB,
                    USER_BOT,
                    USER_EMPTY,
                    WORKSPACE_ID,
                    PROJECT_ID,
                    STATE_ID,
                    STATE_TRIAGE_ID,
                    LABEL_ID,
                    CYCLE_ID,
                    MODULE_ID,
                    ISSUE_ID,
                    ISSUE_CHILD_ID,
                    MEMBER_ROW_ID,
                    ATTACHMENT_ID,
                ]
            ).update(created_at=T_CREATED, updated_at=T_UPDATED)

        # `BaseModel.save()` sets `created_by`/`updated_by` from django-crum,
        # which has no request here, so both land as NULL. Set them explicitly
        # so the goldens exercise a non-null audit user.
        for model in (
            Workspace,
            Project,
            State,
            Label,
            Cycle,
            Module,
            WorkspaceMember,
            IssueLabel,
            IssueAssignee,
            FileAsset,
        ):
            model.objects.filter(
                id__in=[
                    WORKSPACE_ID,
                    PROJECT_ID,
                    STATE_ID,
                    STATE_TRIAGE_ID,
                    LABEL_ID,
                    CYCLE_ID,
                    MODULE_ID,
                    MEMBER_ROW_ID,
                    ATTACHMENT_ID,
                ]
            ).update(created_by=alice, updated_by=alice)
        Issue.objects.filter(id=ISSUE_ID).update(created_by=alice, updated_by=bob)
        Issue.objects.filter(id=ISSUE_CHILD_ID).update(created_by=bob, updated_by=bob)

        # `date_joined` is auto_now_add, like created_at/updated_at, so the
        # value passed to create() is replaced by the clock.
        User.objects.filter(id=USER_ALICE).update(
            date_joined=T_USER_JOINED, is_email_verified=True
        )
        User.objects.filter(id__in=[USER_BOB, USER_BOT, USER_EMPTY]).update(
            date_joined=T_BOB_JOINED
        )
        User.objects.filter(id=USER_BOB).update(is_password_autoset=True)

        # WorkspaceMember's JSON columns carry large instance defaults; replace
        # them with small fixed documents so the golden is readable and the
        # Rust fixture is a short literal.
        WorkspaceMember.objects.filter(id=MEMBER_ROW_ID).update(
            view_props={"filters": {"priority": "high"}, "count": 2},
            default_props={"is_favorite": False},
            issue_props={"layout": "list"},
            getting_started_checklist={"completed": 1},
            tips={"seen": ["a", "b"]},
            explored_features={"cycles": True},
            company_role="Engineer",
        )

        # `State.save()` computes `sequence` from the highest existing value in
        # the project (+15000); force the golden values.
        State.objects.filter(id=STATE_ID).update(sequence=2.5)
        # `State.objects` excludes triage states; `all_state_objects` is the
        # manager that sees them (`plane/db/models/state.py`).
        State.all_state_objects.filter(id=STATE_TRIAGE_ID).update(sequence=0.5)

        # `Issue.save()` assigns sequence_id and sort_order from the project's
        # counters; force the golden values.
        Issue.objects.filter(id=ISSUE_ID).update(
            sequence_id=42, sort_order=65535, description_stripped="Gear"
        )
        Issue.objects.filter(id=ISSUE_CHILD_ID).update(
            sequence_id=43, sort_order=65535, description_stripped="Strut"
        )

        # Re-read so the in-memory instances carry the fixed audit columns.
        alice.refresh_from_db()
        bob.refresh_from_db()
        bot.refresh_from_db()
        empty.refresh_from_db()
        workspace.refresh_from_db()
        project.refresh_from_db()
        state.refresh_from_db()
        triage.refresh_from_db()
        label.refresh_from_db()
        cycle.refresh_from_db()
        module.refresh_from_db()
        issue.refresh_from_db()
        child.refresh_from_db()
        attachment.refresh_from_db()
        member = WorkspaceMember.objects.get(id=MEMBER_ROW_ID)

    # View annotations. `IssueSerializer` reads label_ids/assignee_ids/
    # module_ids/cycle_id/sub_issues_count/attachment_count/link_count, and the
    # views annotate exactly those (`plane/app/views/issue/base.py:533-563`).
    issue.label_ids = [LABEL_ID]
    issue.assignee_ids = [USER_ALICE]
    issue.module_ids = [MODULE_ID]
    issue.cycle_id = CYCLE_ID
    issue.sub_issues_count = 1
    issue.attachment_count = 1
    issue.link_count = 1
    issue.is_subscribed = False
    issue.is_intake = False
    issue.parent_id = None
    issue.estimate_point = None

    # Cycle / module / workspace aggregates, annotated by their views.
    cycle.is_favorite = True
    cycle.total_issues = 3
    cycle.cancelled_issues = 0
    cycle.completed_issues = 1
    cycle.started_issues = 1
    cycle.unstarted_issues = 1
    cycle.backlog_issues = 0
    cycle.status = "current"

    module.member_ids = [USER_ALICE, USER_BOB]
    module.total_estimate_points = 13.0
    module.completed_estimate_points = 5.0
    module.is_favorite = False
    module.total_issues = 4
    module.cancelled_issues = 0
    module.completed_issues = 1
    module.started_issues = 2
    module.unstarted_issues = 1
    module.backlog_issues = 0

    workspace.total_members = 3
    workspace.role = 20

    # `StateSerializer.order` is a declared FloatField over an annotation that
    # `state/list` adds *after* serialising. The plain instance has no `order`,
    # so DRF's `required=False` + missing attribute skips the key. Keep a copy
    # with the attribute for the "order present" case.
    state_with_order = State.objects.get(id=STATE_ID)
    state_with_order.order = 0.5

    return {
        "user1": alice,
        "user2": bob,
        "user_bot": bot,
        "user_empty": empty,
        "user_pair": [alice, bob],
        "workspace1": workspace,
        "project1": project,
        "state1": state,
        "state1_with_order": state_with_order,
        "state_pair": [state, triage],
        "label1": label,
        "issue1": issue,
        "issue_child": child,
        "issue_pair": [issue, child],
        "cycle1": cycle,
        "module1": module,
        "member1": member,
        "attachment1": attachment,
    }


if __name__ == "__main__":
    import os

    sys.path.insert(0, str(SERVER_DIR.parents[1] / "apps" / "api"))
    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    import django

    django.setup()
    built = build()
    print(f"fixture built: {len(built)} subjects")
