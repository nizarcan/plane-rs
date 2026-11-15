# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Build the permissions differential fixture in a throwaway database.

Usage (from the repository root):

    apps/server/dev/mkdb.sh t_perms_fixture --replace
    PLANE_DB=t_perms_fixture apps/server/tools/permissions/run.sh \\
        apps/api/.venv/bin/python apps/server/tools/permissions/fixture.py

``run.sh`` sources ``apps/server/dev/django-env.sh``, so the builder never
talks to any database other than the ``PLANE_DB`` clone.

Why the Django ORM and not raw SQL: the fixture needs every model default
(JSON blobs, roles, ``is_active``), the model hooks that fill ``workspace_id``
from the project, and the side effects of ``Issue.save()`` (advisory lock,
``IssueSequence`` row, ``sort_order``). Raw INSERTs would have to reproduce all
of them and would drift silently when the schema changes. The builder only
writes; ``oracle.py`` only reads.

The database must be a fresh clone of ``plane_tpl``: one instance row, no users
and no workspaces (``dev/mkdb.sh`` guarantees that). The builder refuses to run
otherwise, because a second run would violate the partial unique constraints.
"""

from __future__ import annotations

import json
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

import common

HERE = Path(__file__).resolve().parent
SERVER_DIR = HERE.parents[1]
GOLDEN = SERVER_DIR / "crates" / "plane-permissions" / "tests" / "golden"
FIXTURE_JSON = GOLDEN / "fixture.json"

T0 = datetime(2026, 1, 1, 0, 0, 0, tzinfo=timezone.utc)
T_LATER = T0 + timedelta(days=1)


def die(message: str) -> None:
    print(f"error: {message}", file=sys.stderr)
    raise SystemExit(1)


def main() -> None:
    import django
    from django.conf import settings

    try:
        django.setup()
    except Exception as error:  # pragma: no cover - environment mistake
        die(f"django.setup() failed ({error}); run through tools/permissions/run.sh")

    db_name = settings.DATABASES["default"]["NAME"]
    if not str(db_name).startswith("t_"):
        die(f"refusing to write to database {db_name!r}: expected a t_* clone (dev/mkdb.sh)")

    from django.contrib.auth.hashers import make_password
    from django.db import transaction
    from plane.db.models import (
        Cycle,
        DraftIssue,
        FileAsset,
        Issue,
        IssueComment,
        IssueView,
        Module,
        Page,
        Project,
        ProjectMember,
        ProjectPage,
        Sticky,
        User,
        Workspace,
        WorkspaceMember,
    )
    from plane.license.models import Instance, InstanceAdmin

    if Workspace.all_objects.filter(slug=common.W1_SLUG).exists():
        die(f"database {db_name!r} already contains the fixture; re-clone it with mkdb.sh --replace")

    if Instance.objects.count() != 1:
        die(f"expected exactly one instance row in {db_name!r}, found {Instance.objects.count()}")
    instance = Instance.objects.first()

    with transaction.atomic():
        # ------------------------------------------------------------------
        # Users
        # ------------------------------------------------------------------
        def make_user(name: str, spec: dict):
            user = User.objects.create(
                id=spec["id"],
                username=name,
                email=f"{name}@perms.test",
                display_name=name,
                first_name=name,
                last_name="fixture",
                password=make_password("not-used-by-permissions"),
                is_active=name != "user_inactive",
            )
            User.objects.filter(id=spec["id"]).update(created_at=T0, updated_at=T0)
            return user

        for name, spec in common.USERS.items():
            if spec["id"] is not None:
                make_user(name, spec)

        def u(name: str):
            return User.objects.get(id=common.USERS[name]["id"])

        # ------------------------------------------------------------------
        # Workspaces
        # ------------------------------------------------------------------
        def workspace(wid, slug, name, owner):
            Workspace.objects.create(id=wid, slug=slug, name=name, owner=owner)
            Workspace.objects.filter(id=wid).update(created_at=T0, updated_at=T0)

        workspace(common.W1, common.W1_SLUG, "Perms Main", u("user_admin"))
        workspace(common.W2, common.W2_SLUG, "Perms Other", u("user_admin"))
        workspace(common.W3, common.W3_SLUG, "Perms Deleted", u("user_admin"))

        w1 = Workspace.objects.get(id=common.W1)
        w2 = Workspace.objects.get(id=common.W2)
        w3 = Workspace.objects.get(id=common.W3)

        # ------------------------------------------------------------------
        # Workspace members
        # ------------------------------------------------------------------
        def member(ws, user, role, *, is_active=True, deleted_at=None):
            row = WorkspaceMember.objects.create(
                workspace=ws, member=user, role=role, is_active=is_active
            )
            WorkspaceMember.all_objects.filter(id=row.id).update(
                created_at=T0, updated_at=T0, deleted_at=deleted_at
            )
            return row

        member(w1, u("user_admin"), 20)
        member(w1, u("user_member"), 15)
        member(w1, u("user_guest"), 5)
        member(w1, u("user_guest_all"), 5)
        member(w1, u("user_ws_admin_no_project"), 20)
        member(w1, u("user_ws_member_no_project"), 15)
        member(w1, u("user_project_deleted_member"), 15)
        member(w1, u("user_inactive_member"), 20, is_active=False)
        member(w1, u("user_inactive"), 20)
        member(w1, u("user_deleted_member"), 20, deleted_at=T_LATER)

        member(w2, u("user_admin"), 20)
        member(w2, u("user_w2_admin"), 20)
        member(w3, u("user_member"), 15)

        # A soft-deleted workspace whose member rows are still active. The
        # permission classes join ``workspaces`` by slug without applying the
        # workspace's own soft-delete manager, so these members keep passing
        # workspace-level checks. That is Django's behaviour and the matrix
        # pins it.
        Workspace.all_objects.filter(id=common.W3).update(deleted_at=T_LATER)

        # ------------------------------------------------------------------
        # Projects
        # ------------------------------------------------------------------
        def project(pid, ws, identifier, name, *, guest_view_all=False, deleted_at=None):
            Project.objects.create(
                id=pid,
                workspace=ws,
                identifier=identifier,
                name=name,
                guest_view_all_features=guest_view_all,
            )
            Project.all_objects.filter(id=pid).update(
                created_at=T0, updated_at=T0, deleted_at=deleted_at
            )
            return Project.all_objects.get(id=pid)

        p1 = project(common.P1, w1, common.P1_IDENT, "Perm One")
        p2 = project(common.P2, w1, common.P2_IDENT, "Perm Two", guest_view_all=True)
        p3 = project(common.P3, w1, common.P3_IDENT, "Perm Deleted", deleted_at=T_LATER)
        p_other = project(common.P_OTHER, w2, common.P_OTHER_IDENT, "Perm Other")

        # ------------------------------------------------------------------
        # Project members
        # ------------------------------------------------------------------
        def project_member(project_obj, user, role, *, deleted_at=None):
            row = ProjectMember.objects.create(project=project_obj, member=user, role=role)
            ProjectMember.all_objects.filter(id=row.id).update(
                created_at=T0, updated_at=T0, deleted_at=deleted_at
            )
            return row

        project_member(p1, u("user_admin"), 20)
        project_member(p1, u("user_member"), 15)
        project_member(p1, u("user_guest"), 5)
        project_member(p1, u("user_inactive_member"), 20)
        project_member(p1, u("user_inactive"), 20)
        project_member(p1, u("user_deleted_member"), 20, deleted_at=T_LATER)

        project_member(p2, u("user_member"), 15)
        project_member(p2, u("user_guest_all"), 5)

        project_member(p3, u("user_admin"), 20)
        project_member(p3, u("user_project_deleted_member"), 15)

        project_member(p_other, u("user_w2_admin"), 20)
        project_member(p_other, u("user_admin"), 20)

        # ------------------------------------------------------------------
        # Pages and their project links
        # ------------------------------------------------------------------
        def page(page_id, ws, owner, access, name):
            Page.objects.create(id=page_id, workspace=ws, owned_by=owner, access=access, name=name)
            Page.all_objects.filter(id=page_id).update(created_at=T0, updated_at=T0)

        page(common.PAGE_PUBLIC, w1, u("user_member"), 0, "Public page")
        page(common.PAGE_PRIVATE_MEMBER, w1, u("user_member"), 1, "Private page of member")
        page(common.PAGE_PRIVATE_GUEST, w1, u("user_guest"), 1, "Private page of guest")
        page(common.PAGE_UNLINKED, w1, u("user_member"), 0, "Unlinked page")
        page(common.PAGE_LINK_DELETED, w1, u("user_member"), 0, "Link deleted page")
        page(common.PAGE_P2, w1, u("user_member"), 0, "Page in P2")
        page(common.PAGE_OTHER_WS, w2, u("user_w2_admin"), 0, "Page in W2")

        def link(page_id, project_obj, *, deleted_at=None):
            row = ProjectPage.objects.create(
                page_id=page_id, project=project_obj, workspace=project_obj.workspace
            )
            ProjectPage.all_objects.filter(id=row.id).update(deleted_at=deleted_at)
            return row

        link(common.PAGE_PUBLIC, p1)
        link(common.PAGE_PRIVATE_MEMBER, p1)
        link(common.PAGE_PRIVATE_GUEST, p1)
        link(common.PAGE_LINK_DELETED, p1, deleted_at=T_LATER)
        link(common.PAGE_P2, p2)
        link(common.PAGE_OTHER_WS, p_other)

        # ------------------------------------------------------------------
        # Issues
        # ------------------------------------------------------------------
        def issue(issue_id, project_obj, name, *, deleted_at=None):
            Issue.objects.create(id=issue_id, project=project_obj, name=name)
            Issue.all_objects.filter(id=issue_id).update(
                created_at=T0, updated_at=T0, deleted_at=deleted_at
            )
            return Issue.all_objects.get(id=issue_id)

        issue(common.ISSUE_GUEST, p1, "Guest issue")
        issue(common.ISSUE_MEMBER, p1, "Member issue")
        issue(common.ISSUE_P2, p2, "P2 issue")
        issue(common.ISSUE_GUEST_DELETED, p1, "Deleted guest issue", deleted_at=T_LATER)

        # ------------------------------------------------------------------
        # Every model used with creator=True in routes.json
        # ------------------------------------------------------------------
        IssueComment.objects.create(
            id=common.COMMENT_GUEST, project=p1, issue_id=common.ISSUE_GUEST, comment_html="<p>c</p>"
        )
        Cycle.objects.create(
            id=common.CYCLE_GUEST, project=p1, name="Guest cycle", owned_by=u("user_guest")
        )
        Module.objects.create(id=common.MODULE_GUEST, project=p1, name="Guest module")
        IssueView.objects.create(
            id=common.VIEW_GUEST, project=p1, name="Guest view", query={}, owned_by=u("user_guest")
        )
        IssueView.objects.create(
            id=common.VIEW_WS_GUEST,
            workspace=w1,
            name="Guest workspace view",
            query={},
            owned_by=u("user_guest"),
        )
        DraftIssue.objects.create(id=common.DRAFT_GUEST, project=p1, name="Guest draft")
        Sticky.objects.create(
            id=common.STICKY_GUEST, workspace=w1, owner=u("user_guest"), name="Guest sticky"
        )
        FileAsset.objects.create(
            id=common.ASSET_GUEST,
            project=p1,
            issue_id=common.ISSUE_GUEST,
            workspace=w1,
            user=u("user_guest"),
            asset="perms/guest-asset.txt",
            entity_type=FileAsset.EntityTypeContext.ISSUE_ATTACHMENT,
            entity_identifier=str(common.ISSUE_GUEST),
        )

        for model, fixed in (
            (IssueComment, common.COMMENT_GUEST),
            (Cycle, common.CYCLE_GUEST),
            (Module, common.MODULE_GUEST),
            (IssueView, common.VIEW_GUEST),
            (DraftIssue, common.DRAFT_GUEST),
            (Sticky, common.STICKY_GUEST),
            (FileAsset, common.ASSET_GUEST),
        ):
            model.all_objects.filter(id=fixed).update(created_at=T0, updated_at=T0)

        # ``BaseModel.save()`` overwrites the audit columns from the current
        # request user (None here), so ``created_by`` is set afterwards with a
        # column-only UPDATE, exactly what a fixture needs and what ``crum``
        # would have produced inside a request.
        def set_created_by(model, pk, user):
            model.all_objects.filter(id=pk).update(
                created_by_id=user.id, updated_by_id=user.id, created_at=T0, updated_at=T0
            )

        set_created_by(Issue, common.ISSUE_GUEST, u("user_guest"))
        set_created_by(Issue, common.ISSUE_MEMBER, u("user_member"))
        set_created_by(Issue, common.ISSUE_P2, u("user_member"))
        set_created_by(Issue, common.ISSUE_GUEST_DELETED, u("user_guest"))
        set_created_by(IssueComment, common.COMMENT_GUEST, u("user_guest"))
        set_created_by(Cycle, common.CYCLE_GUEST, u("user_guest"))
        set_created_by(Module, common.MODULE_GUEST, u("user_guest"))
        set_created_by(IssueView, common.VIEW_GUEST, u("user_guest"))
        set_created_by(IssueView, common.VIEW_WS_GUEST, u("user_guest"))
        set_created_by(DraftIssue, common.DRAFT_GUEST, u("user_guest"))
        set_created_by(Sticky, common.STICKY_GUEST, u("user_guest"))
        set_created_by(FileAsset, common.ASSET_GUEST, u("user_guest"))

        # ------------------------------------------------------------------
        # Instance administrators
        # ------------------------------------------------------------------
        def instance_admin(fixed, user, role, *, deleted_at=None):
            InstanceAdmin.objects.create(id=fixed, instance=instance, user=user, role=role)
            InstanceAdmin.all_objects.filter(id=fixed).update(
                created_at=T0, updated_at=T0, deleted_at=deleted_at
            )

        instance_admin(common.INSTANCE_ADMIN_ADMIN, u("user_admin"), 20)
        instance_admin(common.INSTANCE_ADMIN_ONLY, u("user_instance_admin"), 20)
        instance_admin(common.INSTANCE_ADMIN_LOW, u("user_instance_admin_low"), 14)
        instance_admin(common.INSTANCE_ADMIN_MID, u("user_instance_admin_mid"), 15)
        instance_admin(common.INSTANCE_ADMIN_DELETED, u("user_member"), 20, deleted_at=T_LATER)

        # A newer instance row that is soft-deleted: ``Instance.objects.first()``
        # must skip it (the manager filters ``deleted_at``) and pick the
        # template row.
        Instance.all_objects.filter(id=instance.id).update(created_at=T0 - timedelta(days=30))
        Instance.objects.create(
            id=common.INSTANCE_DELETED,
            instance_name="Deleted newer instance",
            instance_id="perms-deleted-instance",
            current_version=instance.current_version,
            last_checked_at=datetime.now(timezone.utc),
        )
        Instance.all_objects.filter(id=common.INSTANCE_DELETED).update(
            created_at=T0, deleted_at=T_LATER
        )

    # ----------------------------------------------------------------------
    # Record the fixture for the oracle and for readers of the golden file.
    # ----------------------------------------------------------------------
    GOLDEN.mkdir(parents=True, exist_ok=True)
    payload = {
        # No instance UUID: it is generated by the template database, so
        # recording it would make this file differ on every `bootstrap.sh
        # --force`. Nothing reads it.
        "database": db_name,
        "users": {
            name: {
                "id": str(spec["id"]) if spec["id"] is not None else None,
                "anonymous": spec["anonymous"],
            }
            for name, spec in common.USERS.items()
        },
        "workspaces": {"w1": str(common.W1), "w2": str(common.W2), "w3": str(common.W3)},
        "projects": {
            "p1": str(common.P1),
            "p2": str(common.P2),
            "p3": str(common.P3),
            "p_other": str(common.P_OTHER),
        },
        "counts": {
            "users": User.objects.count(),
            "workspace_members": WorkspaceMember.all_objects.count(),
            "project_members": ProjectMember.all_objects.count(),
            "pages": Page.all_objects.count(),
            "issues": Issue.all_objects.count(),
        },
    }
    FIXTURE_JSON.write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n")

    print(f"fixture written to {db_name}")
    for key, value in payload["counts"].items():
        print(f"  {key}: {value}")
    print(f"  recorded in {FIXTURE_JSON}")


if __name__ == "__main__":
    main()
