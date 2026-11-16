# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.
"""Build the deterministic database the `plane-query` goldens are computed on.

    apps/server/dev/mkdb.sh t_query_fixture
    apps/server/tools/query/run.sh apps/server/tools/query/build_fixture.py

The fixture contains one workspace with three projects (a main project with 300
issues, a second project with 60, an archived project), an unrelated second
workspace, four users with different timezones, and issues that cover every
filterable attribute: null states, triage states, archived, draft and
soft-deleted issues, sub-issues, 0..4 labels and 0..3 assignees per issue,
cycles, modules (including an archived module), intake rows with every status,
mentions, subscribers (including soft-deleted rows), links, attachments, and
dates spread over a year.

Every id is ``uuid5`` of a readable name (:mod:`fixture_spec`) and every
timestamp derives from :data:`fixture_spec.FIXED_NOW`, so two builds produce
byte-identical data and the committed goldens stay valid.

The script refuses to run on a database that already holds the fixture: clone a
fresh database instead (``mkdb.sh <name> --replace``).
"""

import json
import random
import sys
from datetime import date, timedelta
from pathlib import Path

import django

sys.path.insert(0, str(Path(__file__).resolve().parent))

import fixture_spec as spec  # noqa: E402

django.setup()

from django.db import transaction  # noqa: E402
from django.utils import timezone as dj_timezone  # noqa: E402

from plane.db.models import (  # noqa: E402
    Cycle,
    CycleIssue,
    Estimate,
    EstimatePoint,
    FileAsset,
    Intake,
    IntakeIssue,
    Issue,
    IssueAssignee,
    IssueLabel,
    IssueLink,
    IssueMention,
    IssueSubscriber,
    Label,
    Module,
    ModuleIssue,
    Project,
    ProjectMember,
    State,
    User,
    Workspace,
    WorkspaceMember,
)

SECONDS = timedelta(seconds=1)


def die(message: str) -> None:
    print(f"build_fixture: {message}", file=sys.stderr)
    raise SystemExit(1)


def create_users() -> dict[str, User]:
    users = {}
    for name, (user_id, tz) in spec.USERS.items():
        users[name] = User.objects.create(
            id=user_id,
            username=f"fixture-{name}",
            email=f"{name}@query-fixture.test",
            display_name=f"Fixture {name.title()}",
            first_name=name.title(),
            last_name="Tester",
            user_timezone=tz,
            is_active=True,
            is_email_verified=True,
        )
    return users


def create_workspaces(users: dict[str, User]) -> tuple[Workspace, Workspace]:
    workspace = Workspace.objects.create(
        id=spec.WORKSPACE_ID,
        name="Query Fixture",
        slug=spec.WORKSPACE_SLUG,
        owner=users["alice"],
        timezone="UTC",
    )
    other = Workspace.objects.create(
        id=spec.OTHER_WORKSPACE_ID,
        name="Query Fixture Other",
        slug=spec.OTHER_WORKSPACE_SLUG,
        owner=users["dave"],
        timezone="Europe/Berlin",
    )
    for workspace_row, members in ((workspace, ("alice", "bob", "carol")), (other, ("dave",))):
        for index, name in enumerate(members):
            WorkspaceMember.objects.create(
                id=spec.uid(f"workspace-member:{workspace_row.slug}:{name}"),
                workspace=workspace_row,
                member=users[name],
                role=20 if index == 0 else 15,
            )
    return workspace, other


def create_projects(workspace: Workspace, other: Workspace, users: dict[str, User]) -> dict[str, Project]:
    projects = {
        "main": Project.objects.create(
            id=spec.PROJECT_ID,
            name="Query Fixture Project",
            identifier=spec.PROJECT_IDENTIFIER,
            workspace=workspace,
            timezone="Asia/Tokyo",
            network=2,
        ),
        "second": Project.objects.create(
            id=spec.PROJECT2_ID,
            name="Query Fixture Project Two",
            identifier=spec.PROJECT2_IDENTIFIER,
            workspace=workspace,
            timezone="UTC",
            network=2,
        ),
        "archived": Project.objects.create(
            id=spec.ARCHIVED_PROJECT_ID,
            name="Query Fixture Archived Project",
            identifier="QFIXA",
            workspace=workspace,
            timezone="UTC",
            archived_at=spec.FIXED_NOW - timedelta(days=30),
        ),
        "other": Project.objects.create(
            id=spec.OTHER_PROJECT_ID,
            name="Other Workspace Project",
            identifier="QOTH",
            workspace=other,
            timezone="Europe/Berlin",
        ),
    }
    # alice is ADMIN everywhere in the main workspace, bob MEMBER, carol GUEST.
    roles = {"alice": 20, "bob": 15, "carol": 5}
    for name, project in projects.items():
        members = ("dave",) if name == "other" else tuple(roles)
        for index, member in enumerate(members):
            ProjectMember.objects.create(
                id=spec.uid(f"project-member:{name}:{member}"),
                project=project,
                workspace=project.workspace,
                member=users[member],
                role=roles.get(member, 20 if index == 0 else 15),
            )
    return projects


def create_states(projects: dict[str, Project]) -> dict[str, dict[str, State]]:
    states: dict[str, dict[str, State]] = {}
    for project_name, rows, project_key in (
        ("main", spec.PROJECT_STATES, "main"),
        ("second", spec.PROJECT2_STATES, "second"),
    ):
        created = {}
        for index, (name, group, is_triage, is_deleted) in enumerate(rows):
            state = State.objects.create(
                id=spec.state_id(project_key, name),
                project=projects[project_name],
                workspace=projects[project_name].workspace,
                name=name,
                group=group,
                color="#aabbcc",
                sequence=(index + 1) * 15000.0,
                is_triage=is_triage,
                default=name in ("backlog", "p2-backlog"),
                deleted_at=(spec.FIXED_NOW if is_deleted else None),
            )
            created[name] = state
        states[project_key] = created
    return states


def create_labels(projects: dict[str, Project]) -> dict[str, Label]:
    labels = {}
    for name in spec.LABELS:
        if name == "other-workspace-label":
            project = projects["other"]
        elif name.startswith("p2-"):
            project = projects["second"]
        elif name == "workspace-label":
            project = None
        else:
            project = projects["main"]
        labels[name] = Label.objects.create(
            id=spec.label_id(name),
            name=name,
            project=project,
            workspace=project.workspace if project else projects["main"].workspace,
            color="#123456",
            sort_order=65535,
            deleted_at=(spec.FIXED_NOW if name == "deleted-label" else None),
        )
    return labels


def create_modules(projects: dict[str, Project]) -> dict[str, Module]:
    modules = {}
    for name in spec.MODULES:
        project = projects["second"] if name.startswith("p2-") else projects["main"]
        modules[name] = Module.objects.create(
            id=spec.module_id(name),
            project=project,
            workspace=project.workspace,
            name=name,
            status="planned",
            start_date=(spec.FIXED_NOW - timedelta(days=60)).date(),
            target_date=(spec.FIXED_NOW + timedelta(days=60)).date(),
            archived_at=(spec.FIXED_NOW if name == "module-archived" else None),
            deleted_at=(spec.FIXED_NOW if name == "module-deleted" else None),
        )
    return modules


def create_cycles(projects: dict[str, Project], users: dict[str, User]) -> dict[str, Cycle]:
    cycles = {}
    for name in spec.CYCLES:
        project = projects["second"] if name.startswith("p2-") else projects["main"]
        cycles[name] = Cycle.objects.create(
            id=spec.cycle_id(name),
            project=project,
            workspace=project.workspace,
            name=name,
            owned_by=users["alice"],
            start_date=spec.FIXED_NOW - timedelta(days=30),
            end_date=spec.FIXED_NOW + timedelta(days=30),
            deleted_at=(spec.FIXED_NOW if name == "cycle-deleted" else None),
        )
    return cycles


def create_estimate(project: Project) -> list[EstimatePoint]:
    estimate = Estimate.objects.create(
        id=spec.uid("estimate"),
        project=project,
        workspace=project.workspace,
        name="Fixture Estimate",
        type="points",
        last_used=True,
    )
    return [
        EstimatePoint.objects.create(
            id=spec.estimate_point_id(key),
            project=project,
            workspace=project.workspace,
            estimate=estimate,
            key=key,
            description=f"Point {key}",
            value=str(key),
        )
        for key in (1, 2, 3)
    ]


def create_issue_relations(issue, index: int, rng: random.Random, ctx) -> None:
    """Labels, assignees, modules, cycles and intake rows for one issue."""
    project_name = "second" if issue.project_id == ctx["projects"]["second"].id else "main"
    label_names = [name for name in spec.LABELS if not name.startswith("other-")]
    if project_name == "second":
        candidates = [name for name in label_names if name.startswith("p2-")]
    else:
        candidates = [name for name in label_names if not name.startswith("p2-")]

    for name in rng.sample(candidates, min(len(candidates), index % 5)):
        IssueLabel.objects.create(
            id=spec.uid(f"issue-label:{issue.id}:{name}"),
            issue=issue,
            label=ctx["labels"][name],
            project=issue.project,
            workspace=issue.workspace,
        )
    if index % 11 == 0:
        # A soft-deleted label link, which every filter must ignore.
        IssueLabel.objects.create(
            id=spec.uid(f"issue-label-deleted:{issue.id}"),
            issue=issue,
            label=ctx["labels"]["alpha-label"],
            project=issue.project,
            workspace=issue.workspace,
            deleted_at=spec.FIXED_NOW,
        )

    member_ids = ["alice", "bob", "carol"]
    for name in rng.sample(member_ids, index % 4):
        IssueAssignee.objects.create(
            id=spec.uid(f"issue-assignee:{issue.id}:{name}"),
            issue=issue,
            assignee=ctx["users"][name],
            project=issue.project,
            workspace=issue.workspace,
        )
    if index % 13 == 0:
        IssueAssignee.objects.create(
            id=spec.uid(f"issue-assignee-deleted:{issue.id}"),
            issue=issue,
            assignee=ctx["users"]["alice"],
            project=issue.project,
            workspace=issue.workspace,
            deleted_at=spec.FIXED_NOW,
        )

    module_names = ["module-alpha", "module-beta", "module-archived"] if project_name == "main" else ["p2-module"]
    for name in rng.sample(module_names, min(len(module_names), index % 3)):
        ModuleIssue.objects.create(
            id=spec.uid(f"module-issue:{issue.id}:{name}"),
            issue=issue,
            module=ctx["modules"][name],
            project=issue.project,
            workspace=issue.workspace,
        )
    if index % 17 == 0:
        ModuleIssue.objects.create(
            id=spec.uid(f"module-issue-deleted:{issue.id}"),
            issue=issue,
            module=ctx["modules"]["module-beta"],
            project=issue.project,
            workspace=issue.workspace,
            deleted_at=spec.FIXED_NOW,
        )

    if index % 3 == 0:
        cycle_name = "cycle-alpha" if index % 6 == 0 else "cycle-beta"
        if project_name == "second":
            cycle_name = "p2-cycle"
        CycleIssue.objects.create(
            id=spec.uid(f"cycle-issue:{issue.id}"),
            issue=issue,
            cycle=ctx["cycles"][cycle_name],
            project=issue.project,
            workspace=issue.workspace,
        )

    if index % 4 == 0:
        statuses: tuple[int, ...] = ([1, -1, 2, 0, -2][(index // 4) % 5],)
        if index % 8 == 0:
            statuses = (1, 2)
        for offset, status in enumerate(statuses):
            IntakeIssue.objects.create(
                id=spec.uid(f"intake-issue:{issue.id}:{offset}"),
                intake=ctx["intake"],
                issue=issue,
                project=issue.project,
                workspace=issue.workspace,
                status=status,
            )

    if index % 6 == 0:
        IssueMention.objects.create(
            id=spec.uid(f"issue-mention:{issue.id}"),
            issue=issue,
            mention=ctx["users"]["bob"],
            project=issue.project,
            workspace=issue.workspace,
        )

    if index % 3 == 0:
        IssueSubscriber.objects.create(
            id=spec.uid(f"issue-subscriber:{issue.id}:alice"),
            issue=issue,
            subscriber=ctx["users"]["alice"],
            project=issue.project,
            workspace=issue.workspace,
        )
        if index % 6 == 0:
            IssueSubscriber.objects.create(
                id=spec.uid(f"issue-subscriber:{issue.id}:bob"),
                issue=issue,
                subscriber=ctx["users"]["bob"],
                project=issue.project,
                workspace=issue.workspace,
            )
    if index % 9 == 0:
        IssueSubscriber.objects.create(
            id=spec.uid(f"issue-subscriber-deleted:{issue.id}"),
            issue=issue,
            subscriber=ctx["users"]["carol"],
            project=issue.project,
            workspace=issue.workspace,
            deleted_at=spec.FIXED_NOW,
        )

    for link in range(index % 4):
        IssueLink.objects.create(
            id=spec.uid(f"issue-link:{issue.id}:{link}"),
            issue=issue,
            title=f"link {link}",
            url=f"https://example.test/{issue.sequence_id}/{link}",
            project=issue.project,
            workspace=issue.workspace,
        )
    for attachment in range(index % 3):
        FileAsset.objects.create(
            id=spec.uid(f"issue-asset:{issue.id}:{attachment}"),
            asset=f"fixtures/{issue.id}-{attachment}.txt",
            issue=issue,
            project=issue.project,
            workspace=issue.workspace,
            user=ctx["users"]["alice"],
            entity_type=FileAsset.EntityTypeContext.ISSUE_ATTACHMENT,
            size=10.0,
            is_uploaded=True,
        )


def create_issues(ctx) -> None:
    rng = random.Random(spec.SEED)
    issues: list[tuple[Issue, dict]] = []
    intake = ctx["intake"]
    assert intake is not None

    for index in range(spec.MAIN_ISSUE_COUNT):
        project = ctx["projects"]["main"]
        bucket = index % 20
        state = ctx["states"]["main"][["backlog", "todo", "in-progress", "done", "cancelled"][index % 5]]
        state_is_null = bucket == 0
        if bucket in (1, 14):
            state = ctx["states"]["main"]["triage"]
        archived_at = None
        is_draft = False
        deleted_at = None
        if bucket == 2:
            deleted_at = spec.FIXED_NOW - timedelta(days=1)
        elif bucket == 3:
            archived_at = (spec.FIXED_NOW - timedelta(days=10)).date()
        elif bucket == 4:
            is_draft = True
        issue = Issue.objects.create(
            id=spec.issue_id(index),
            project=project,
            workspace=project.workspace,
            name=issue_name(index),
            description_html="<p>fixture</p>",
            priority=spec.PRIORITIES[index % 5],
            state=state,
            archived_at=archived_at,
            is_draft=is_draft,
            deleted_at=deleted_at,
            estimate_point=(ctx["points"][index % 3] if index % 7 == 0 else None),
            created_by=ctx["users"]["alice"] if index % 10 else None,
            updated_by=ctx["users"]["bob"] if index % 10 == 1 else None,
        )
        issues.append(
            (
                issue,
                {
                    "index": index,
                    "rel": index,
                    "state_is_null": state_is_null,
                    "start_date": (spec.FIXED_NOW.date() - timedelta(days=index % 400)) if index % 3 else None,
                    "target_date": (spec.FIXED_NOW.date() + timedelta(days=index % 400)) if index % 4 else None,
                    "completed_at": (
                        spec.FIXED_NOW - timedelta(days=index % 200) if state.group == "completed" else None
                    ),
                    "sort_order": float(65535 + index * 10000),
                },
            )
        )

    for index in range(spec.SECOND_ISSUE_COUNT):
        project = ctx["projects"]["second"]
        state = ctx["states"]["second"][["p2-backlog", "p2-started", "p2-done"][index % 3]]
        issue = Issue.objects.create(
            id=spec.issue_id(index, "second"),
            project=project,
            workspace=project.workspace,
            name=issue_name(index, prefix="p2"),
            priority=spec.PRIORITIES[(index + 2) % 5],
            state=state,
            created_by=ctx["users"]["bob"],
        )
        issues.append(
            (
                issue,
                {
                    "index": index + 10_000,
                    "rel": index,
                    "start_date": (spec.FIXED_NOW.date() - timedelta(days=index % 90)) if index % 2 else None,
                    "target_date": (spec.FIXED_NOW.date() + timedelta(days=index % 90)) if index % 2 else None,
                    "completed_at": (
                        spec.FIXED_NOW - timedelta(days=index % 50) if state.group == "completed" else None
                    ),
                    "sort_order": float(65535 + index * 10000),
                },
            )
        )

    # A handful of issues in the archived project and in the other workspace.
    for index in range(5):
        project = ctx["projects"]["archived"]
        issue = Issue.objects.create(
            id=spec.issue_id(index, "archived"),
            project=project,
            workspace=project.workspace,
            name=issue_name(index, prefix="archived"),
            state=None,
            priority="low",
        )
        issues.append((issue, {"index": index, "rel": index, "sort_order": 65535.0}))
    for index in range(3):
        project = ctx["projects"]["other"]
        issue = Issue.objects.create(
            id=spec.issue_id(index, "other"),
            project=project,
            workspace=project.workspace,
            name=issue_name(index, prefix="other"),
            state=None,
            priority="high",
        )
        issues.append((issue, {"index": index, "rel": index, "sort_order": 65535.0}))

    # Relations need the issue rows first.
    for issue, extra in issues:
        if issue.project_id in (ctx["projects"]["main"].id, ctx["projects"]["second"].id):
            create_issue_relations(issue, extra["rel"], rng, ctx)

    # Give the first twenty main issues sub-issues (100..119).
    for offset in range(20):
        parent = issues[offset][0]
        child = issues[100 + offset][0]
        Issue.all_objects.filter(pk=child.pk).update(parent_id=parent.id)

    # Deterministic timestamps, overriding auto_now_add/auto_now. `update()` on a
    # queryset does not run the auto fields, exactly like Django's own bulk paths.
    for issue, extra in issues:
        index = extra["index"]
        created_at = spec.FIXED_NOW - timedelta(days=index % 365, minutes=index)
        Issue.all_objects.filter(pk=issue.pk).update(
            created_at=created_at,
            updated_at=created_at + timedelta(minutes=1 + (index % 600)),
            start_date=extra.get("start_date"),
            target_date=extra.get("target_date"),
            completed_at=extra.get("completed_at"),
            sort_order=extra["sort_order"],
            state_id=None if extra.get("state_is_null") else issue.state_id,
        )


def issue_name(index: int, prefix: str = "issue") -> str:
    words = ["alpha", "BETA", "gamma", "Delta", "epsilon", "zeta", "eta", "theta"]
    word = words[index % len(words)]
    return f"{prefix} {word} work item {index:03d}"


def main() -> None:
    if Workspace.objects.filter(slug=spec.WORKSPACE_SLUG).exists():
        die(
            f"workspace {spec.WORKSPACE_SLUG!r} already exists; "
            "clone a fresh database (apps/server/dev/mkdb.sh <name> --replace) and rerun"
        )

    with transaction.atomic():
        users = create_users()
        workspace, other = create_workspaces(users)
        projects = create_projects(workspace, other, users)
        states = create_states(projects)
        labels = create_labels(projects)
        modules = create_modules(projects)
        cycles = create_cycles(projects, users)
        points = create_estimate(projects["main"])
        intake = Intake.objects.create(
            id=spec.uid("intake"),
            project=projects["main"],
            workspace=workspace,
            name="Fixture Intake",
            is_default=True,
        )
        ctx = {
            "users": users,
            "workspace": workspace,
            "projects": projects,
            "states": states,
            "labels": labels,
            "modules": modules,
            "cycles": cycles,
            "points": points,
            "intake": intake,
            "parents": [],
        }
        create_issues(ctx)

    write_meta()
    print(
        f"build_fixture: workspace={spec.WORKSPACE_SLUG} "
        f"issues={Issue.all_objects.count()} scored={dj_timezone.now().isoformat()}"
    )


def write_meta() -> None:
    """Write the ids and constants the oracle and the Rust tests share."""
    repo_root = Path(__file__).resolve().parents[4]
    golden = repo_root / "apps" / "server" / "crates" / "plane-query" / "tests" / "golden"
    golden.mkdir(parents=True, exist_ok=True)
    meta = {
        "fixed_now": spec.FIXED_NOW.isoformat(),
        "seed": spec.SEED,
        "workspace_slug": spec.WORKSPACE_SLUG,
        "other_workspace_slug": spec.OTHER_WORKSPACE_SLUG,
        "workspace_id": str(spec.WORKSPACE_ID),
        "project_id": str(spec.PROJECT_ID),
        "project2_id": str(spec.PROJECT2_ID),
        "archived_project_id": str(spec.ARCHIVED_PROJECT_ID),
        "other_project_id": str(spec.OTHER_PROJECT_ID),
        "users": {
            "alice": {"id": str(spec.ALICE), "timezone": "UTC"},
            "bob": {"id": str(spec.BOB), "timezone": "Asia/Tokyo"},
            "carol": {"id": str(spec.CAROL), "timezone": "America/Los_Angeles"},
            "dave": {"id": str(spec.DAVE), "timezone": "Europe/Berlin"},
        },
        "states": {
            project: {name: str(value) for name, value in rows.items()} for project, rows in project_states().items()
        },
        "labels": {name: str(spec.label_id(name)) for name in spec.LABELS},
        "modules": {name: str(spec.module_id(name)) for name in spec.MODULES},
        "cycles": {name: str(spec.cycle_id(name)) for name in spec.CYCLES},
        "estimate_points": {str(key): str(spec.estimate_point_id(key)) for key in (1, 2, 3)},
    }
    (golden / "meta.json").write_text(json.dumps(meta, indent=2, sort_keys=True) + "\n")
    print(f"build_fixture: wrote {golden / 'meta.json'}")


def project_states() -> dict[str, dict]:
    return {
        "main": spec.state_ids("main"),
        "second": spec.state_ids("second"),
    }


if __name__ == "__main__":
    main()
