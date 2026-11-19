#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Build the Phase 8 set D task fixtures with the real Django ORM.

Hand-written INSERTs for `issues`, `pages` and `issue_activities` would have to
spell out every NOT NULL column and every model default; the ORM does it with
the pinned model definitions. This script creates a small graph in a scratch
database cloned from `plane_tpl`, then dumps the touched tables as
`INSERT ... SELECT * FROM json_populate_record(NULL::table, '<json>')`, which
round-trips every PostgreSQL type the fixture uses (the binary columns stay
NULL).

The dump is a checked-in artefact: `tools/jobs/fixtures/*.sql`. It is
deterministic — every primary key is set explicitly and every relative
timestamp is `now() - interval` — so a regenerated database has the same rows
apart from the clock values the differential harness normalises.

Usage, with the dev environment sourced like the harness does:

  PLANE_DB=t_p8d_fixture ../api/.venv/bin/python tools/jobs/make_task_fixture.py \
      --fixture cleanup --out tools/jobs/fixtures/cleanup-base.sql

The scratch database must already exist and be migrated (clone `plane_tpl`):
the script does not create it.
"""

import argparse
import os
import sys
from datetime import datetime, timedelta, timezone as dt_timezone

# Deterministic UUIDs. The first byte encodes the fixture group so a reader can
# see at a glance which fixture a row belongs to.
U = "11111111-1111-1111-1111-111111111111"
W1 = "22222222-2222-2222-2222-222222222222"
W2 = "22222222-2222-2222-2222-222222222223"
PROJECT = "33333333-3333-3333-3333-333333333333"
PROJECT2 = "33333333-3333-3333-3333-333333333334"
STATE_BACKLOG = "44444444-4444-4444-4444-444444444441"
STATE_DONE = "44444444-4444-4444-4444-444444444442"
STATE_CANCELLED = "44444444-4444-4444-4444-444444444443"
STATE_TRIAGE = "44444444-4444-4444-4444-444444444444"
ISSUE1 = "55555555-5555-5555-5555-555555555551"
ISSUE2 = "55555555-5555-5555-5555-555555555552"
ISSUE3 = "55555555-5555-5555-5555-555555555553"
INTAKE = "66666666-6666-6666-6666-666666666666"
INTAKE_ISSUE = "77777777-7777-7777-7777-777777777771"
CYCLE = "88888888-8888-8888-8888-888888888881"
MODULE = "88888888-8888-8888-8888-888888888882"
PAGE = "99999999-9999-9999-9999-999999999991"
PAGE2 = "99999999-9999-9999-9999-999999999992"
LABEL = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
VIEW = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbb1"
WEBHOOK = "cccccccc-cccc-cccc-cccc-ccccccccccc1"
ESTIMATE = "dddddddd-dddd-dddd-dddd-dddddddddd01"
ESTIMATE_POINT = "dddddddd-dddd-dddd-dddd-dddddddddd02"


def uuid_tail(index: int) -> str:
    return f"eeeeeeee-eeee-eeee-eeee-{index:012d}"


def build_cleanup(now):
    """Rows the five cleanup tasks select and must leave alone."""
    from plane.db.models import (
        APIActivityLog,
        EmailNotificationLog,
        Issue,
        IssueDescriptionVersion,
        Page,
        PageVersion,
        Project,
        State,
        Webhook,
        WebhookLog,
        Workspace,
        User,
    )

    user = User.objects.create(
        id=U,
        username="p8d",
        email="p8d@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    workspace = Workspace.objects.create(
        id=W1, name="P8D", slug="p8d-ws", owner=user, timezone="UTC"
    )
    project = Project.objects.create(
        id=PROJECT, name="P8D", identifier="P8D", workspace=workspace, created_by=user
    )
    state = State.objects.create(
        id=STATE_BACKLOG,
        name="Backlog",
        color="#ffffff",
        project=project,
        workspace=workspace,
        created_by=user,
    )
    project.default_state = state
    project.save(update_fields=["default_state"])

    issue = Issue.objects.create(
        id=ISSUE1,
        name="Cleanup",
        project=project,
        workspace=workspace,
        state=state,
        created_by=user,
    )
    page = Page.objects.create(
        id=PAGE, name="Page", workspace=workspace, owned_by=user, created_by=user
    )
    page2 = Page.objects.create(
        id=PAGE2, name="Page 2", workspace=workspace, owned_by=user, created_by=user
    )

    # 25 versions for one page (5 must go) and 3 for another (all stay).
    for index in range(25):
        PageVersion.objects.create(
            id=uuid_tail(index),
            page=page,
            workspace=workspace,
            owned_by=user,
            description_html=f"<p>v{index}</p>",
            created_by=user,
        )
    for index in range(3):
        PageVersion.objects.create(
            id=uuid_tail(100 + index),
            page=page2,
            workspace=workspace,
            owned_by=user,
            description_html=f"<p>w{index}</p>",
            created_by=user,
        )
    for model, page in ((PageVersion, page), (PageVersion, page2)):
        versions = list(model.objects.filter(page=page).order_by("id"))
        for position, version in enumerate(versions):
            model.objects.filter(id=version.id).update(
                created_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc)
                + timedelta(hours=position)
            )

    for index in range(25):
        IssueDescriptionVersion.objects.create(
            id=uuid_tail(200 + index),
            issue=issue,
            project=project,
            workspace=workspace,
            owned_by=user,
            description_html=f"<p>d{index}</p>",
            created_by=user,
        )
    for position, version in enumerate(
        IssueDescriptionVersion.objects.filter(issue=issue).order_by("id")
    ):
        IssueDescriptionVersion.objects.filter(id=version.id).update(
            created_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc)
            + timedelta(hours=position)
        )

    for index in range(5):
        APIActivityLog.objects.create(
            id=uuid_tail(300 + index),
            token_identifier="t",
            path="/api/v1/x",
            method="GET",
            response_code=200,
        )
    for index in range(3):
        APIActivityLog.objects.create(
            id=uuid_tail(310 + index),
            token_identifier="t",
            path="/api/v1/y",
            method="GET",
            response_code=200,
        )
    APIActivityLog.objects.filter(id__in=[uuid_tail(300 + i) for i in range(5)]).update(
        created_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc)
    )
    APIActivityLog.objects.filter(id__in=[uuid_tail(310 + i) for i in range(3)]).update(
        created_at=now - timedelta(days=1)
    )

    for index in range(3):
        EmailNotificationLog.objects.create(
            id=uuid_tail(400 + index),
            receiver=user,
            triggered_by=user,
            entity_name="issue",
            entity="state",
            data={},
            created_by=user,
        )
    EmailNotificationLog.objects.filter(
        id__in=[uuid_tail(400 + i) for i in range(2)]
    ).update(sent_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc))
    EmailNotificationLog.objects.filter(id=uuid_tail(402)).update(
        sent_at=now - timedelta(days=1)
    )

    webhook = Webhook.objects.create(
        id=WEBHOOK,
        workspace=workspace,
        url="https://example.com/hook",
        secret_key="secret",
        created_by=user,
    )
    for index in range(4):
        WebhookLog.objects.create(
            id=uuid_tail(500 + index),
            workspace=workspace,
            webhook=WEBHOOK,
            event_type="issue",
            request_method="POST",
            request_headers="{}",
            request_body="{}",
            response_status="200",
            response_body="{}",
            created_by=user,
        )
    WebhookLog.objects.filter(id__in=[uuid_tail(500 + i) for i in range(2)]).update(
        created_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc)
    )
    WebhookLog.objects.filter(id__in=[uuid_tail(502 + i) for i in range(2)]).update(
        created_at=now - timedelta(days=1)
    )


def build_soft_cascade(now):
    """One workspace mid-cascade and one live workspace for the direct call."""
    from plane.db.models import (
        Cycle,
        CycleIssue,
        Estimate,
        EstimatePoint,
        Intake,
        IntakeIssue,
        Issue,
        IssueActivity,
        IssueComment,
        IssueView,
        Label,
        Module,
        ModuleIssue,
        Page,
        PageVersion,
        Project,
        State,
        User,
        Workspace,
    )

    user = User.objects.create(
        id=U,
        username="p8d",
        email="p8d@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    workspace = Workspace.objects.create(
        id=W1, name="P8D", slug="p8d-cascade", owner=user, timezone="UTC"
    )
    workspace_live = Workspace.objects.create(
        id=W2, name="P8D live", slug="p8d-live", owner=user, timezone="UTC"
    )
    project = Project.objects.create(
        id=PROJECT,
        name="Cascade",
        identifier="cascade",
        workspace=workspace,
        created_by=user,
    )
    backlog = State.objects.create(
        id=STATE_BACKLOG,
        name="Backlog",
        color="#ffffff",
        project=project,
        workspace=workspace,
        created_by=user,
        default=True,
    )
    State.objects.create(
        id=STATE_CANCELLED,
        name="Cancelled",
        color="#ffffff",
        group="cancelled",
        project=project,
        workspace=workspace,
        created_by=user,
    )
    triage = State.objects.create(
        id=STATE_TRIAGE,
        name="Triage",
        color="#ffffff",
        group="triage",
        is_triage=True,
        project=project,
        workspace=workspace,
        created_by=user,
    )
    project.default_state = backlog
    project.save(update_fields=["default_state"])
    # The cascade's full save runs the model hooks; these stale values prove
    # they ran (`State.slug`, the `description_stripped` family,
    # `IssueView.query`).
    State.objects.filter(id__in=[STATE_BACKLOG, STATE_CANCELLED, STATE_TRIAGE]).update(
        slug="WRONG"
    )
    Project.objects.filter(id=PROJECT).update(identifier="cascade")

    label = Label.objects.create(
        id=LABEL, name="Bug", project=project, workspace=workspace, created_by=user
    )
    issue = Issue.objects.create(
        id=ISSUE1,
        name="Cascade",
        project=project,
        workspace=workspace,
        state=backlog,
        description_html="<p>hello</p>",
        created_by=user,
    )
    from plane.db.models import IssueLabel

    IssueLabel.objects.create(
        issue=issue, label=label, project=project, workspace=workspace, created_by=user
    )
    triage_issue = Issue.objects.create(
        id=ISSUE2,
        name="Triage issue",
        project=project,
        workspace=workspace,
        state=triage,
        created_by=user,
    )

    intake = Intake.objects.create(
        id=INTAKE, name="Intake", project=project, workspace=workspace, created_by=user
    )
    IntakeIssue.objects.create(
        id=INTAKE_ISSUE,
        intake=intake,
        issue=issue,
        duplicate_to=issue,
        status=2,
        project=project,
        workspace=workspace,
        created_by=user,
    )

    activity = IssueActivity.objects.create(
        id=uuid_tail(600),
        issue=issue,
        project=project,
        workspace=workspace,
        actor=user,
        field="state",
        verb="updated",
        created_by=user,
    )
    comment = IssueComment.objects.create(
        id=uuid_tail(601),
        issue=issue,
        project=project,
        workspace=workspace,
        actor=user,
        comment_html="<p>comment</p>",
        created_by=user,
    )

    page = Page.objects.create(
        id=PAGE,
        name="Page",
        workspace=workspace,
        owned_by=user,
        description_html="<p>page</p>",
        created_by=user,
    )
    PageVersion.objects.create(
        id=uuid_tail(602),
        page=page,
        workspace=workspace,
        owned_by=user,
        description_html="<p>version</p>",
        created_by=user,
    )

    cycle = Cycle.objects.create(
        id=CYCLE,
        name="Cycle",
        project=project,
        workspace=workspace,
        owned_by=user,
        created_by=user,
    )
    CycleIssue.objects.create(
        id=uuid_tail(603),
        cycle=cycle,
        issue=issue,
        project=project,
        workspace=workspace,
        created_by=user,
    )
    module = Module.objects.create(
        id=MODULE,
        name="Module",
        project=project,
        workspace=workspace,
        created_by=user,
    )
    ModuleIssue.objects.create(
        id=uuid_tail(604),
        module=module,
        issue=issue,
        project=project,
        workspace=workspace,
        created_by=user,
    )

    view = IssueView.objects.create(
        id=VIEW,
        name="View",
        project=project,
        workspace=workspace,
        filters={"state": ["Backlog"]},
        created_by=user,
        owned_by=user,
    )
    IssueView.objects.filter(id=VIEW).update(query={})

    estimate = Estimate.objects.create(
        id=ESTIMATE,
        name="Estimate",
        project=project,
        workspace=workspace,
        created_by=user,
    )
    point = EstimatePoint.objects.create(
        id=ESTIMATE_POINT,
        estimate=estimate,
        key=1,
        value="1",
        project=project,
        workspace=workspace,
        created_by=user,
    )
    estimate_issue = Issue.objects.create(
        id=ISSUE3,
        name="Estimated",
        project=project,
        workspace=workspace,
        state=backlog,
        estimate_point=point,
        created_by=user,
    )

    Issue.objects.filter(id=ISSUE1).update(description_stripped=None)
    Page.objects.filter(id=PAGE).update(description_stripped=None)
    PageVersion.objects.filter(id=uuid_tail(602)).update(description_stripped=None)
    IssueComment.objects.filter(id=uuid_tail(601)).update(comment_stripped="")

    # H03 has already run when the task is published: the workspace row is
    # soft-deleted, its children are not. The second workspace stays live so a
    # case can call the task directly and exercise the final self-delete.
    workspace.deleted_at = now
    workspace.save(update_fields=["deleted_at"])

    # The workspace delete's own slug rewrite is H05, not this task.
    Workspace.objects.filter(id=W1).update(slug="p8d-cascade__0")


def build_hard_delete(now):
    """One old subtree that deletes cleanly and one detached old issue whose
    activity row blocks the step, reproducing the upstream DO_NOTHING bug."""
    from plane.db.models import (
        Cycle,
        CycleIssue,
        Issue,
        IssueActivity,
        IssueComment,
        Module,
        ModuleIssue,
        Page,
        PageVersion,
        Project,
        State,
        User,
        Workspace,
    )

    user = User.objects.create(
        id=U,
        username="p8d",
        email="p8d@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    old_ws = Workspace.objects.create(
        id=W1, name="Old", slug="p8d-old", owner=user, timezone="UTC"
    )
    live_ws = Workspace.objects.create(
        id=W2, name="Live", slug="p8d-live", owner=user, timezone="UTC"
    )
    old_project = Project.objects.create(
        id=PROJECT,
        name="Old",
        identifier="old",
        workspace=old_ws,
        created_by=user,
    )
    live_project = Project.objects.create(
        id=PROJECT2,
        name="Live",
        identifier="live",
        workspace=live_ws,
        created_by=user,
    )
    old_state = State.objects.create(
        id=STATE_BACKLOG,
        name="Backlog",
        color="#ffffff",
        project=old_project,
        workspace=old_ws,
        created_by=user,
    )
    live_state = State.objects.create(
        id=STATE_DONE,
        name="Done",
        color="#ffffff",
        group="completed",
        project=live_project,
        workspace=live_ws,
        created_by=user,
    )
    old_issue = Issue.objects.create(
        id=ISSUE1,
        name="Old issue",
        project=old_project,
        workspace=old_ws,
        state=old_state,
        created_by=user,
    )
    IssueActivity.objects.create(
        id=uuid_tail(700),
        issue=old_issue,
        project=old_project,
        workspace=old_ws,
        actor=user,
        field="state",
        verb="updated",
        created_by=user,
    )
    IssueComment.objects.create(
        id=uuid_tail(701),
        issue=old_issue,
        project=old_project,
        workspace=old_ws,
        actor=user,
        comment_html="<p>c</p>",
        created_by=user,
    )
    page = Page.objects.create(
        id=PAGE,
        name="Old page",
        workspace=old_ws,
        owned_by=user,
        created_by=user,
    )
    PageVersion.objects.create(
        id=uuid_tail(702),
        page=page,
        workspace=old_ws,
        owned_by=user,
        created_by=user,
    )
    cycle = Cycle.objects.create(
        id=CYCLE,
        name="Old cycle",
        project=old_project,
        workspace=old_ws,
        owned_by=user,
        created_by=user,
    )
    CycleIssue.objects.create(
        id=uuid_tail(703),
        cycle=cycle,
        issue=old_issue,
        project=old_project,
        workspace=old_ws,
        created_by=user,
    )
    module = Module.objects.create(
        id=MODULE, name="Old module", project=old_project, workspace=old_ws, created_by=user
    )
    ModuleIssue.objects.create(
        id=uuid_tail(704),
        module=module,
        issue=old_issue,
        project=old_project,
        workspace=old_ws,
        created_by=user,
    )

    # The orphan: its project and workspace stay live, so only step 5 (`Issue`)
    # ever touches it. Its activity row is DO_NOTHING and stays.
    orphan = Issue.objects.create(
        id=ISSUE2,
        name="Orphan",
        project=live_project,
        workspace=live_ws,
        state=live_state,
        created_by=user,
    )
    IssueActivity.objects.create(
        id=uuid_tail(705),
        issue=orphan,
        project=live_project,
        workspace=live_ws,
        actor=user,
        field="state",
        verb="updated",
        created_by=user,
    )

    old_ws.deleted_at = datetime(2026, 1, 1, tzinfo=dt_timezone.utc)
    old_ws.save(update_fields=["deleted_at"])
    Issue.objects.filter(id=ISSUE2).update(deleted_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc))
    del now


def build_automation(now):
    """Archive and close cases, including the join-multiplying intake rows."""
    from plane.db.models import (
        Cycle,
        CycleIssue,
        Intake,
        IntakeIssue,
        Issue,
        Module,
        ModuleIssue,
        Project,
        State,
        User,
        Workspace,
    )

    user = User.objects.create(
        id=U,
        username="p8d",
        email="p8d@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    workspace = Workspace.objects.create(
        id=W1, name="P8D", slug="p8d-automation", owner=user, timezone="UTC"
    )
    project = Project.objects.create(
        id=PROJECT,
        name="Automation",
        identifier="AUTO",
        workspace=workspace,
        created_by=user,
        archive_in=1,
        close_in=1,
    )
    backlog = State.objects.create(
        id=STATE_BACKLOG,
        name="Backlog",
        color="#ffffff",
        project=project,
        workspace=workspace,
        created_by=user,
        default=True,
    )
    done = State.objects.create(
        id=STATE_DONE,
        name="Done",
        color="#ffffff",
        group="completed",
        project=project,
        workspace=workspace,
        created_by=user,
    )
    State.objects.create(
        id=STATE_CANCELLED,
        name="Cancelled",
        color="#ffffff",
        group="cancelled",
        sequence=55000,
        project=project,
        workspace=workspace,
        created_by=user,
    )
    intake = Intake.objects.create(
        id=INTAKE, name="Intake", project=project, workspace=workspace, created_by=user
    )

    def issue(suffix, state, **kwargs):
        return Issue.objects.create(
            id=f"55555555-5555-5555-5555-{suffix:012d}",
            name=f"Automation {suffix}",
            project=project,
            workspace=workspace,
            state=state,
            created_by=user,
            **kwargs,
        )

    archived = issue(1, done)
    closed = issue(2, backlog)
    in_cycle = issue(3, done)
    in_module = issue(4, done)
    pending_intake = issue(5, done)
    recent = issue(6, done)
    already_archived = issue(7, done, archived_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc).date())
    duplicated = issue(8, done)

    cycle = Cycle.objects.create(
        id=CYCLE,
        name="Open",
        project=project,
        workspace=workspace,
        owned_by=user,
        end_date="2030-01-01",
        created_by=user,
    )
    CycleIssue.objects.create(
        id=uuid_tail(800),
        cycle=cycle,
        issue=in_cycle,
        project=project,
        workspace=workspace,
        created_by=user,
    )
    closed_cycle = Cycle.objects.create(
        id=uuid_tail(801),
        name="Closed",
        project=project,
        workspace=workspace,
        owned_by=user,
        end_date="2020-01-01",
        created_by=user,
    )
    CycleIssue.objects.create(
        id=uuid_tail(802),
        cycle=closed_cycle,
        issue=archived,
        project=project,
        workspace=workspace,
        created_by=user,
    )
    module = Module.objects.create(
        id=MODULE,
        name="Open module",
        project=project,
        workspace=workspace,
        target_date="2030-01-01",
        created_by=user,
    )
    ModuleIssue.objects.create(
        id=uuid_tail(803),
        module=module,
        issue=in_module,
        project=project,
        workspace=workspace,
        created_by=user,
    )
    IntakeIssue.objects.create(
        id=uuid_tail(804),
        intake=intake,
        issue=pending_intake,
        status=-2,
        project=project,
        workspace=workspace,
        created_by=user,
    )
    # Two accepted rows on one issue: the LEFT JOIN returns the issue twice,
    # so the activity is published twice. That is the upstream behaviour.
    for index, status in enumerate((1, 2)):
        IntakeIssue.objects.create(
            id=uuid_tail(810 + index),
            intake=intake,
            issue=duplicated,
            status=status,
            project=project,
            workspace=workspace,
            created_by=user,
        )
    # Older than `archive_in/close_in * 30` days; `recent` is inside the window.
    Issue.objects.filter(
        id__in=[archived.id, closed.id, in_cycle.id, in_module.id, pending_intake.id,
                already_archived.id, duplicated.id]
    ).update(updated_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc))
    Issue.objects.filter(id=recent.id).update(updated_at=now - timedelta(days=1))


def build_unsnooze(now):
    """Expired, future and non-snoozed intake rows."""
    from plane.db.models import Intake, IntakeIssue, Issue, Project, State, User, Workspace

    user = User.objects.create(
        id=U,
        username="p8d",
        email="p8d@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    workspace = Workspace.objects.create(
        id=W1, name="P8D", slug="p8d-unsnooze", owner=user, timezone="UTC"
    )
    project = Project.objects.create(
        id=PROJECT,
        name="Unsnooze",
        identifier="SNZ",
        workspace=workspace,
        created_by=user,
    )
    state = State.objects.create(
        id=STATE_BACKLOG,
        name="Backlog",
        color="#ffffff",
        project=project,
        workspace=workspace,
        created_by=user,
    )
    intake = Intake.objects.create(
        id=INTAKE, name="Intake", project=project, workspace=workspace, created_by=user
    )
    for index in range(4):
        issue = Issue.objects.create(
            id=f"55555555-5555-5555-5555-{index:012d}",
            name=f"Snooze {index}",
            project=project,
            workspace=workspace,
            state=state,
            created_by=user,
        )
        IntakeIssue.objects.create(
            id=f"77777777-7777-7777-7777-{index:012d}",
            intake=intake,
            issue=issue,
            status=0 if index < 3 else 1,
            snoozed_till=now - timedelta(hours=index + 1)
            if index < 2
            else now + timedelta(days=1),
            project=project,
            workspace=workspace,
            created_by=user,
        )
    del now


BUILDERS = {
    "cleanup": build_cleanup,
    "soft-cascade": build_soft_cascade,
    "hard-delete": build_hard_delete,
    "automation": build_automation,
    "unsnooze": build_unsnooze,
}


def dump_tables(tables):
    """`INSERT ... json_populate_record` for every row of `tables`."""
    import psycopg
    from django.db import connection

    chunks = []
    with connection.cursor() as cursor:
        for table in tables:
            cursor.execute(f"SELECT to_jsonb(t)::text FROM {table} t ORDER BY t::text")
            for (row,) in cursor.fetchall():
                chunks.append(
                    f"INSERT INTO {table} SELECT * FROM json_populate_record(NULL::{table}, $json${row}$json$);"
                )
    return chunks


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--fixture", required=True, choices=sorted(BUILDERS))
    parser.add_argument("--out", required=True)
    args = parser.parse_args()

    os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
    import django

    django.setup()

    from django.db import connection

    from django.utils import timezone

    BUILDERS[args.fixture](timezone.now())

    tables = TOUCHED[args.fixture]
    header = (
        "-- Copyright (c) 2023-present Plane Software, Inc. and contributors\n"
        "-- SPDX-License-Identifier: AGPL-3.0-only\n"
        "-- See the LICENSE file for details.\n"
        "--\n"
        f"-- Generated by tools/jobs/make_task_fixture.py --fixture {args.fixture}.\n"
        "-- Deterministic ids; timestamps are relative to the run.\n\n"
    )
    chunks = dump_tables(tables)
    for table, record_id, column, expression in RELATIVE.get(args.fixture, []):
        chunks.append(
            f"UPDATE {table} SET {column} = {expression} WHERE id = '{record_id}';"
        )
    with open(args.out, "w") as handle:
        handle.write(header)
        handle.write("\n".join(chunks))
        handle.write("\n")
    print(f"wrote {args.out}")
    return 0


# Values that must be relative to the **run**, not to generation: a fixture
# checked in once must keep selecting the same rows months later.
RELATIVE = {
    "cleanup": [
        ("api_activity_logs", uuid_tail(310 + index), "created_at",
         "now() - interval '1 day'")
        for index in range(3)
    ]
    + [
        ("email_notification_logs", uuid_tail(402), "sent_at",
         "now() - interval '1 day'"),
    ]
    + [
        ("webhook_logs", uuid_tail(502 + index), "created_at",
         "now() - interval '1 day'")
        for index in range(2)
    ],
    "soft-cascade": [
        ("workspaces", W1, "deleted_at", "now() - interval '1 hour'"),
    ],
    "automation": [
        ("issues", f"55555555-5555-5555-5555-{6:012d}", "updated_at",
         "now() - interval '1 day'"),
    ],
}


# Tables each fixture writes, in foreign-key order.
TOUCHED = {
    "cleanup": [
        "users",
        "workspaces",
        "projects",
        "states",
        "issues",
        "issue_sequences",
        "pages",
        "page_versions",
        "issue_description_versions",
        "api_activity_logs",
        "email_notification_logs",
        "webhooks",
        "webhook_logs",
    ],
    "soft-cascade": [
        "users",
        "workspaces",
        "projects",
        "states",
        "labels",
        "issues",
        "issue_sequences",
        "issue_labels",
        "intakes",
        "intake_issues",
        "issue_activities",
        "issue_comments",
        "descriptions",
        "pages",
        "page_versions",
        "cycles",
        "cycle_issues",
        "modules",
        "module_issues",
        "issue_views",
        "estimates",
        "estimate_points",
    ],
    "hard-delete": [
        "users",
        "workspaces",
        "projects",
        "states",
        "issues",
        "issue_sequences",
        "issue_activities",
        "issue_comments",
        "descriptions",
        "pages",
        "page_versions",
        "cycles",
        "cycle_issues",
        "modules",
        "module_issues",
    ],
    "automation": [
        "users",
        "workspaces",
        "projects",
        "states",
        "intakes",
        "issues",
        "issue_sequences",
        "cycles",
        "cycle_issues",
        "modules",
        "module_issues",
        "intake_issues",
    ],
    "unsnooze": [
        "users",
        "workspaces",
        "projects",
        "states",
        "intakes",
        "issues",
        "issue_sequences",
        "intake_issues",
    ],
}


if __name__ == "__main__":
    sys.exit(main())
