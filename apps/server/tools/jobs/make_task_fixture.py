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
U2 = "11111111-1111-1111-1111-111111111112"
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
CYCLE2 = "88888888-8888-8888-8888-888888888883"
MODULE = "88888888-8888-8888-8888-888888888882"
MODULE2 = "88888888-8888-8888-8888-888888888884"
PAGE = "99999999-9999-9999-9999-999999999991"
PAGE2 = "99999999-9999-9999-9999-999999999992"
LABEL = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa1"
LABEL2 = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaa2"
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


def build_description_versions(now):
    """Issues and versions for every `issue_description_version_task` branch."""
    from plane.db.models import (
        Issue,
        IssueDescriptionVersion,
        Project,
        State,
        User,
        Workspace,
    )

    user = User.objects.create(
        id=U,
        username="p8a3",
        email="p8a3@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    other = User.objects.create(
        id=U2,
        username="p8a3-other",
        email="p8a3-other@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    workspace = Workspace.objects.create(
        id=W1, name="P8A3", slug="p8a3-description", owner=user, timezone="UTC"
    )
    project = Project.objects.create(
        id=PROJECT,
        name="P8A3",
        identifier="P8A3",
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

    def issue(index, html, **kwargs):
        return Issue.objects.create(
            id=f"55555555-5555-5555-5555-{index:012d}",
            name=f"Description {index}",
            project=project,
            workspace=workspace,
            state=state,
            description_html=html,
            description_json={"type": "doc", "index": index},
            created_by=user,
            **kwargs,
        )

    # A: no version at all (create branch).
    issue(1, "<p>a</p>")
    # B: same owner, inside the 600 s window (update branch).
    issue(2, "<p>b</p>")
    # C: same owner, outside the window (create branch).
    issue(3, "<p>c</p>")
    # D: another owner, inside the window (create branch); its
    # `description_binary` proves the bytea copy on create.
    issue(4, "<p>d</p>", description_binary=b"\x01\x02")
    # E: the argument's description_html equals the row (no-op branch).
    issue(5, "<p>unchanged</p>")
    # F: soft-deleted, so `Issue.objects.get` misses it.
    issue(6, "<p>f</p>", deleted_at=now)

    def version(record_id, issue_row, owner, html, **kwargs):
        row = IssueDescriptionVersion.objects.create(
            id=record_id,
            issue=issue_row,
            project=project,
            workspace=workspace,
            owned_by=owner,
            description_html=html,
            description_json={"version": record_id},
            created_by=user,
            **kwargs,
        )
        IssueDescriptionVersion.objects.filter(id=row.id).update(created_by_id=user.id)
        return row

    version(uuid_tail(1), Issue.objects.get(
        id="55555555-5555-5555-5555-000000000002"
    ), user, "<p>old-b</p>")
    version(uuid_tail(2), Issue.objects.get(
        id="55555555-5555-5555-5555-000000000003"
    ), user, "<p>old-c</p>")
    version(uuid_tail(3), Issue.objects.get(
        id="55555555-5555-5555-5555-000000000004"
    ), other, "<p>old-d</p>")
    version(uuid_tail(4), Issue.objects.get(
        id="55555555-5555-5555-5555-000000000005"
    ), user, "<p>unchanged</p>")
    version(uuid_tail(5), Issue.all_objects.get(
        id="55555555-5555-5555-5555-000000000006"
    ), user, "<p>f</p>")

    # The window is measured against the run, not against generation time.
    # B and D are 300 s old (inside), C is an hour old (outside), E is inside
    # and its issue text matches.
    for index in (1, 3, 4):
        IssueDescriptionVersion.objects.filter(id=uuid_tail(index)).update(
            last_saved_at=now - timedelta(seconds=300)
        )
    IssueDescriptionVersion.objects.filter(id=uuid_tail(2)).update(
        last_saved_at=now - timedelta(hours=1)
    )


def build_page_versions(now):
    """Pages for every `track_page_version` branch (all no-ops upstream)."""
    from plane.db.models import Page, PageVersion, User, Workspace

    user = User.objects.create(
        id=U,
        username="p8a3",
        email="p8a3@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    workspace = Workspace.objects.create(
        id=W1, name="P8A3", slug="p8a3-page", owner=user, timezone="UTC"
    )
    changed = Page.objects.create(
        id=PAGE,
        name="Changed",
        workspace=workspace,
        owned_by=user,
        description_html="<p>changed</p>",
        created_by=user,
    )
    same = Page.objects.create(
        id=PAGE2,
        name="Same",
        workspace=workspace,
        owned_by=user,
        description_html="<p>same</p>",
        created_by=user,
    )
    many = Page.objects.create(
        id="99999999-9999-9999-9999-999999999993",
        name="Many",
        workspace=workspace,
        owned_by=user,
        description_html="<p>many</p>",
        created_by=user,
    )
    deleted = Page.objects.create(
        id="99999999-9999-9999-9999-999999999994",
        name="Deleted",
        workspace=workspace,
        owned_by=user,
        description_html="<p>deleted</p>",
        created_by=user,
        deleted_at=now,
    )

    def version(record_id, page, html, index):
        row = PageVersion.objects.create(
            id=record_id,
            page=page,
            workspace=workspace,
            owned_by=user,
            description_html=html,
            description_json={"index": index},
            created_by=user,
        )
        PageVersion.objects.filter(id=row.id).update(
            created_by_id=user.id,
            created_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc)
            + timedelta(hours=index),
        )
        return row

    version(uuid_tail(10), same, "<p>same</p>", 0)
    for index in range(25):
        version(uuid_tail(100 + index), many, f"<p>v{index}</p>", index)
    version(uuid_tail(11), deleted, "<p>deleted</p>", 0)
    PageVersion.objects.filter(id__in=[uuid_tail(100 + index) for index in range(25)]).update(
        last_saved_at=now - timedelta(hours=1)
    )


def build_page_transactions(now):
    """Pages and `PageLog` rows for `page_transaction`."""
    from plane.db.models import Page, PageLog, User, Workspace

    user = User.objects.create(
        id=U,
        username="p8a3",
        email="p8a3@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    workspace = Workspace.objects.create(
        id=W1, name="P8A3", slug="p8a3-transaction", owner=user, timezone="UTC"
    )
    page = Page.objects.create(
        id=PAGE,
        name="One",
        workspace=workspace,
        owned_by=user,
        created_by=user,
    )
    other = Page.objects.create(
        id=PAGE2,
        name="Two",
        workspace=workspace,
        owned_by=user,
        created_by=user,
    )
    empty = Page.objects.create(
        id="99999999-9999-9999-9999-999999999993",
        name="Empty",
        workspace=workspace,
        owned_by=user,
        created_by=user,
    )
    deleted = Page.objects.create(
        id="99999999-9999-9999-9999-999999999994",
        name="Deleted",
        workspace=workspace,
        owned_by=user,
        created_by=user,
        deleted_at=now,
    )

    def log(page_row, index, transaction=None, deleted_at=None):
        row = PageLog.objects.create(
            id=uuid_tail(700 + index),
            transaction=uuid_tail(800 + (index if transaction is None else transaction)),
            page=page_row,
            workspace=workspace,
            entity_identifier=uuid_tail(900 + index),
            entity_name="user",
            created_by=user,
            deleted_at=deleted_at,
        )
        PageLog.objects.filter(id=row.id).update(created_by_id=user.id)
        return row

    # T1 (800…01): live on page 1, removed by the "remove" case.
    log(page, 1)
    # T2 (800…02): soft-deleted, so the manager never sees it.
    log(page, 2, deleted_at=now)
    # T3 (800…03): live on page 2 only.
    log(other, 3)
    # T4 (800…04): live on both pages; removing it on page 1 deletes both,
    # because the filter has no page_id.
    log(page, 4)
    log(other, 6, transaction=4)
    # T5 (800…05): already logged; an old and new HTML that both contain it
    # must not insert a second row (unique (page, transaction)).
    log(page, 5)


def build_backfill(now):
    """Issues for the two `plane-admin` version backfills."""
    from plane.db.models import (
        Cycle,
        CycleIssue,
        Issue,
        IssueActivity,
        IssueAssignee,
        IssueLabel,
        Label,
        Module,
        ModuleIssue,
        Project,
        ProjectMember,
        State,
        User,
        Workspace,
    )

    user = User.objects.create(
        id=U,
        username="p8a3",
        email="p8a3@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    admin = User.objects.create(
        id=U2,
        username="p8a3-admin",
        email="p8a3-admin@example.com",
        first_name="",
        last_name="",
        user_timezone="UTC",
    )
    workspace = Workspace.objects.create(
        id=W1, name="P8A3", slug="p8a3-backfill", owner=user, timezone="UTC"
    )
    project = Project.objects.create(
        id=PROJECT,
        name="Backfill",
        identifier="BF",
        workspace=workspace,
        created_by=user,
    )
    empty_project = Project.objects.create(
        id=PROJECT2,
        name="No admin",
        identifier="BF2",
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
    empty_state = State.objects.create(
        id=STATE_DONE,
        name="Done",
        color="#ffffff",
        project=empty_project,
        workspace=workspace,
        created_by=user,
    )
    # One project admin (role 20) and one ordinary member, both live. The
    # fallback must pick `admin` even though `user` is older.
    ProjectMember.objects.create(
        id=uuid_tail(1), project=project, workspace=workspace, member=user, role=5,
        created_by=user,
    )
    ProjectMember.objects.create(
        id=uuid_tail(2), project=project, workspace=workspace, member=admin, role=20,
        created_by=user,
    )

    label1 = Label.objects.create(
        id=LABEL, name="Bug", project=project, workspace=workspace, created_by=user
    )
    label2 = Label.objects.create(
        id=LABEL2, name="Feature", project=project, workspace=workspace, created_by=user
    )
    module1 = Module.objects.create(
        id=MODULE, name="M1", project=project, workspace=workspace, created_by=user
    )
    module2 = Module.objects.create(
        id=MODULE2, name="M2", project=project, workspace=workspace, created_by=user
    )
    cycle1 = Cycle.objects.create(
        id=CYCLE, name="C1", project=project, workspace=workspace, owned_by=user,
        created_by=user,
    )
    cycle2 = Cycle.objects.create(
        id=CYCLE2, name="C2", project=project, workspace=workspace, owned_by=user,
        created_by=user,
    )

    def issue(index, project_row, state, **kwargs):
        return Issue.objects.create(
            id=f"55555555-5555-5555-5555-{index:012d}",
            name=f"Backfill {index}",
            project=project_row,
            workspace=workspace,
            state=state,
            description_html=f"<p>d{index}</p>",
            description_json={"index": index},
            description_stripped=f"d{index}",
            sequence_id=index,
            sort_order=100.0 + index,
            priority="high" if index % 2 else "none",
            **kwargs,
        )

    # `Issue.objects.create` goes through `BaseModel.save`, which nulls the
    # audit users outside a request, so the columns are set afterwards.
    # 1: updated_by wins, cycle links (two -> the oldest wins), two
    # assignees, one label, two modules, two activities (the newest wins).
    one = issue(1, project, backlog)
    issue(2, project, backlog)
    issue(3, project, backlog)
    issue(4, project, backlog, deleted_at=now)
    issue(5, empty_project, empty_state)
    issue(6, project, triage)
    issue(7, project, backlog, start_date="2026-02-01", target_date="2026-03-01",
          completed_at=datetime(2026, 1, 15, tzinfo=dt_timezone.utc),
          archived_at=datetime(2026, 1, 20, tzinfo=dt_timezone.utc).date(),
          external_source="github", external_id="ext-7", is_draft=False)

    for index, cycle in ((0, cycle1), (1, cycle2)):
        CycleIssue.objects.create(
            id=uuid_tail(10 + index), cycle=cycle, issue=one, project=project,
            workspace=workspace, created_by=user,
        )
    CycleIssue.objects.filter(id=uuid_tail(10)).update(
        created_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc)
    )
    CycleIssue.objects.filter(id=uuid_tail(11)).update(
        created_at=datetime(2026, 1, 2, tzinfo=dt_timezone.utc)
    )
    for index, assignee in enumerate((U, U2)):
        IssueAssignee.objects.create(
            id=uuid_tail(20 + index), issue=one, assignee_id=assignee, project=project,
            workspace=workspace, created_by=user,
        )
    IssueLabel.objects.create(
        id=uuid_tail(30), issue=one, label=label1, project=project, workspace=workspace,
        created_by=user,
    )
    IssueLabel.objects.create(
        id=uuid_tail(31),
        issue=Issue.objects.get(id="55555555-5555-5555-5555-000000000002"),
        label=label2,
        project=project,
        workspace=workspace,
        created_by=user,
    )
    for index, module in enumerate((module1, module2)):
        ModuleIssue.objects.create(
            id=uuid_tail(40 + index), module=module, issue=one, project=project,
            workspace=workspace, created_by=user,
        )
    for index in range(2):
        IssueActivity.objects.create(
            id=uuid_tail(50 + index), issue=one, project=project, workspace=workspace,
            actor=user, field="state", verb="updated", created_by=user,
        )
        IssueActivity.objects.filter(id=uuid_tail(50 + index)).update(
            created_at=datetime(2026, 1, 1 + index, tzinfo=dt_timezone.utc)
        )
    # Audit users, in the combinations `get_owner_id` distinguishes.
    Issue.objects.filter(id="55555555-5555-5555-5555-000000000001").update(
        created_by_id=U, updated_by_id=U2
    )
    # 2: no audit users at all -> the project admin (U2) wins.
    Issue.objects.filter(id="55555555-5555-5555-5555-000000000003").update(
        created_by_id=U2, updated_by_id=None
    )
    Issue.objects.filter(
        id__in=[f"55555555-5555-5555-5555-{index:012d}" for index in (4, 6, 7)]
    ).update(created_by_id=U)
    # 5: no audit users and no project admin -> skipped by the backfill.

    # Batch order is `created_at ASC`; make it explicit and independent of
    # insertion time.
    for index in range(1, 8):
        Issue.objects.filter(id=f"55555555-5555-5555-5555-{index:012d}").update(
            created_at=datetime(2026, 1, 1, tzinfo=dt_timezone.utc)
            + timedelta(hours=index)
        )


BUILDERS = {
    "cleanup": build_cleanup,
    "soft-cascade": build_soft_cascade,
    "hard-delete": build_hard_delete,
    "automation": build_automation,
    "unsnooze": build_unsnooze,
    "description-version": build_description_versions,
    "page-version": build_page_versions,
    "page-transaction": build_page_transactions,
    "backfill": build_backfill,
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
    "description-version": [
        ("issue_description_versions", uuid_tail(1), "last_saved_at",
         "now() - interval '5 minutes'"),
        ("issue_description_versions", uuid_tail(3), "last_saved_at",
         "now() - interval '5 minutes'"),
        ("issue_description_versions", uuid_tail(4), "last_saved_at",
         "now() - interval '5 minutes'"),
        ("issue_description_versions", uuid_tail(2), "last_saved_at",
         "now() - interval '1 hour'"),
        ("issues", f"55555555-5555-5555-5555-{6:012d}", "deleted_at", "now()"),
    ],
    "page-version": [
        ("pages", "99999999-9999-9999-9999-999999999994", "deleted_at", "now()"),
    ],
    "page-transaction": [
        ("pages", "99999999-9999-9999-9999-999999999994", "deleted_at", "now()"),
        ("page_logs", uuid_tail(702), "deleted_at", "now()"),
    ],
    "backfill": [
        ("issues", f"55555555-5555-5555-5555-{4:012d}", "deleted_at", "now()"),
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
    "description-version": [
        "users",
        "workspaces",
        "projects",
        "states",
        "issues",
        "issue_sequences",
        "issue_description_versions",
    ],
    "page-version": [
        "users",
        "workspaces",
        "pages",
        "page_versions",
    ],
    "page-transaction": [
        "users",
        "workspaces",
        "pages",
        "page_logs",
    ],
    "backfill": [
        "users",
        "workspaces",
        "projects",
        "project_members",
        "states",
        "labels",
        "cycles",
        "modules",
        "issues",
        "issue_sequences",
        "cycle_issues",
        "module_issues",
        "issue_assignees",
        "issue_labels",
        "issue_activities",
    ],
}


if __name__ == "__main__":
    sys.exit(main())
