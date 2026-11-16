#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Generate the deterministic SQL fixture the policy probe runs against.

Why a fixture file instead of HTTP setup: `/auth/` and `/api/` are not ported in
Phase 2, so a scenario cannot create users, sessions or projects through the
Rust backend. The harness applies a SQL file to **both** target databases
between the template clone and the baseline snapshot (`Scenario::fixture_sql`,
the reserved `fixture_sql` scenario variable), so both sides start from
identical rows and the fixture never appears in a step diff.

How it is produced:

1. clone the template database (`plane_tpl`) into a scratch database;
2. create the users, sessions, workspace, projects and one object of each kind
   a URL kwarg can name, with a fixed UUID namespace and fixed values;
3. `pg_dump --data-only --inserts --on-conflict-do-nothing` the tables this
   script touched;
4. drop the scratch database and write the file.

The dump is what guarantees the columns match the Django-migrated schema; no
INSERT is written by hand. `--on-conflict-do-nothing` makes replaying against
an attached database idempotent.

Usage (from `apps/server`):

    cd apps/api
    .venv/bin/python ../server/tools/policy/gen_fixture.py

`SECRET_KEY` is read from `apps/server/dev/.env.local` (the same file the
harness gives both targets), so the signed session payloads verify on both
backends. Nothing is printed except counts; no secret is written to the file.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import uuid
from pathlib import Path

SCRIPT = Path(__file__).resolve()
SERVER_DIR = SCRIPT.parents[2]
REPO_ROOT = SCRIPT.parents[4]
API_DIR = REPO_ROOT / "apps" / "api"
ENV_FILE = SERVER_DIR / "dev" / ".env.local"
DEFAULT_OUT = SERVER_DIR / "scenarios" / "policy-fixture.sql"
MANIFEST_OUT = SERVER_DIR / "scenarios" / "policy-fixture.json"

NAMESPACE = uuid.UUID("6f1d3c2a-0000-4000-8000-00000000a11c")


def stable(name: str) -> uuid.UUID:
    return uuid.uuid5(NAMESPACE, name)


def read_secret_key() -> str:
    for line in ENV_FILE.read_text().splitlines():
        line = line.strip()
        if line.startswith("SECRET_KEY="):
            return line.split("=", 1)[1].strip()
    raise SystemExit(f"SECRET_KEY not found in {ENV_FILE}")


def database_url() -> str:
    value = os.environ.get("DATABASE_URL")
    if not value:
        raise SystemExit("DATABASE_URL must be set (source apps/server/dev/django-env.sh)")
    return value


def psql(url: str, sql: str, database: str | None = None) -> None:
    """Run one statement through `psql` (CREATE/DROP DATABASE cannot run in a tx)."""
    import urllib.parse

    parts = urllib.parse.urlparse(url)
    env = dict(os.environ, PGPASSWORD=urllib.parse.unquote(parts.password or ""))
    command = [
        "psql",
        "-h",
        parts.hostname or "127.0.0.1",
        "-p",
        str(parts.port or 5432),
        "-U",
        urllib.parse.unquote(parts.username or "plane"),
        "-d",
        database or urllib.parse.unquote(parts.path.lstrip("/") or "postgres"),
        "-v",
        "ON_ERROR_STOP=1",
        "-q",
        "-c",
        sql,
    ]
    subprocess.run(command, check=True, env=env, capture_output=True, text=True)


def with_database(url: str, database: str) -> str:
    import urllib.parse

    parts = urllib.parse.urlparse(url)
    return urllib.parse.urlunparse(parts._replace(path="/" + database))


def dump(wanted: list[str], url: str, out: Path) -> None:
    import shutil
    import urllib.parse

    parts = urllib.parse.urlparse(url)
    password = urllib.parse.unquote(parts.password or "")
    username = urllib.parse.unquote(parts.username or "plane")
    database = urllib.parse.unquote(parts.path.lstrip("/"))
    tables: list[str] = []
    for table in wanted:
        tables += ["--table", f"public.{table}"]

    # The dev Postgres server is 15.7 and the Homebrew client is older; prefer
    # the matching `pg_dump` inside the dev container. The data path is the
    # container's local socket, so no host/port is passed.
    container = os.environ.get("PLANE_PG_CONTAINER", "plane-rs-dev-postgres-1")
    if shutil.which("docker") and subprocess.run(
        ["docker", "inspect", "-f", "{{.State.Running}}", container],
        capture_output=True,
        text=True,
    ).stdout.strip() == "true":
        command = [
            "docker",
            "exec",
            "-e",
            f"PGPASSWORD={password}",
            container,
            "pg_dump",
            "--data-only",
            "--inserts",
            "--on-conflict-do-nothing",
            "--no-owner",
            "--no-privileges",
            "--no-comments",
            "-U",
            username,
            "-d",
            database,
            *tables,
        ]
    else:
        env = dict(os.environ, PGPASSWORD=password)
        command = [
            "pg_dump",
            "--data-only",
            "--inserts",
            "--on-conflict-do-nothing",
            "--no-owner",
            "--no-privileges",
            "--no-comments",
            "-h",
            parts.hostname or "127.0.0.1",
            "-p",
            str(parts.port or 5432),
            "-U",
            username,
            "-d",
            database,
            *tables,
        ]
    result = subprocess.run(
        command,
        env=dict(os.environ, PGPASSWORD=password),
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise SystemExit(f"pg_dump failed: {result.stderr.strip()}")
    body = order_inserts(result.stdout, wanted)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(
        "-- Copyright (c) 2023-present Plane Software, Inc. and contributors\n"
        "-- SPDX-License-Identifier: AGPL-3.0-only\n"
        "-- See the LICENSE file for details.\n"
        "--\n"
        "-- GENERATED by apps/server/tools/policy/gen_fixture.py. Do not edit.\n"
        "-- The policy probe's users, sessions, workspace, projects and objects.\n"
        "-- Applied to both differential targets before the baseline snapshot.\n\n"
        + body,
        encoding="utf-8",
        newline="\n",
    )


# Foreign keys are checked per statement, and `pg_dump --table` does not
# topologically sort the selected tables. Applied in dump order, `issues` can
# precede `states`. Emit the INSERT blocks in dependency order instead.
TABLE_ORDER = (
    "users",
    "workspaces",
    "workspace_members",
    "projects",
    "project_members",
    "states",
    "labels",
    "cycles",
    "modules",
    "descriptions",
    "issues",
    "issue_comments",
    "issue_views",
    "pages",
    "intakes",
    "estimates",
    "estimate_points",
    "stickies",
    "draft_issues",
    "file_assets",
    "deploy_boards",
    "api_tokens",
    "instance_admins",
    "sessions",
)


def order_inserts(dump_text: str, tables: list[str]) -> str:
    """Group INSERTs by table and emit them in [`TABLE_ORDER`]."""
    header: list[str] = []
    blocks: dict[str, list[str]] = {table: [] for table in tables}
    for line in dump_text.splitlines():
        if line.startswith("INSERT INTO"):
            table = line.split()[2].split(".")[-1].strip('"')
            blocks.setdefault(table, []).append(line)
        elif line.strip():
            header.append(line)
    ordered: list[str] = list(header)
    seen: set[str] = set()
    for table in TABLE_ORDER:
        if table in blocks:
            ordered.extend(blocks.pop(table))
            seen.add(table)
    # Any table the order list does not know: keep it, deterministically.
    for table in sorted(blocks):
        ordered.extend(blocks[table])
    return "\n".join(ordered) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--manifest", type=Path, default=MANIFEST_OUT)
    parser.add_argument("--keep", action="store_true", help="keep the scratch database")
    args = parser.parse_args()

    url = database_url()
    scratch = f"t_policy_fixture_{os.getpid()}"
    os.environ["SECRET_KEY"] = read_secret_key()

    psql(url, f'CREATE DATABASE "{scratch}" TEMPLATE plane_tpl')
    try:
        os.environ["DATABASE_URL"] = with_database(url, scratch)
        # Django import order: settings first.
        sys.path.insert(0, str(API_DIR))
        os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
        import django

        django.setup()
        manifest = create_fixture()
        django.db.connections.close_all()
        dump(manifest.pop("tables"), with_database(url, scratch), args.out)
    finally:
        if not args.keep:
            psql(url, f'DROP DATABASE IF EXISTS "{scratch}" WITH (FORCE)')

    manifest["scratch_database"] = scratch if args.keep else None
    args.manifest.write_text(
        json.dumps(manifest, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"wrote {args.out} ({args.out.stat().st_size} bytes)")
    print(f"wrote {args.manifest}")
    return 0


def create_fixture() -> dict:
    from django.contrib.sessions.serializers import JSONSerializer
    from django.core import signing
    from django.utils import timezone
    from plane.license.models import Instance, InstanceAdmin
    from plane.db.models import (
        APIToken,
        Cycle,
        DeployBoard,
        DraftIssue,
        Estimate,
        EstimatePoint,
        FileAsset,
        Intake,
        Issue,
        IssueComment,
        IssueView,
        Label,
        Module,
        Page,
        Project,
        ProjectMember,
        State,
        Sticky,
        User,
        Workspace,
        WorkspaceMember,
    )

    now = timezone.now()
    tables: set[str] = set()
    manifest: dict = {}

    def user(name: str, *, active: bool = True) -> User:
        key = stable(f"user:{name}")
        email = f"{name}@policy.test"
        row = User.objects.create(
            id=key,
            email=email,
            username=key.hex,
            display_name=name.replace("_", " ").title(),
            first_name=name.replace("_", " ").title(),
            last_name="Probe",
            user_timezone="Asia/Kolkata",
            is_active=active,
            is_email_verified=True,
            password="!unusable-password-hash",
        )
        tables.add("users")
        manifest.setdefault("users", {})[name] = {
            "id": str(row.id),
            "email": email,
            "is_active": active,
        }
        return row

    def session(name: str, owner: User) -> str:
        key = "".join(ch for ch in stable(f"session:{name}").hex) + "0" * 96
        payload = {
            "_auth_user_id": str(owner.id),
            "_auth_user_backend": "django.contrib.auth.backends.ModelBackend",
            "_auth_user_hash": owner.get_session_auth_hash(),
            "device_info": {"user_agent": "policy-probe", "ip_address": "203.0.113.7", "domain": "policy.test"},
        }
        data = signing.dumps(
            payload,
            salt="django.contrib.sessions.SessionStore",
            serializer=JSONSerializer,
            compress=True,
        )
        from plane.db.models.session import Session

        Session.objects.create(
            session_key=key,
            session_data=data,
            expire_date=now + timezone.timedelta(days=7),
            user_id=str(owner.id),
            device_info=payload["device_info"],
        )
        tables.add("sessions")
        manifest.setdefault("sessions", {})[name] = key
        return key

    users = {
        "nonmember": user("policy_nonmember"),
        "ws_guest": user("policy_ws_guest"),
        "ws_member": user("policy_ws_member"),
        "ws_admin": user("policy_ws_admin"),
        "pj_guest": user("policy_pj_guest"),
        "pj_member": user("policy_pj_member"),
        "pj_admin": user("policy_pj_admin"),
        "inactive": user("policy_inactive", active=False),
        # API-key users: `scenarios/v1-token-log.yaml` authenticates as
        # `api_active`; the policy probe itself sends no X-Api-Key, so every v1
        # step there is the anonymous denial.
        "api_active": user("policy_api_active"),
        "api_expired": user("policy_api_expired"),
        "api_inactive": user("policy_api_inactive"),
        # Users used by exactly one destructive step each (deactivate, project
        # leave, workspace leave), so a successful call cannot change what any
        # later step sees.
        "disposable": user("policy_disposable"),
        "disposable_project_leave": user("policy_disposable_project_leave"),
        "disposable_workspace_leave": user("policy_disposable_workspace_leave"),
    }
    for name, row in users.items():
        if name not in ("api_active", "api_expired", "api_inactive"):
            manifest.setdefault("session_keys", {})[name] = session(f"read:{name}", row)

    workspace = Workspace.objects.create(
        id=stable("workspace"),
        slug="policy",
        name="Policy Probe",
        owner=users["ws_admin"],
    )
    tables.add("workspaces")
    manifest["workspace"] = {"id": str(workspace.id), "slug": workspace.slug}

    # A second workspace used only by the `DELETE /api/workspaces/{slug}/`
    # steps at the end of the write block: deleting it cannot affect the
    # workspace every other step uses. ws_admin is its admin so the permitted
    # delete is exercised; ws_member is a member so the deny path is too.
    throwaway = Workspace.objects.create(
        id=stable("workspace:throwaway"),
        slug="policy-throwaway",
        name="Policy Throwaway",
        owner=users["ws_admin"],
    )
    tables.add("workspaces")
    manifest["throwaway_workspace_slug"] = throwaway.slug
    for name, role in (
        ("ws_admin", 20),
        ("ws_member", 15),
        ("disposable", 15),
        ("disposable_workspace_leave", 15),
    ):
        WorkspaceMember.objects.create(
            id=stable(f"workspace_member:throwaway:{name}"),
            workspace=throwaway,
            member=users[name],
            role=role,
            company_role="probe",
        )
    tables.add("workspace_members")
    # A throwaway project used only by the project-`leave` step, so leaving it
    # cannot change a later step's project membership.
    throwaway_project = Project.objects.create(
        id=stable("project:throwaway"),
        workspace=throwaway,
        name="Throwaway Project",
        identifier="THR",
        created_by=users["ws_admin"],
    )
    tables.add("projects")
    for name in ("disposable", "disposable_project_leave"):
        ProjectMember.objects.create(
            id=stable(f"project_member:throwaway:{name}"),
            workspace=throwaway,
            project=throwaway_project,
            member=users[name],
            role=15,
        )
    tables.add("project_members")
    manifest["throwaway_project"] = str(throwaway_project.id)
    # A second throwaway workspace for the two `leave` steps: the delete step
    # above removes the first one before `members/leave/` runs.
    throwaway_leave = Workspace.objects.create(
        id=stable("workspace:throwaway_leave"),
        slug="policy-throwaway-leave",
        name="Policy Throwaway Leave",
        owner=users["ws_admin"],
    )
    tables.add("workspaces")
    manifest["throwaway_leave_workspace_slug"] = throwaway_leave.slug
    for name in ("disposable_project_leave", "disposable_workspace_leave"):
        WorkspaceMember.objects.create(
            id=stable(f"workspace_member:throwaway_leave:{name}"),
            workspace=throwaway_leave,
            member=users[name],
            role=15,
        )
    tables.add("workspace_members")
    throwaway_leave_project = Project.objects.create(
        id=stable("project:throwaway_leave"),
        workspace=throwaway_leave,
        name="Throwaway Leave Project",
        identifier="THL",
        created_by=users["ws_admin"],
    )
    tables.add("projects")
    ProjectMember.objects.create(
        id=stable("project_member:throwaway_leave:disposable_project_leave"),
        workspace=throwaway_leave,
        project=throwaway_leave_project,
        member=users["disposable_project_leave"],
        role=15,
    )
    tables.add("project_members")
    manifest["throwaway_leave_project"] = str(throwaway_leave_project.id)

    roles = {"ws_guest": 5, "ws_member": 15, "ws_admin": 20, "pj_guest": 5, "pj_member": 15, "pj_admin": 20}
    for name, role in roles.items():
        WorkspaceMember.objects.create(
            id=stable(f"workspace_member:{name}"),
            workspace=workspace,
            member=users[name],
            role=role,
            company_role="probe",
        )
    tables.add("workspace_members")

    projects = {}
    for key, identifier in (("alpha", "ALP"), ("beta", "BET")):
        project = Project.objects.create(
            id=stable(f"project:{key}"),
            workspace=workspace,
            name=f"Project {key.title()}",
            identifier=identifier,
            created_by=users["ws_admin"],
        )
        projects[key] = project
        tables.add("projects")
        for name, role in roles.items():
            ProjectMember.objects.create(
                id=stable(f"project_member:{key}:{name}"),
                workspace=workspace,
                project=project,
                member=users[name],
                role=role,
            )
        tables.add("project_members")
    manifest["projects"] = {key: {"id": str(project.id), "identifier": project.identifier} for key, project in projects.items()}

    alpha = projects["alpha"]
    state = State.objects.create(
        id=stable("state"),
        workspace=workspace,
        project=alpha,
        name="Todo",
        group="unstarted",
        color="#60646C",
        sequence=1,
        created_by=users["ws_admin"],
    )
    label = Label.objects.create(
        id=stable("label"),
        workspace=workspace,
        project=alpha,
        name="Probe",
        color="#ff0000",
        created_by=users["ws_admin"],
    )
    cycle = Cycle.objects.create(
        id=stable("cycle"),
        workspace=workspace,
        project=alpha,
        name="Probe Cycle",
        description="",
        owned_by=users["ws_admin"],
        created_by=users["ws_admin"],
    )
    module = Module.objects.create(
        id=stable("module"),
        workspace=workspace,
        project=alpha,
        name="Probe Module",
        description="",
        created_by=users["ws_admin"],
    )
    issue = Issue.objects.create(
        id=stable("issue"),
        workspace=workspace,
        project=alpha,
        name="Probe Issue",
        state=state,
        sequence_id=1,
        created_by=users["ws_admin"],
    )
    comment = IssueComment.objects.create(
        id=stable("issue_comment"),
        workspace=workspace,
        project=alpha,
        issue=issue,
        comment_html="<p>probe</p>",
        created_by=users["ws_admin"],
    )
    page = Page.objects.create(
        id=stable("page"),
        workspace=workspace,
        name="Probe Page",
        owned_by=users["ws_admin"],
        color="#ff0000",
    )
    view = IssueView.objects.create(
        id=stable("view"),
        workspace=workspace,
        name="Probe View",
        description="",
        query={},
        owned_by=users["ws_admin"],
    )
    intake = Intake.objects.create(
        id=stable("intake"),
        workspace=workspace,
        project=alpha,
        name="Probe Intake",
        description="",
    )
    estimate = Estimate.objects.create(
        id=stable("estimate"),
        workspace=workspace,
        project=alpha,
        name="Probe Estimate",
        description="",
    )
    estimate_point = EstimatePoint.objects.create(
        id=stable("estimate_point"),
        workspace=workspace,
        project=alpha,
        estimate=estimate,
        description="",
        value="1",
    )
    sticky = Sticky.objects.create(
        id=stable("sticky"),
        workspace=workspace,
        owner=users["ws_admin"],
        name="Probe Sticky",
        description="{}\n",
        color="#ff0000",
    )
    draft = DraftIssue.objects.create(
        id=stable("draft"),
        workspace=workspace,
        name="Probe Draft",
    )
    asset = FileAsset.objects.create(
        id=stable("file_asset"),
        workspace=workspace,
        project=alpha,
        issue=issue,
        entity_type="ISSUE_ATTACHMENT",
        entity_identifier=str(issue.id),
        size=5,
        asset="policy/probe.txt",
    )
    tables.update(
        {
            "states",
            "labels",
            "cycles",
            "modules",
            "descriptions",
            "issues",
            "issue_comments",
            "pages",
            "issue_views",
            "intakes",
            "estimates",
            "estimate_points",
            "stickies",
            "draft_issues",
            "file_assets",
        }
    )

    # A deploy board whose anchor points at the project, plus one pointing at
    # another project: the space `DeployBoardAnchorMixin` must 404 the second.
    DeployBoard.objects.create(
        id=stable("deploy_board_ok"),
        workspace=workspace,
        project=alpha,
        entity_name="project",
        entity_identifier=alpha.id,
        anchor="anchor-ok",
    )
    DeployBoard.objects.create(
        id=stable("deploy_board_bad"),
        workspace=workspace,
        project=alpha,
        entity_name="project",
        entity_identifier=projects["beta"].id,
        anchor="anchor-bad",
    )
    tables.add("deploy_boards")

    instance = Instance.objects.first()
    InstanceAdmin.objects.create(
        id=stable("instance_admin"),
        instance=instance,
        user=users["ws_admin"],
        role=20,
    )
    tables.add("instance_admins")

    # API tokens: active, expired, and inactive-owner, for the fixture contract.
    for name, expired, active in (
        ("active", False, True),
        ("expired", True, True),
        ("inactive", False, False),
    ):
        owner = users["api_active"] if name == "active" else users["api_expired"]
        APIToken.objects.create(
            id=stable(f"api_token:{name}"),
            user=owner,
            label=f"probe-{name}",
            token=stable(f"api_token_value:{name}").hex + stable(f"api_token_value2:{name}").hex,
            expired_at=(now - timezone.timedelta(days=1)) if expired else None,
            is_active=active,
        )
    tables.add("api_tokens")

    manifest["creator_objects"] = {
        "plane.db.models.issue.Issue": str(issue.id),
        "plane.db.models.issue.IssueComment": str(comment.id),
        "plane.db.models.asset.FileAsset": str(asset.id),
        "plane.db.models.sticky.Sticky": str(sticky.id),
        "plane.db.models.cycle.Cycle": str(cycle.id),
        "plane.db.models.module.Module": str(module.id),
        "plane.db.models.view.IssueView": str(view.id),
        "plane.db.models.draft.DraftIssue": str(draft.id),
    }
    manifest["objects"] = {
        "issue": str(issue.id),
        "issue_comment": str(comment.id),
        "page": str(page.id),
        "view": str(view.id),
        "cycle": str(cycle.id),
        "module": str(module.id),
        "label": str(label.id),
        "state": str(state.id),
        "intake": str(intake.id),
        "estimate": str(estimate.id),
        "estimate_point": str(estimate_point.id),
        "sticky": str(sticky.id),
        "draft": str(draft.id),
        "file_asset": str(asset.id),
        "member": str(users["pj_member"].id),
        "anchor_ok": "anchor-ok",
        "anchor_bad": "anchor-bad",
        "nonexistent": str(stable("does-not-exist")),
        "issue_identifier": f"{alpha.identifier}-{issue.sequence_id}",
    }
    manifest["tables"] = sorted(tables)
    manifest["counts"] = {
        table: {"rows": 0}
        for table in sorted(tables)
    }
    for table in sorted(tables):
        from django.db import connection

        with connection.cursor() as cursor:
            cursor.execute(f'SELECT count(*) FROM "{table}"')
            manifest["counts"][table]["rows"] = cursor.fetchone()[0]
    return manifest


if __name__ == "__main__":
    raise SystemExit(main())
