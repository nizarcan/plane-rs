# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Shared facts about the permissions differential fixture.

Imported by ``fixture.py`` (which creates the rows) and ``oracle.py`` (which
evaluates the real Django permission classes and ``allow_permission``
decorators against them) and by nothing that runs in production.

Every id is ``uuid.UUID(int=n)`` so the database contents and therefore the
golden matrix are byte-stable across rebuilds. The names are descriptive: the
matrix refers to users and contexts by these names, never by UUID.
"""

from __future__ import annotations

import uuid
from typing import Any


def uid(n: int) -> uuid.UUID:
    return uuid.UUID(int=n)


# ---------------------------------------------------------------------------
# Users
# ---------------------------------------------------------------------------
#
# ``anonymous`` models what DRF's ``BaseSessionAuthentication`` (which inherits
# ``rest_framework.authentication.SessionAuthentication.authenticate``) hands to
# the permission classes: an inactive user has ``user.is_active == False``, so
# ``authenticate`` returns ``None`` and ``request.user`` is ``AnonymousUser``
# even though the row exists. That is the only way a user row's ``is_active``
# can affect a permission check; no permission class reads it.
USERS: dict[str, dict[str, Any]] = {
    "user_admin": {"id": uid(1001), "anonymous": False},
    "user_member": {"id": uid(1002), "anonymous": False},
    "user_guest": {"id": uid(1003), "anonymous": False},
    "user_guest_all": {"id": uid(1004), "anonymous": False},
    "user_ws_admin_no_project": {"id": uid(1005), "anonymous": False},
    "user_ws_member_no_project": {"id": uid(1006), "anonymous": False},
    "user_inactive_member": {"id": uid(1007), "anonymous": False},
    "user_deleted_member": {"id": uid(1008), "anonymous": False},
    "user_project_deleted_member": {"id": uid(1009), "anonymous": False},
    "user_nonmember": {"id": uid(1010), "anonymous": False},
    "user_instance_admin": {"id": uid(1011), "anonymous": False},
    "user_instance_admin_low": {"id": uid(1012), "anonymous": False},
    # `InstanceAdminPermission` filters `role__gte=15`; 15 must pass, 14 must not.
    "user_instance_admin_mid": {"id": uid(1015), "anonymous": False},
    "user_inactive": {"id": uid(1013), "anonymous": True},
    "user_w2_admin": {"id": uid(1014), "anonymous": False},
    "anonymous": {"id": None, "anonymous": True},
}

# ---------------------------------------------------------------------------
# Workspaces, projects, pages, and the objects used by creator checks
# ---------------------------------------------------------------------------

W1 = uid(2001)
W2 = uid(2002)
W3 = uid(2003)  # soft-deleted workspace, members left active

W1_SLUG = "perms-ws-main"
W2_SLUG = "perms-ws-other"
W3_SLUG = "perms-ws-deleted"

P1 = uid(3001)  # guest_view_all_features = False
P2 = uid(3002)  # guest_view_all_features = True
P3 = uid(3003)  # soft-deleted project, members left active
P_OTHER = uid(3004)  # a project in W2

P1_IDENT = "PERM1"
P2_IDENT = "PERM2"
P3_IDENT = "PERMDEL"
P_OTHER_IDENT = "PERM3"

PAGE_PUBLIC = uid(4001)  # owned by user_member, access 0, linked to P1
PAGE_PRIVATE_MEMBER = uid(4002)  # owned by user_member, access 1, linked to P1
PAGE_PRIVATE_GUEST = uid(4003)  # owned by user_guest, access 1, linked to P1
PAGE_UNLINKED = uid(4004)  # owned by user_member, no ProjectPage link
PAGE_LINK_DELETED = uid(4005)  # ProjectPage link to P1 is soft-deleted
PAGE_P2 = uid(4006)  # owned by user_member, linked to P2
PAGE_OTHER_WS = uid(4007)  # owned by user_w2_admin, in W2, linked to P_OTHER

ISSUE_GUEST = uid(5001)  # created_by user_guest, in P1
ISSUE_GUEST_DELETED = uid(5002)  # created_by user_guest, in P1, soft-deleted
ISSUE_MEMBER = uid(5003)  # created_by user_member, in P1
ISSUE_P2 = uid(5004)  # created_by user_member, in P2

COMMENT_GUEST = uid(5101)  # created_by user_guest, on ISSUE_GUEST
CYCLE_GUEST = uid(5201)  # created_by user_guest, in P1
MODULE_GUEST = uid(5301)  # created_by user_guest, in P1
VIEW_GUEST = uid(5401)  # created_by user_guest, project P1
VIEW_WS_GUEST = uid(5402)  # created_by user_guest, workspace-level (no project)
DRAFT_GUEST = uid(5501)  # created_by user_guest, project P1
STICKY_GUEST = uid(5601)  # created_by user_guest, in W1
ASSET_GUEST = uid(5701)  # created_by user_guest, in P1, on ISSUE_GUEST

INSTANCE_ADMIN_ADMIN = uid(6001)
INSTANCE_ADMIN_ONLY = uid(6002)
INSTANCE_ADMIN_LOW = uid(6003)
INSTANCE_ADMIN_MID = uid(6005)
INSTANCE_ADMIN_DELETED = uid(6004)  # soft-deleted row for user_member
INSTANCE_DELETED = uid(6101)  # a newer, soft-deleted instance row

# ---------------------------------------------------------------------------
# Matrix contexts
# ---------------------------------------------------------------------------
#
# A context is a URL-kwargs shape plus a representative *real* route used to
# build the request path. The route only matters for two things Django derives
# from the request: ``resolve(path_info).url_name`` (``BaseAPIView.project_id``
# falls back to ``kwargs["pk"]`` when the resolved name is ``"project"``) and
# the view class, which decides whether ``view.project_identifier`` exists.
#
# ``{...}`` placeholders are filled from ``kwargs`` plus ``extra`` (path-only
# values that are not URL kwargs).
CONTEXTS: dict[str, dict[str, Any]] = {
    # Nothing in the URL.
    "blank": {"route": "/api/workspaces/", "kwargs": {}},
    "instance": {"route": "/api/instances/admins/", "kwargs": {}},
    # Workspace by slug.
    "ws": {"route": "/api/workspaces/{slug}/", "kwargs": {"slug": W1_SLUG}},
    "ws_other": {"route": "/api/workspaces/{slug}/", "kwargs": {"slug": W2_SLUG}},
    "ws_deleted": {"route": "/api/workspaces/{slug}/", "kwargs": {"slug": W3_SLUG}},
    # The v1 project collection resolves with url_name == "project" but has no
    # pk, so BaseAPIView.project_id is None.
    "ws_projectname": {"route": "/api/v1/workspaces/{slug}/projects/", "kwargs": {"slug": W1_SLUG}},
    # Workspace by id (FileAssetEndpoint's second URL shape).
    "ws_id": {
        "route": "/api/workspaces/file-assets/{workspace_id}/{asset_key}/",
        "kwargs": {"workspace_id": str(W1), "asset_key": "a"},
    },
    "ws_id_other": {
        "route": "/api/workspaces/file-assets/{workspace_id}/{asset_key}/",
        "kwargs": {"workspace_id": str(W2), "asset_key": "a"},
    },
    # Project by slug + project_id.
    "proj": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/cycles/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1)},
    },
    "proj_p2": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/cycles/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P2)},
    },
    "proj_deleted": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/cycles/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P3)},
    },
    "proj_cross": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/cycles/",
        "kwargs": {"slug": W2_SLUG, "project_id": str(P1)},
    },
    # Project detail by pk, url_name == "project".
    "proj_pk": {
        "route": "/api/v1/workspaces/{slug}/projects/{pk}/",
        "kwargs": {"slug": W1_SLUG, "pk": str(P1)},
    },
    # pk present but the route name is not "project", so project_id is None.
    "proj_pk_other": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
        "kwargs": {"slug": W1_SLUG, "pk": str(P1)},
        "extra": {"project_id": str(P1)},
    },
    # Pages.
    "page_pub": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "page_id": str(PAGE_PUBLIC)},
    },
    "page_priv_member": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "page_id": str(PAGE_PRIVATE_MEMBER)},
    },
    "page_priv_guest": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "page_id": str(PAGE_PRIVATE_GUEST)},
    },
    "page_unlinked": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "page_id": str(PAGE_UNLINKED)},
    },
    "page_link_deleted": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "page_id": str(PAGE_LINK_DELETED)},
    },
    "page_other_ws": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/",
        "kwargs": {"slug": W2_SLUG, "project_id": str(P1), "page_id": str(PAGE_PUBLIC)},
    },
    "page_p2": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P2), "page_id": str(PAGE_P2)},
    },
    # Project identifier (view exposes the ``project_identifier`` property).
    "ident_p1": {
        "route": "/api/v1/workspaces/{slug}/issues/{project_identifier}-{issue_identifier}/",
        "kwargs": {"slug": W1_SLUG, "project_identifier": P1_IDENT, "issue_identifier": "1"},
    },
    "ident_p2": {
        "route": "/api/v1/workspaces/{slug}/issues/{project_identifier}-{issue_identifier}/",
        "kwargs": {"slug": W1_SLUG, "project_identifier": P2_IDENT, "issue_identifier": "1"},
    },
    "ident_cross": {
        "route": "/api/v1/workspaces/{slug}/issues/{project_identifier}-{issue_identifier}/",
        "kwargs": {"slug": W2_SLUG, "project_identifier": P1_IDENT, "issue_identifier": "1"},
    },
    # pk contexts for the creator branch of allow_permission.
    "pk_issue_guest": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "pk": str(ISSUE_GUEST)},
    },
    "pk_issue_deleted": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "pk": str(ISSUE_GUEST_DELETED)},
    },
    "issue_member": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "pk": str(ISSUE_MEMBER)},
    },
    "issue_p2": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P2), "pk": str(ISSUE_P2)},
    },
    "pk_comment_guest": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/",
        "kwargs": {
            "slug": W1_SLUG,
            "project_id": str(P1),
            "issue_id": str(ISSUE_GUEST),
            "pk": str(COMMENT_GUEST),
        },
    },
    "pk_asset_guest": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/",
        "kwargs": {
            "slug": W1_SLUG,
            "project_id": str(P1),
            "issue_id": str(ISSUE_GUEST),
            "pk": str(ASSET_GUEST),
        },
    },
    "pk_cycle_guest": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "pk": str(CYCLE_GUEST)},
    },
    "pk_module_guest": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "pk": str(MODULE_GUEST)},
    },
    "pk_view_guest": {
        "route": "/api/workspaces/{slug}/projects/{project_id}/views/{pk}/",
        "kwargs": {"slug": W1_SLUG, "project_id": str(P1), "pk": str(VIEW_GUEST)},
    },
    "pk_draft_guest": {
        "route": "/api/workspaces/{slug}/draft-issues/{pk}/",
        "kwargs": {"slug": W1_SLUG, "pk": str(DRAFT_GUEST)},
    },
    "pk_sticky_guest": {
        "route": "/api/workspaces/{slug}/stickies/{pk}/",
        "kwargs": {"slug": W1_SLUG, "pk": str(STICKY_GUEST)},
    },
    "pk_view_ws_guest": {
        "route": "/api/workspaces/{slug}/views/{pk}/",
        "kwargs": {"slug": W1_SLUG, "pk": str(VIEW_WS_GUEST)},
    },
}

METHODS = ["GET", "HEAD", "OPTIONS", "POST", "PUT", "PATCH", "DELETE", "TRACE"]

# Contexts every ``allow_permission`` spec is evaluated against. They exercise:
# a normal workspace, a second workspace, a soft-deleted workspace (the join to
# ``workspaces`` does not filter ``deleted_at``), a normal project, the
# guest-unrestricted project, a soft-deleted project, a project id paired with
# the wrong workspace slug, the pk-as-project shape, and two missing-kwarg
# shapes (no slug; slug+pk without project_id).
SPEC_BASE_CONTEXTS = [
    "blank",
    "ws",
    "ws_other",
    "ws_deleted",
    "proj",
    "proj_p2",
    "proj_deleted",
    "proj_cross",
    "proj_pk",
    "proj_pk_other",
]

# Extra contexts for ``creator=True`` specs: one pk per ``model`` used with
# ``creator=True`` anywhere in routes.json, plus the soft-deleted twin of the
# Issue row (proves the ``objects`` manager's ``deleted_at IS NULL``).
SPEC_CREATOR_CONTEXTS = [
    "pk_issue_guest",
    "pk_issue_deleted",
    "pk_comment_guest",
    "pk_asset_guest",
    "pk_cycle_guest",
    "pk_module_guest",
    "pk_view_guest",
    "pk_draft_guest",
    "pk_sticky_guest",
    "pk_view_ws_guest",
]

# Per-class contexts. Chosen from the kwargs shapes the class actually reads,
# taken from the routes that declare it, plus the missing-kwarg and
# cross-workspace shapes that exercise its deny paths.
CLASS_CONTEXTS: dict[str, list[str]] = {
    "rest_framework.permissions.AllowAny": ["blank", "ws", "proj", "proj_pk", "ws_id", "instance"],
    "rest_framework.permissions.IsAuthenticated": ["blank", "ws", "proj", "proj_pk", "ws_id", "instance"],
    "plane.app.permissions.workspace.WorkSpaceBasePermission": [
        "blank",
        "ws",
        "ws_other",
        "ws_deleted",
        "proj",
        "proj_pk",
        "ws_projectname",
    ],
    "plane.app.permissions.workspace.WorkspaceOwnerPermission": [
        "blank",
        "ws",
        "ws_other",
        "ws_deleted",
        "proj",
    ],
    "plane.app.permissions.workspace.WorkSpaceAdminPermission": [
        "blank",
        "ws",
        "ws_other",
        "ws_deleted",
        "proj",
    ],
    "plane.app.permissions.workspace.WorkspaceEntityPermission": [
        "blank",
        "ws",
        "ws_other",
        "ws_deleted",
        "proj",
    ],
    "plane.app.permissions.workspace.WorkspaceViewerPermission": [
        "blank",
        "ws",
        "ws_other",
        "ws_deleted",
    ],
    "plane.app.permissions.workspace.WorkspaceUserPermission": [
        "blank",
        "ws",
        "ws_other",
        "ws_deleted",
    ],
    "plane.app.permissions.workspace.WorkspaceMemberPermission": [
        "blank",
        "ws",
        "ws_other",
        "ws_deleted",
        "ws_id",
        "ws_id_other",
    ],
    "plane.app.permissions.project.ProjectBasePermission": [
        "blank",
        "ws",
        "ws_other",
        "proj",
        "proj_p2",
        "proj_deleted",
        "proj_cross",
        "proj_pk",
        "proj_pk_other",
        "ws_projectname",
    ],
    "plane.app.permissions.project.ProjectArchiveUnarchivePermission": [
        "blank",
        "ws",
        "ws_other",
        "proj",
        "proj_p2",
        "proj_deleted",
        "proj_cross",
        "proj_pk",
        "proj_pk_other",
        "ws_projectname",
    ],
    "plane.app.permissions.project.ProjectMemberPermission": [
        "blank",
        "ws",
        "proj",
        "proj_p2",
        "proj_deleted",
        "proj_cross",
        "proj_pk",
        "proj_pk_other",
        "ws_projectname",
    ],
    "plane.app.permissions.project.ProjectEntityPermission": [
        "blank",
        "ws",
        "proj",
        "proj_p2",
        "proj_deleted",
        "proj_cross",
        "proj_pk",
        "proj_pk_other",
        "ws_projectname",
        "ident_p1",
        "ident_p2",
        "ident_cross",
    ],
    "plane.app.permissions.project.ProjectAdminPermission": [
        "blank",
        "ws",
        "ws_other",
        "proj",
        "proj_p2",
        "proj_deleted",
        "proj_cross",
        "proj_pk",
        "proj_pk_other",
        "ws_projectname",
    ],
    "plane.app.permissions.project.ProjectLitePermission": [
        "blank",
        "ws",
        "proj",
        "proj_p2",
        "proj_deleted",
        "proj_cross",
        "proj_pk",
        "proj_pk_other",
    ],
    "plane.app.permissions.page.ProjectPagePermission": [
        "blank",
        "ws",
        "proj",
        "proj_p2",
        "proj_deleted",
        "proj_cross",
        "page_pub",
        "page_priv_member",
        "page_priv_guest",
        "page_unlinked",
        "page_link_deleted",
        "page_other_ws",
        "page_p2",
    ],
    "plane.license.api.permissions.instance.InstanceAdminPermission": [
        "blank",
        "ws",
        "proj",
        "instance",
    ],
}

# The utils copies are byte-identical to the app ones except for the two classes
# that only exist in app (WorkspaceMemberPermission,
# ProjectArchiveUnarchivePermission) and ``issue_hidden_from_guest`` (app only).
for _app, _utils in (
    ("plane.app.permissions.workspace.WorkSpaceBasePermission", "plane.utils.permissions.workspace.WorkSpaceBasePermission"),
    ("plane.app.permissions.workspace.WorkspaceOwnerPermission", "plane.utils.permissions.workspace.WorkspaceOwnerPermission"),
    ("plane.app.permissions.workspace.WorkSpaceAdminPermission", "plane.utils.permissions.workspace.WorkSpaceAdminPermission"),
    ("plane.app.permissions.workspace.WorkspaceEntityPermission", "plane.utils.permissions.workspace.WorkspaceEntityPermission"),
    ("plane.app.permissions.workspace.WorkspaceViewerPermission", "plane.utils.permissions.workspace.WorkspaceViewerPermission"),
    ("plane.app.permissions.workspace.WorkspaceUserPermission", "plane.utils.permissions.workspace.WorkspaceUserPermission"),
    ("plane.app.permissions.project.ProjectBasePermission", "plane.utils.permissions.project.ProjectBasePermission"),
    ("plane.app.permissions.project.ProjectMemberPermission", "plane.utils.permissions.project.ProjectMemberPermission"),
    ("plane.app.permissions.project.ProjectEntityPermission", "plane.utils.permissions.project.ProjectEntityPermission"),
    ("plane.app.permissions.project.ProjectAdminPermission", "plane.utils.permissions.project.ProjectAdminPermission"),
    ("plane.app.permissions.project.ProjectLitePermission", "plane.utils.permissions.project.ProjectLitePermission"),
    ("plane.app.permissions.page.ProjectPagePermission", "plane.utils.permissions.page.ProjectPagePermission"),
):
    CLASS_CONTEXTS[_utils] = CLASS_CONTEXTS[_app]

# The two decorator copies are identical (proved by ``diff``); every route uses
# the app one. ``allow_permission`` never reads the request method, so all
# methods are evaluated to prove that.
SPEC_ROLES = {
    "ADMIN": 20,
    "MEMBER": 15,
    "GUEST": 5,
}

# ``issue_hidden_from_guest`` contexts: the guest's own issue, another member's
# issue, a soft-deleted issue of the guest's, and an issue in the unrestricted
# project.
HELPER_CONTEXTS = ["pk_issue_guest", "issue_member", "pk_issue_deleted", "issue_p2"]

# Views whose ``get_permissions`` is dynamic (``dynamic_permissions: true`` in
# routes.json), as (owner dotted path, route context, actions to evaluate).
DYNAMIC_VIEWS: list[dict[str, Any]] = [
    {
        "view": "plane.app.views.issue.subscriber.IssueSubscriberViewSet",
        "context": "proj",
        "pairs": [
            ("GET", "list"),
            ("POST", "create"),
            ("DELETE", "destroy"),
            ("GET", "subscription_status"),
            ("POST", "subscribe"),
            ("DELETE", "unsubscribe"),
            ("PATCH", "update"),
        ],
    },
    {
        "view": "plane.space.views.issue.IssueCommentPublicViewSet",
        "context": "blank",
        "pairs": [
            ("GET", "list"),
            ("GET", "retrieve"),
            ("POST", "create"),
            ("PATCH", "partial_update"),
            ("DELETE", "destroy"),
            ("OPTIONS", None),
            ("HEAD", None),
        ],
    },
    {
        "view": "plane.space.views.asset.EntityAssetEndpoint",
        "context": "blank",
        "pairs": [("GET", None), ("POST", None), ("PATCH", None), ("DELETE", None), ("OPTIONS", None)],
    },
    {
        "view": "plane.license.api.views.instance.InstanceEndpoint",
        "context": "instance",
        "pairs": [("GET", None), ("PATCH", None), ("POST", None), ("OPTIONS", None)],
    },
    {
        "view": "plane.api.views.issue.LabelListCreateAPIEndpoint",
        "context": "proj",
        "pairs": [("GET", None), ("HEAD", None), ("OPTIONS", None), ("POST", None), ("PATCH", None), ("DELETE", None)],
    },
    {
        "view": "plane.api.views.member.ProjectMemberListCreateAPIEndpoint",
        "context": "proj",
        "pairs": [("GET", None), ("HEAD", None), ("OPTIONS", None), ("POST", None), ("PATCH", None), ("DELETE", None)],
    },
    {
        "view": "plane.api.views.state.StateListCreateAPIEndpoint",
        "context": "proj",
        "pairs": [("GET", None), ("HEAD", None), ("OPTIONS", None), ("POST", None), ("PATCH", None), ("DELETE", None)],
    },
    {
        # Unlike the label and member detail endpoints, this one defines its own
        # `get_permissions` (`plane/api/views/state.py:176-179`), so
        # `routes.json` records it as the owner rather than the list endpoint.
        "view": "plane.api.views.state.StateDetailAPIEndpoint",
        "context": "proj",
        "pairs": [("GET", None), ("HEAD", None), ("OPTIONS", None), ("POST", None), ("PATCH", None), ("DELETE", None)],
    },
]
