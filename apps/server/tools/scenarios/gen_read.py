#!/usr/bin/env python3
# Copyright (c) 2023-present Plane Software, Inc. and contributors
# SPDX-License-Identifier: AGPL-3.0-only
# See the LICENSE file for details.

"""Generate the read (GET) differential scenarios.

Every GET route in `apps/server/routes.json` becomes at least one step in one
file per group from `deploy/route-groups.yaml`. Each step binds the route's URL
kwargs to objects from the fixture template (`plane_fix_tpl`, built by
`tools/fixtures/build.py`), authenticates as a fixture user, and carries query
parameters the view actually reads (grepped from the view's
`request.GET`/`query_params` usage and from `plane/utils/issue_filters.py`).

What the generated file looks like
----------------------------------
* `template_db: plane_fix_tpl` -- the rich fixture template, so a GET returns
  real, varied rows instead of an empty list.
* `vars:` -- every fixture id/slug/session the steps reference, copied from
  `scenarios/fixtures/fix_tpl.json`. Re-run the generator after rebuilding the
  fixture.
* `steps:` -- one request per (route, user, query variant). Sessions are sent
  as an explicit `Cookie` header instead of a sign-in flow: the fixture already
  contains signed session rows, and a read scenario must not perform writes.
  Anonymous requests carry a distinct `X-Forwarded-For` so the anonymous
  throttle bucket is exercised per client.

The header of every file says it is generated. Re-run:

    apps/api/.venv/bin/python apps/server/tools/scenarios/gen_read.py

Then, for each group:

    cd apps/server
    ./target/debug/difftest run --scenario scenarios/read/<group>.yaml --a django --b django
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import re
import sys
from collections import OrderedDict
from pathlib import Path
from typing import Any

SCRIPT = Path(__file__).resolve()
SERVER_DIR = SCRIPT.parents[2]
REPO_ROOT = SCRIPT.parents[4]
ROUTES_JSON = SERVER_DIR / "routes.json"
GROUPS_FILE = SERVER_DIR / "deploy" / "route-groups.yaml"
MANIFEST = SERVER_DIR / "scenarios" / "fixtures" / "fix_tpl.json"
OUT_DIR = SERVER_DIR / "scenarios" / "read"

# Every request carries this Host so host-derived values (cache keys, S3
# signatures, API-token log messages) are identical on both targets.
FIXED_HOST = "plane-fixture.test"
GEN_CADDY = SERVER_DIR / "tools" / "proxy" / "gen_caddy.py"

# ---------------------------------------------------------------------------
# Request identities
# ---------------------------------------------------------------------------

# name -> (header name, value prefix, fixture var). `None` means anonymous.
# The session cookies are `session-id` for users and `admin-session-id` for the
# instance-admin surface; both hold a key from the same `sessions` table.
IDENTITIES: dict[str, tuple[str, str, str] | None] = {
    "anon": None,
    "admin": ("Cookie", "session-id=", "session_admin"),
    "member": ("Cookie", "session-id=", "session_member"),
    "member2": ("Cookie", "session-id=", "session_member2"),
    "guest": ("Cookie", "session-id=", "session_guest"),
    "guest2": ("Cookie", "session-id=", "session_guest2"),
    "nonmember": ("Cookie", "session-id=", "session_nonmember"),
    "onboarded": ("Cookie", "session-id=", "session_onboarded"),
    "tz_kolkata": ("Cookie", "session-id=", "session_tz_kolkata"),
    "tz_newyork": ("Cookie", "session-id=", "session_tz_newyork"),
    "tz_berlin": ("Cookie", "session-id=", "session_tz_berlin"),
    "tz_tokyo": ("Cookie", "session-id=", "session_tz_tokyo"),
    "no_workspace": ("Cookie", "session-id=", "session_no_workspace"),
    "deleted_member": ("Cookie", "session-id=", "session_deleted_member"),
    "invitee": ("Cookie", "session-id=", "session_invitee"),
    # The instance-admin cookie is the same session store behind a different
    # cookie name; the session middleware selects it when "instances" appears
    # in the request path (plane/authentication/middleware/session.py).
    "instance_admin": ("Cookie", "admin-session-id=", "session_instance_admin"),
    "api_admin": ("X-API-Key", "", "api_token_active"),
    "api_member": ("X-API-Key", "", "api_token_member"),
    "api_expired": ("X-API-Key", "", "api_token_expired"),
    "api_admin2": ("X-API-Key", "", "api_token_admin2"),
    "api_admin3": ("X-API-Key", "", "api_token_admin3"),
    "api_member2": ("X-API-Key", "", "api_token_member2"),
    "api_member3": ("X-API-Key", "", "api_token_member3"),
}

# The v1 throttle keys on the token, so rotate tokens of the same role across
# routes: one read scenario issues more than 60 requests per token per minute.
API_ROTATION = {
    "api_admin": ["api_admin", "api_admin2", "api_admin3"],
    "api_member": ["api_member", "api_member2", "api_member3"],
}

WS_USERS = ["admin", "member", "guest", "nonmember"]
PROJECT_USERS = ["admin", "member", "guest", "onboarded"]
PROJECT_RESTRICTED_USERS = ["admin", "member", "guest2", "nonmember"]

# ---------------------------------------------------------------------------
# Query parameter values
# ---------------------------------------------------------------------------

# Every value is either a literal or `{name}`/`{{name}}` for a fixture variable
# (the fixture var syntax in a scenario is `{{name}}`; a bare `{name}` is
# rewritten by `render_query`).
P: dict[str, str] = {
    "fields": "id,name",
    "fields_issues": "id,name,state_id,priority,assignees,labels",
    "expand": "state,assignees,labels",
    "expand_issue": "state,assignees,labels,project",
    "per_page": "2",
    "cursor": "2:1:0",
    "order_by_created": "-created_at",
    "order_by_updated": "-updated_at",
    "order_by_name": "name",
    "order_by_sequence": "-sequence_id",
    "group_by_state": "state_id",
    "group_by_priority": "priority",
    "group_by_assignee": "assignees__id",
    "group_by_labels": "labels__id",
    "group_by_created_by": "created_by",
    "group_by_cycle": "cycle_id",
    "group_by_module": "issue_module__module_id",
    "sub_group_by_priority": "priority",
    "group_by_state_group": "state__group",
    "state": "{state_alpha_todo}",
    "state_in": "{state_alpha_todo},{state_alpha_done}",
    "state_group": "started",
    "priority": "high",
    "priority_in": "urgent,high",
    "assignees": "{user_member_id}",
    "labels": "{label_alpha_bug}",
    "created_by": "{user_admin_id}",
    "mentions": "{user_member_id}",
    "subscriber": "{user_member_id}",
    "cycle": "{cycle_alpha_current}",
    "module": "{module_alpha_in_progress}",
    "type_active": "active",
    "type_all": "all",
    "search": "cache",
    "query": "cache",
    "name": "Cache",
    "project": "{project_alpha}",
    "projects": "{project_alpha},{project_beta}",
    "issues": "{issue_alpha},{issue_alpha_child}",
    "parent_none": "None",
    "parent_issue": "{issue_alpha}",
    "sub_issue": "true",
    "is_archived": "true",
    "estimate_point": "{estimate_point_alpha_points_3}",
    "intake_status": "1",
    "inbox_status": "1",
    "start_date_after": "after;{date_past_date}",
    "target_date_before": "before;{date_future_date}",
    "created_at_range": "{date_issue_created_range}",
    "created_at_day": "{date_issue_alpha_created_day}",
    "updated_at_day": "{date_issue_alpha_created_day}",
    "completed_at_range": "{date_issue_created_range}",
    "month": "3",
    "cycle_view": "all",
    "cycle_view_current": "current",
    "grouped": "true",
    "read": "true",
    "archived": "true",
    "snoozed": "true",
    "mentioned": "true",
    "notification_type": "issue",
    "status_pending": "-2",
    "status_accepted": "1",
    "activity_type": "issue",
    "updated_at_gt": "2026-01-01T00:00:00Z",
    "track_visit": "false",
    "action_view": "view",
    "action_edit": "edit",
    "page_action_public": "make-public",
    "entity_name_project": "PROJECT",
    "entities": "project,issue,cycle,module,page",
    "query_type": "issue",
    "workspace_search": "true",
    "count": "true",
    "include_archived": "true",
    "x_axis": "state_id",
    "y_axis": "issue_count",
    "segment": "priority",
    "date_filter": "last_3_months",
    "tab": "overview",
    "analytics_type": "work-items",
    "slug_check": "fix-available-slug",
    "search_instances": "Fixture",
    "external_source": "fixture",
    "external_id": "ext-alpha:000",
    "limit": "2",
    "next_path": "/fix-workspace/",
    "code": "fixture-code",
    "state_code": "fixture-state",
    "description": "true",
    "show_sub_issues": "true",
    "entity_type": "PROJECT",
    "project_ids": "{project_alpha}",
    "cycle_id_q": "{cycle_alpha_current}",
    "module_id_q": "{module_alpha_in_progress}",
    "asset_id": "{attachment_id}",
    "unsplash_query": "plane",
    "per_page_unsplash": "1",
    "page_unsplash": "1",
}

# ---------------------------------------------------------------------------
# Per-route overrides
# ---------------------------------------------------------------------------

USERS: dict[str, list[str]] = {
    # auth / health
    "/": ["anon"],
    "/robots.txt": ["anon"],
    "/auth/get-csrf-token/": ["anon"],
    # instance admin surface
    "/api/instances/admins/session/": ["anon", "instance_admin", "member"],
    "/api/instances/workspaces/": ["instance_admin", "member", "anon"],
    # users-me: only the caller has data at these paths
    "/api/users/me/": ["admin", "member", "guest"],
    "/api/users/session/": ["admin", "member", "anon"],
    "/api/users/me/settings/": ["admin", "member"],
    "/api/users/me/profile/": ["admin", "member"],
    "/api/users/me/accounts/": ["admin", "member"],
    "/api/users/me/accounts/{pk}/": ["admin", "member"],
    "/api/users/me/instance-admin/": ["admin", "member", "anon"],
    "/api/users/me/activities/": ["admin", "member", "onboarded"],
    "/api/users/me/workspaces/": ["admin", "member", "no_workspace"],
    "/api/users/me/workspaces/{slug}/activity-graph/": ["admin", "member", "guest"],
    "/api/users/me/workspaces/{slug}/issues-completed-graph/": ["admin", "member"],
    "/api/users/me/workspaces/{slug}/dashboard/": ["admin", "member"],
    "/api/users/me/workspaces/invitations/": ["invitee", "admin", "no_workspace"],
    "/api/users/me/workspaces/{slug}/project-roles/": ["admin", "member", "guest"],
    "/api/users/me/workspaces/{slug}/projects/invitations/": ["invitee", "admin", "member"],
    "/api/users/last-visited-workspace/": ["admin", "member", "no_workspace"],
    # workspace membership
    "/api/workspaces/": ["admin", "member", "no_workspace"],
    "/api/workspace-slug-check/": ["admin", "anon"],
    "/api/workspaces/{slug}/": ["admin", "member", "guest", "nonmember"],
    "/api/workspaces/{slug}/members/": ["admin", "member", "guest", "nonmember"],
    "/api/workspaces/{slug}/members/{pk}/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/project-members/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/workspace-members/me/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/invitations/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/invitations/{pk}/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/invitations/{pk}/join/": ["invitee", "admin"],
    "/api/workspaces/{slug}/workspace-themes/": ["admin", "member"],
    "/api/workspaces/{slug}/workspace-themes/{pk}/": ["admin", "member"],
    "/api/workspaces/{slug}/user-stats/{user_id}/": ["admin", "member"],
    "/api/workspaces/{slug}/user-activity/{user_id}/": ["admin", "member"],
    "/api/workspaces/{slug}/user-profile/{user_id}/": ["admin", "member"],
    "/api/workspaces/{slug}/user-issues/{user_id}/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/user-properties/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/user-favorites/": ["admin", "member"],
    "/api/workspaces/{slug}/user-favorites/{favorite_id}/": ["admin", "member"],
    "/api/workspaces/{slug}/user-favorites/{favorite_id}/group/": ["admin", "member"],
    "/api/workspaces/{slug}/quick-links/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/quick-links/{pk}/": ["admin", "member"],
    "/api/workspaces/{slug}/home-preferences/": ["admin", "member"],
    "/api/workspaces/{slug}/home-preferences/{key}/": ["admin", "member"],
    "/api/workspaces/{slug}/recent-visits/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/sidebar-preferences/": ["admin", "member"],
    "/api/workspaces/{slug}/stickies/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/stickies/{pk}/": ["admin", "member"],
    "/api/workspaces/{slug}/webhooks/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/webhooks/{pk}/": ["admin", "member"],
    "/api/workspaces/{slug}/webhook-logs/{webhook_id}/": ["admin", "member"],
    "/api/workspaces/{slug}/labels/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/states/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/estimates/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/modules/": ["admin", "member"],
    "/api/workspaces/{slug}/cycles/": ["admin", "member"],
    "/api/workspaces/{slug}/draft-issues/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/draft-issues/{pk}/": ["admin", "member"],
    "/api/workspaces/{slug}/views/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/views/{pk}/": ["admin", "member"],
    "/api/workspaces/{slug}/issues/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/export-issues/": ["admin", "member"],
    "/api/workspaces/{slug}/pages/{page_id}/live-access/": ["admin", "member", "guest"],
    # analytics
    "/api/workspaces/{slug}/analytics/": ["admin", "member"],
    "/api/workspaces/{slug}/analytic-view/": ["admin", "member"],
    "/api/workspaces/{slug}/analytic-view/{pk}/": ["admin", "member"],
    "/api/workspaces/{slug}/saved-analytic-view/{analytic_id}/": ["admin", "member"],
    "/api/workspaces/{slug}/default-analytics/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/project-stats/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/advance-analytics/": ["admin", "member"],
    "/api/workspaces/{slug}/advance-analytics-stats/": ["admin", "member"],
    "/api/workspaces/{slug}/advance-analytics-charts/": ["admin", "member"],
    # notifications
    "/api/workspaces/{slug}/users/notifications/": ["member", "admin", "guest"],
    "/api/workspaces/{slug}/users/notifications/{pk}/": ["member", "admin"],
    "/api/workspaces/{slug}/users/notifications/unread/": ["member", "admin"],
    "/api/users/me/notification-preferences/": ["admin", "member", "guest"],
    # pages
    "/api/workspaces/{slug}/projects/{project_id}/pages/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/projects/{project_id}/pages-summary/": ["admin", "member"],
    "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/description/": ["admin", "member"],
    "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/": ["admin", "member"],
    "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/{pk}/": ["admin", "member"],
    # projects
    "/api/workspaces/{slug}/projects/": ["admin", "member", "guest", "nonmember"],
    "/api/workspaces/{slug}/projects/details/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/projects/{pk}/": ["admin", "member", "guest", "nonmember"],
    "/api/workspaces/{slug}/projects/{project_id}/invitations/": ["admin", "member"],
    "/api/workspaces/{slug}/projects/{project_id}/invitations/{pk}/": ["admin", "member"],
    "/api/workspaces/{slug}/projects/{project_id}/join/{pk}/": ["invitee", "admin"],
    "/api/workspaces/{slug}/projects/{project_id}/members/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/projects/{project_id}/members/{pk}/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/projects/{project_id}/project-members/me/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/projects/{project_id}/preferences/member/{member_id}/": ["admin", "member"],
    "/api/workspaces/{slug}/project-identifiers/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/user-favorite-projects/": ["admin", "member"],
    "/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/{pk}/": ["admin", "member"],
    # search
    "/api/workspaces/{slug}/search/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/entity-search/": ["admin", "member", "guest"],
    "/api/workspaces/{slug}/projects/{project_id}/search-issues/": ["admin", "member", "guest"],
    # assets
    "/api/workspaces/{slug}/file-assets/": ["admin", "member"],
    "/api/workspaces/file-assets/{workspace_id}/{asset_key}/": ["admin", "member"],
    "/api/users/file-assets/": ["admin", "member"],
    "/api/users/file-assets/{asset_key}/": ["admin", "member"],
    "/api/assets/v2/workspaces/{slug}/": ["admin", "member"],
    "/api/assets/v2/workspaces/{slug}/{asset_id}/": ["admin", "member"],
    "/api/assets/v2/static/{asset_id}/": ["admin", "member", "anon"],
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/": ["admin", "member"],
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/": ["admin", "member"],
    "/api/assets/v2/workspaces/{slug}/check/{asset_id}/": ["admin", "member"],
    "/api/assets/v2/workspaces/{slug}/download/{asset_id}/": ["admin", "member"],
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/download/{asset_id}/": ["admin", "member"],
    # auth oauth: the providers are unconfigured in the fixture instance
    "/auth/google/": ["anon"],
    "/auth/google/callback/": ["anon"],
    "/auth/github/": ["anon"],
    "/auth/github/callback/": ["anon"],
    "/auth/gitlab/": ["anon"],
    "/auth/gitlab/callback/": ["anon"],
    "/auth/gitea/": ["anon"],
    "/auth/gitea/callback/": ["anon"],
    # v1: API key auth
    # The duplicate api-root (the invitations router wins) authenticates with a
    # session, not an API key: an API-key request is 401 (verified against
    # Django), a session returns the route map.
    "/api/v1/workspaces/{slug}/": ["admin", "api_admin", "anon"],
    "/api/v1/workspaces/{slug}/.{format}": ["admin", "api_admin", "anon"],
    "/api/v1/users/me/": ["api_admin", "api_member", "api_expired", "anon"],
    # public (space) API: session auth, project guests are restricted
    "/api/public/workspaces/{slug}/project-boards/": ["admin", "member", "anon"],
    "/api/public/workspaces/{slug}/projects/{project_id}/anchor/": ["admin", "member", "anon"],
}

# Additional query variants emitted only for the first identity of the route,
# so the full parameter sweep does not multiply by every user.
EXTRA: dict[str, list[dict[str, str]]] = {}

# Fixture bindings per path (overrides the generic kwarg resolver). Values are
# fixture variable names.
BIND: dict[str, dict[str, str]] = {}

# Why a route cannot produce a 200 with data (recorded in COVERAGE.md).

def bind(**kwargs: str) -> dict[str, str]:
    return dict(kwargs)


BIND.update(
    {
        "/api/workspaces/{slug}/projects/{pk}/": bind(pk="project_alpha"),
        "/api/workspaces/{slug}/projects/{project_id}/archived-cycles/{pk}/": bind(pk="cycle_alpha_archived"),
        "/api/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/": bind(pk="module_alpha_archived"),
        "/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/": bind(pk="cycle_alpha_current"),
        "/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/": bind(pk="module_alpha_in_progress"),
        "/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/": bind(pk="intake_alpha"),
        "/api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/": bind(
            work_item_id="issue_alpha"
        ),
        "/api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/{pk}/": bind(
            work_item_id="issue_alpha_commented", pk="issue_description_version_0"
        ),
        "/api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/": bind(pk="intake_alpha"),
        "/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/": bind(
            pk="intake_issue_alpha_pending_issue"
        ),
        "/api/workspaces/{slug}/projects/{project_id}/inbox-issues/{pk}/": bind(
            pk="intake_issue_alpha_accepted_issue"
        ),
        "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{issue_id}/": bind(
            issue_id="intake_issue_alpha_pending_issue"
        ),
        "/api/workspaces/{slug}/projects/{project_id}/views/{pk}/": bind(pk="view_alpha_high"),
        "/api/workspaces/{slug}/views/{pk}/": bind(pk="view_ws_public"),
        "/api/workspaces/{slug}/projects/{project_id}/states/{pk}/": bind(pk="state_alpha_todo"),
        "/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/": bind(pk="label_alpha_bug"),
        "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/": bind(pk="issue_alpha"),
        "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/archive/": bind(pk="issue_alpha_archived"),
        "/api/workspaces/{slug}/projects/{project_id}/members/{pk}/": bind(pk="project_member_alpha_member"),
        "/api/workspaces/{slug}/members/{pk}/": bind(pk="workspace_member_ws_member"),
        "/api/workspaces/{slug}/invitations/{pk}/": bind(pk="workspace_invite_id"),
        "/api/workspaces/{slug}/invitations/{pk}/join/": bind(pk="workspace_invite_id"),
        "/api/workspaces/{slug}/projects/{project_id}/invitations/{pk}/": bind(pk="project_invite_id"),
        "/api/workspaces/{slug}/projects/{project_id}/join/{pk}/": bind(pk="project_invite_id"),
        "/api/workspaces/{slug}/users/notifications/{pk}/": bind(pk="notification_unread_issue"),
        "/api/workspaces/{slug}/webhooks/{pk}/": bind(pk="webhook_active"),
        "/api/workspaces/{slug}/stickies/{pk}/": bind(pk="sticky_0"),
        "/api/workspaces/{slug}/workspace-themes/{pk}/": bind(pk="workspace_theme_0"),
        "/api/workspaces/{slug}/quick-links/{pk}/": bind(pk="quick_link_0"),
        "/api/workspaces/{slug}/user-favorites/{favorite_id}/": bind(favorite_id="favorite_project"),
        "/api/workspaces/{slug}/user-favorites/{favorite_id}/group/": bind(favorite_id="favorite_folder"),
        "/api/workspaces/{slug}/analytic-view/{pk}/": bind(pk="analytic_view_default"),
        "/api/workspaces/{slug}/saved-analytic-view/{analytic_id}/": bind(analytic_id="analytic_view_default"),
        "/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/{pk}/": bind(pk="deploy_board_alpha"),
        "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/": bind(page_id="page_project"),
        "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/description/": bind(page_id="page_project"),
        "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/": bind(page_id="page_child"),
        "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/{pk}/": bind(
            page_id="page_child", pk="page_version_child_0"
        ),
        "/api/workspaces/{slug}/pages/{page_id}/live-access/": bind(page_id="page_public"),
        "/api/users/me/accounts/{pk}/": bind(pk="account_admin"),
        "/api/users/api-tokens/{pk}/": bind(pk="api_token_active_id"),
        "/api/instances/admins/{pk}/": bind(pk="instance_admin_member"),
        "/api/workspaces/{slug}/home-preferences/{key}/": bind(key="=quick_links"),
        "/api/workspaces/{slug}/draft-issues/{pk}/": bind(pk="draft_admin_alpha"),
        "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="issue_version_0"
        ),
        "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/{pk}/": bind(
            work_item_id="issue_alpha_commented", pk="issue_description_version_0"
        ),
        "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="issue_link_design"
        ),
        "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/{pk}/": bind(
            module_id="module_alpha_in_progress", pk="module_link_0"
        ),
        "/api/assets/v2/workspaces/{slug}/{asset_id}/": bind(asset_id="workspace_logo_asset"),
        "/api/assets/v2/static/{asset_id}/": bind(asset_id="avatar_asset_admin"),
        "/api/assets/v2/workspaces/{slug}/check/{asset_id}/": bind(asset_id="attachment_id"),
        "/api/assets/v2/workspaces/{slug}/download/{asset_id}/": bind(asset_id="attachment_id"),
        "/api/assets/v2/workspaces/{slug}/projects/{project_id}/download/{asset_id}/": bind(
            asset_id="attachment_id"
        ),
        "/api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/": bind(pk="attachment_id"),
        "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/": bind(issue_id="issue_alpha_commented"),
        "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="comment_alpha"
        ),
        "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/": bind(
            issue_id="issue_alpha_commented"
        ),
        "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="attachment_id"
        ),
        "/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="attachment_id"
        ),
        "/api/workspaces/file-assets/{workspace_id}/{asset_key}/": bind(
            workspace_id="ws_id", asset_key="attachment_asset_key"
        ),
        "/api/users/file-assets/{asset_key}/": bind(asset_key="avatar_asset_key_admin"),
        "/api/workspaces/{slug}/webhook-logs/{webhook_id}/": bind(webhook_id="webhook_active"),
        "/api/workspaces/{slug}/user-stats/{user_id}/": bind(user_id="user_member_id"),
        "/api/workspaces/{slug}/user-activity/{user_id}/": bind(user_id="user_member_id"),
        "/api/workspaces/{slug}/user-profile/{user_id}/": bind(user_id="user_member_id"),
        "/api/workspaces/{slug}/user-issues/{user_id}/": bind(user_id="user_member_id"),
        "/api/workspaces/{slug}/projects/{project_id}/preferences/member/{member_id}/": bind(
            member_id="user_member_id"
        ),
        "/api/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/": bind(
            project_identifier="project_alpha_identifier", issue_identifier="issue_alpha_child_sequence"
        ),
        "/api/v1/workspaces/{slug}/issues/{project_identifier}-{issue_identifier}/": bind(
            project_identifier="project_alpha_identifier", issue_identifier="issue_alpha_child_sequence"
        ),
        "/api/v1/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/": bind(
            project_identifier="project_alpha_identifier", issue_identifier="issue_alpha_child_sequence"
        ),
        "/api/v1/workspaces/{slug}/projects/{pk}/": bind(pk="project_alpha"),
        "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{pk}/": bind(pk="cycle_alpha_current"),
        "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/": bind(pk="module_alpha_in_progress"),
        "/api/v1/workspaces/{slug}/projects/{project_id}/labels/{pk}/": bind(pk="label_alpha_bug"),
        "/api/v1/workspaces/{slug}/projects/{project_id}/members/{pk}/": bind(pk="project_member_alpha_member"),
        "/api/v1/workspaces/{slug}/projects/{project_id}/project-members/{pk}/": bind(
            pk="project_member_alpha_member"
        ),
        "/api/v1/workspaces/{slug}/projects/{project_id}/states/{state_id}/": bind(state_id="state_alpha_todo"),
        "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/": bind(
            issue_id="issue_in_current_cycle"
        ),
        "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/": bind(pk="issue_alpha"),
        "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="issue_link_design"
        ),
        "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="comment_alpha"
        ),
        "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="issue_activity_0"
        ),
        "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="attachment_id"
        ),
        "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/": bind(pk="issue_alpha"),
        "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="issue_link_design"
        ),
        "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="comment_alpha"
        ),
        "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="issue_activity_0"
        ),
        "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="attachment_id"
        ),
        "/api/v1/workspaces/{slug}/invitations/{pk}/": bind(pk="workspace_invite_id"),
        "/api/v1/workspaces/{slug}/invitations/{pk}.{format}": bind(pk="workspace_invite_id"),
        "/api/v1/workspaces/{slug}/stickies/{pk}.{format}": bind(pk="sticky_0"),
        "/api/v1/workspaces/{slug}/stickies/{pk}/": bind(pk="sticky_0"),
        "/api/v1/workspaces/{slug}/assets/{asset_id}/": bind(asset_id="attachment_id"),
        "/api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/{pk}/": bind(
            intake_id="intake_alpha", pk="intake_issue_alpha_pending"
        ),
        "/api/public/anchor/{anchor}/issues/{issue_id}/": bind(issue_id="issue_alpha_commented"),
        "/api/public/anchor/{anchor}/issues/{issue_id}/comments/": bind(issue_id="issue_alpha_commented"),
        "/api/public/anchor/{anchor}/issues/{issue_id}/comments/{pk}/": bind(
            issue_id="issue_alpha_commented", pk="comment_alpha"
        ),
        "/api/public/anchor/{anchor}/issues/{issue_id}/reactions/": bind(issue_id="issue_alpha_commented"),
        "/api/public/anchor/{anchor}/comments/{comment_id}/reactions/": bind(comment_id="comment_alpha"),
        "/api/public/anchor/{anchor}/issues/{issue_id}/votes/": bind(issue_id="issue_alpha_commented"),
        "/api/public/assets/v2/anchor/{anchor}/{pk}/": bind(pk="description_asset_0"),
    }
)

# Query variants, keyed by exact path template. `[]` means the route takes no
# query parameters.
QUERIES: dict[str, list[dict[str, str]]] = {
    # --- meta / health -----------------------------------------------------
    "/": [{}],
    "/robots.txt": [{}],
    "/api/timezones/": [{}],
    "/api/unsplash/": [{}, {"query": P["unsplash_query"], "per_page": P["per_page_unsplash"], "page": P["page_unsplash"]}],
    "/auth/get-csrf-token/": [{}],
    # --- instances ---------------------------------------------------------
    "/api/instances/": [{}],
    "/api/instances/admins/": [{}],
    "/api/instances/admins/me/": [{}],
    "/api/instances/admins/session/": [{}],
    "/api/instances/admins/{pk}/": [{}],
    "/api/instances/configurations/": [{}],
    "/api/instances/workspace-slug-check/": [{"slug": P["slug_check"]}, {}],
    "/api/instances/workspaces/": [{}, {"search": P["search_instances"]}],
    # --- users-me ----------------------------------------------------------
    "/api/users/me/": [{}],
    "/api/users/session/": [{}],
    "/api/users/me/settings/": [{}],
    "/api/users/me/profile/": [{}],
    "/api/users/me/accounts/": [{}],
    "/api/users/me/accounts/{pk}/": [{}],
    "/api/users/me/instance-admin/": [{}],
    "/api/users/me/activities/": [{}, {"order_by": P["order_by_created"]}, {"per_page": P["per_page"]}],
    "/api/users/me/workspaces/": [{}],
    "/api/users/me/workspaces/{slug}/activity-graph/": [{}, {"month": P["month"]}],
    "/api/users/me/workspaces/{slug}/issues-completed-graph/": [{}, {"month": P["month"]}],
    "/api/users/me/workspaces/{slug}/dashboard/": [{}, {"month": P["month"]}],
    "/api/users/me/workspaces/invitations/": [{}],
    "/api/users/me/workspaces/{slug}/project-roles/": [{}],
    "/api/users/me/workspaces/{slug}/projects/invitations/": [{}],
    "/api/users/last-visited-workspace/": [{}],
    "/api/users/api-tokens/": [{}],
    "/api/users/api-tokens/{pk}/": [{}],
    "/api/users/me/notification-preferences/": [{}],
    # --- workspaces --------------------------------------------------------
    "/api/workspaces/": [{}],
    "/api/workspace-slug-check/": [{}, {"slug": P["slug_check"]}],
    "/api/workspaces/{slug}/": [{}],
    "/api/workspaces/{slug}/members/": [{}],
    "/api/workspaces/{slug}/members/{pk}/": [{}],
    "/api/workspaces/{slug}/project-members/": [{}],
    "/api/workspaces/{slug}/workspace-members/me/": [{}],
    "/api/workspaces/{slug}/invitations/": [{}],
    "/api/workspaces/{slug}/invitations/{pk}/": [{}],
    "/api/workspaces/{slug}/invitations/{pk}/join/": [{}, {"accepted": "true"}],
    "/api/workspaces/{slug}/workspace-themes/": [{}],
    "/api/workspaces/{slug}/workspace-themes/{pk}/": [{}],
    "/api/workspaces/{slug}/user-stats/{user_id}/": [{}],
    "/api/workspaces/{slug}/user-activity/{user_id}/": [{}],
    "/api/workspaces/{slug}/user-profile/{user_id}/": [{}],
    "/api/workspaces/{slug}/user-issues/{user_id}/": [
        {},
        {"per_page": P["per_page"]},
        {"priority": P["priority"]},
        {"group_by": P["group_by_state"]},
    ],
    "/api/workspaces/{slug}/user-properties/": [{}],
    "/api/workspaces/{slug}/user-favorites/": [{}],
    "/api/workspaces/{slug}/user-favorites/{favorite_id}/": [{}],
    "/api/workspaces/{slug}/user-favorites/{favorite_id}/group/": [{}],
    "/api/workspaces/{slug}/quick-links/": [{}],
    "/api/workspaces/{slug}/quick-links/{pk}/": [{}],
    "/api/workspaces/{slug}/home-preferences/": [{}],
    "/api/workspaces/{slug}/home-preferences/{key}/": [{}],
    "/api/workspaces/{slug}/recent-visits/": [{}, {"entity_name": P["entity_name_project"]}],
    "/api/workspaces/{slug}/sidebar-preferences/": [{}],
    "/api/workspaces/{slug}/stickies/": [{}, {"query": P["query"]}],
    "/api/workspaces/{slug}/stickies/{pk}/": [{}],
    "/api/workspaces/{slug}/webhooks/": [{}],
    "/api/workspaces/{slug}/webhooks/{pk}/": [{}],
    "/api/workspaces/{slug}/webhook-logs/{webhook_id}/": [{}],
    # --- analytics ---------------------------------------------------------
    "/api/workspaces/{slug}/analytics/": [
        {"x_axis": P["x_axis"], "y_axis": P["y_axis"]},
        {"x_axis": P["x_axis"], "y_axis": P["y_axis"], "segment": P["segment"]},
        {"x_axis": "priority", "y_axis": "estimate", "segment": "state__group"},
        {},
    ],
    "/api/workspaces/{slug}/analytic-view/": [{}, {"fields": P["fields"]}],
    "/api/workspaces/{slug}/analytic-view/{pk}/": [{}],
    "/api/workspaces/{slug}/saved-analytic-view/{analytic_id}/": [{}],
    "/api/workspaces/{slug}/default-analytics/": [{}, {"project_ids": P["project_ids"]}],
    "/api/workspaces/{slug}/project-stats/": [{}, {"fields": "total_issues,completed_issues", "project_ids": P["project_ids"]}],
    "/api/workspaces/{slug}/advance-analytics/": [
        {},
        {"date_filter": P["date_filter"], "tab": "overview"},
        {"date_filter": P["date_filter"], "tab": "work-items"},
    ],
    "/api/workspaces/{slug}/advance-analytics-stats/": [{}, {"date_filter": P["date_filter"]}],
    "/api/workspaces/{slug}/advance-analytics-charts/": [{}, {"date_filter": P["date_filter"], "x_axis": P["x_axis"]}],
    "/api/workspaces/{slug}/projects/{project_id}/advance-analytics/": [
        {},
        {"date_filter": P["date_filter"], "cycle_id": P["cycle_id_q"]},
        {"date_filter": P["date_filter"], "module_id": P["module_id_q"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/advance-analytics-stats/": [{}, {"date_filter": P["date_filter"]}],
    "/api/workspaces/{slug}/projects/{project_id}/advance-analytics-charts/": [
        {"type": "custom-work-items", "x_axis": "PRIORITY"},
        {"type": "work-items"},
        {"type": "custom-work-items", "date_filter": P["date_filter"], "group_by": "state__group"},
    ],
    # --- projects ----------------------------------------------------------
    "/api/workspaces/{slug}/projects/": [
        {},
        {"per_page": P["per_page"]},
        {"cursor": P["cursor"]},
        {"order_by": P["order_by_name"]},
        {"name": P["name"]},
        {"fields": P["fields"]},
    ],
    "/api/workspaces/{slug}/projects/details/": [{}, {"per_page": P["per_page"]}],
    "/api/workspaces/{slug}/projects/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/invitations/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/invitations/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/join/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/members/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/members/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/project-members/me/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/preferences/member/{member_id}/": [{}],
    "/api/workspaces/{slug}/project-identifiers/": [{"name": "ALP"}, {}],
    "/api/workspaces/{slug}/user-favorite-projects/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/project-deploy-boards/{pk}/": [{}],
    # --- states ------------------------------------------------------------
    "/api/workspaces/{slug}/states/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/states/": [{}, {"grouped": P["grouped"]}],
    "/api/workspaces/{slug}/projects/{project_id}/states/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/intake-state/": [{}],
    # --- labels ------------------------------------------------------------
    "/api/workspaces/{slug}/labels/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issue-labels/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issue-labels/{pk}/": [{}],
    # --- estimates ---------------------------------------------------------
    "/api/workspaces/{slug}/estimates/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/project-estimates/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/estimates/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/estimates/{estimate_id}/": [{}],
    # --- issues ------------------------------------------------------------
    "/api/workspaces/{slug}/issues/": [
        {},
        {"per_page": P["per_page"]},
        {"cursor": P["cursor"]},
        {"order_by": P["order_by_updated"]},
        {"priority": P["priority"]},
        {"state_group": P["state_group"]},
        {"assignees": P["assignees"]},
        {"labels": P["labels"]},
        {"created_by": P["created_by"]},
        {"project": P["project"]},
        {"cycle": P["cycle"]},
        {"module": P["module"]},
        {"type": P["type_active"]},
        {"group_by": P["group_by_state"]},
        {"group_by": P["group_by_state_group"]},
        {"group_by": P["group_by_priority"]},
        {"group_by": P["group_by_assignee"]},
        {"group_by": P["group_by_labels"]},
        {"group_by": P["group_by_cycle"]},
        {"group_by": P["group_by_module"]},
        {"group_by": P["group_by_created_by"]},
        {"group_by": P["group_by_priority"], "sub_group_by": P["group_by_state_group"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/issues/": [
        {},
        {"per_page": P["per_page"]},
        {"cursor": P["cursor"]},
        {"order_by": P["order_by_updated"]},
        {"order_by": P["order_by_sequence"]},
        {"state": P["state_in"]},
        {"state_group": P["state_group"]},
        {"priority": P["priority_in"]},
        {"assignees": P["assignees"]},
        {"labels": P["labels"]},
        {"mentions": P["mentions"]},
        {"subscriber": P["subscriber"]},
        {"created_by": P["created_by"]},
        {"cycle": P["cycle"]},
        {"module": P["module"]},
        {"estimate_point": P["estimate_point"]},
        {"type": P["type_active"]},
        {"start_date": P["start_date_after"]},
        {"target_date": P["target_date_before"]},
        {"created_at": P["created_at_range"]},
        {"updated_at": P["updated_at_day"]},
        {"completed_at": P["completed_at_range"]},
        {"name": P["name"]},
        {"parent": P["parent_none"]},
        {"sub_issue": P["sub_issue"]},
        {"start_target_date": "true"},
        {"is_draft": "true"},
        {"is_archived": "true"},
        {"logged_by": P["created_by"]},
        {"intake_status": P["intake_status"]},
        {"inbox_status": P["inbox_status"]},
        {"group_by": P["group_by_state"]},
        {"group_by": P["group_by_assignee"], "sub_group_by": P["sub_group_by_priority"]},
        {"group_by": P["group_by_state_group"], "sub_group_by": P["group_by_priority"]},
        {"fields": P["fields_issues"]},
        {"expand": P["expand_issue"]},
        {"description": P["description"]},
        {"updated_at__gt": P["updated_at_gt"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/issues/list/": [
        {"issues": P["issues"]},
        {"issues": P["issues"], "per_page": P["per_page"]},
        {},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/issues-detail/": [{}, {"issues": P["issues"]}],
    "/api/workspaces/{slug}/projects/{project_id}/v2/issues/": [
        {},
        {"per_page": P["per_page"]},
        {"cursor": P["cursor"]},
        {"order_by": P["order_by_updated"], "group_by": P["group_by_state"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/sub-issues/": [
        {},
        {"order_by": P["order_by_created"]},
        {"group_by": P["group_by_state"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-links/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/": [{}],
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/": [{}],
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/attachments/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/history/": [
        {},
        {"activity_type": P["activity_type"]},
        {"created_at__gt": P["updated_at_gt"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-subscribers/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/subscribe/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/reactions/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/comments/{comment_id}/reactions/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/user-properties/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/archived-issues/": [
        {},
        {"per_page": P["per_page"]},
        {"group_by": P["group_by_state"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{pk}/archive/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-relation/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/deleted-issues/": [{}, {"per_page": P["per_page"]}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/": [{}, {"cursor": P["cursor"]}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/versions/{pk}/": [{}, {"cursor": P["cursor"]}],
    "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/": [
        {},
        {"cursor": P["cursor"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/work-items/{work_item_id}/description-versions/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/meta/": [{}],
    "/api/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/": [{}],
    "/api/workspaces/{slug}/draft-issues/": [{}, {"per_page": P["per_page"]}, {"priority": P["priority"]}],
    "/api/workspaces/{slug}/draft-issues/{pk}/": [{}],
    "/api/workspaces/{slug}/export-issues/": [
        {},
        {"per_page": P["per_page"]},
        {"cursor": P["cursor"]},
        {"order_by": P["order_by_created"]},
    ],
    # --- cycles ------------------------------------------------------------
    "/api/workspaces/{slug}/cycles/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/cycles/": [
        {},
        {"cycle_view": P["cycle_view"]},
        {"cycle_view": P["cycle_view_current"]},
        {"type": "all"},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/": [
        {},
        {"per_page": P["per_page"]},
        {"order_by": P["order_by_created"]},
        {"group_by": P["group_by_state"]},
        {"group_by": P["group_by_priority"], "sub_group_by": P["group_by_state"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/user-favorite-cycles/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/user-properties/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/archive/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/archived-cycles/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/archived-cycles/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/progress/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/analytics/": [{}, {"type": "issues"}],
    # --- modules -----------------------------------------------------------
    "/api/workspaces/{slug}/modules/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/modules/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/modules/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/": [
        {},
        {"per_page": P["per_page"]},
        {"order_by": P["order_by_created"]},
        {"group_by": P["group_by_state"]},
        {"group_by": P["group_by_state_group"], "sub_group_by": P["group_by_priority"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/issues/{issue_id}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-links/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/user-favorite-modules/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/user-properties/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/modules/{module_id}/archive/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/archived-modules/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/archived-modules/{pk}/": [{}],
    # --- views -------------------------------------------------------------
    "/api/workspaces/{slug}/views/": [{}, {"fields": P["fields"]}, {"order_by": P["order_by_created"]}],
    "/api/workspaces/{slug}/views/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/views/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/views/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/user-favorite-views/": [{}],
    # --- pages -------------------------------------------------------------
    "/api/workspaces/{slug}/projects/{project_id}/pages/": [
        {},
        {"order_by": P["order_by_created"]},
        {"track_visit": P["track_visit"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/pages-summary/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/description/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/pages/{page_id}/versions/{pk}/": [{}],
    "/api/workspaces/{slug}/pages/{page_id}/live-access/": [
        {"action": P["action_view"]},
        {"action": P["action_edit"]},
        {},
    ],
    # --- intake ------------------------------------------------------------
    "/api/workspaces/{slug}/projects/{project_id}/intakes/": [{}, {"cursor": P["cursor"]}, {"status": P["status_accepted"]}],
    "/api/workspaces/{slug}/projects/{project_id}/intakes/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/inboxes/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/inboxes/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/intake-issues/": [
        {},
        {"per_page": P["per_page"]},
        {"cursor": P["cursor"]},
        {"status": P["status_pending"]},
        {"order_by": P["order_by_created"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/intake-issues/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/inbox-issues/": [{}, {"status": P["status_accepted"]}],
    "/api/workspaces/{slug}/projects/{project_id}/inbox-issues/{pk}/": [{}],
    "/api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/": [
        {},
        {"cursor": P["cursor"]},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/intake-work-items/{work_item_id}/description-versions/{pk}/": [{}],
    # --- notifications -----------------------------------------------------
    "/api/workspaces/{slug}/users/notifications/": [
        {},
        {"read": P["read"]},
        {"archived": P["archived"]},
        {"snoozed": P["snoozed"]},
        {"mentioned": P["mentioned"]},
        {"type": P["notification_type"]},
        {"per_page": P["per_page"]},
        {"cursor": P["cursor"]},
        {"order_by": P["order_by_created"]},
    ],
    "/api/workspaces/{slug}/users/notifications/{pk}/": [{}],
    "/api/workspaces/{slug}/users/notifications/unread/": [{}],
    # --- search ------------------------------------------------------------
    "/api/workspaces/{slug}/search/": [
        {"query": P["query"]},
        {"query": P["query"], "count": P["count"]},
        {"search": P["search"]},
        {},
    ],
    "/api/workspaces/{slug}/entity-search/": [
        {"query": P["query"], "query_type": P["query_type"]},
        {"search": P["search"], "entities": P["entities"]},
        {},
    ],
    "/api/workspaces/{slug}/projects/{project_id}/search-issues/": [
        {"search": P["search"]},
        {"search": P["search"], "workspace_search": P["workspace_search"]},
        {"search": P["search"], "parent": "true"},
        {"search": P["search"], "sub_issue": "true"},
        {"search": P["search"], "issue_relation": "true"},
        {"search": P["search"], "cycle": "true"},
        {"search": P["search"], "module": "true"},
        {"search": P["search"], "target_date": "true"},
        {"search": P["search"], "issue_id": "{issue_alpha}"},
        {},
    ],
    # --- assets ------------------------------------------------------------
    "/api/workspaces/{slug}/file-assets/": [{}],
    "/api/workspaces/file-assets/{workspace_id}/{asset_key}/": [{}],
    "/api/users/file-assets/": [{}],
    "/api/users/file-assets/{asset_key}/": [{}],
    "/api/assets/v2/workspaces/{slug}/": [{}],
    "/api/assets/v2/workspaces/{slug}/{asset_id}/": [{}],
    "/api/assets/v2/static/{asset_id}/": [{}],
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/": [{}],
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/{pk}/": [{}],
    "/api/assets/v2/workspaces/{slug}/check/{asset_id}/": [{}],
    "/api/assets/v2/workspaces/{slug}/download/{asset_id}/": [{}],
    "/api/assets/v2/workspaces/{slug}/projects/{project_id}/download/{asset_id}/": [{}],
    # --- v1 ---------------------------------------------------------------
    "/api/v1/workspaces/{slug}/": [{}],
    "/api/v1/workspaces/{slug}/.{format}": [{}],
    "/api/v1/users/me/": [{}],
    "/api/v1/workspaces/{slug}/projects/": [{}, {"per_page": P["per_page"]}, {"order_by": P["order_by_created"]}, {"include_archived": P["include_archived"]}, {"fields": P["fields"]}],
    "/api/v1/workspaces/{slug}/projects-lite/": [{}, {"include_archived": P["include_archived"]}],
    "/api/v1/workspaces/{slug}/projects/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/summary/": [{}],
    "/api/v1/workspaces/{slug}/members/": [{}],
    "/api/v1/workspaces/{slug}/members-lite/": [{}],
    "/api/v1/workspaces/{slug}/invitations/": [{}],
    "/api/v1/workspaces/{slug}/invitations.{format}": [{}],
    "/api/v1/workspaces/{slug}/invitations/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/invitations/{pk}.{format}": [{}],
    "/api/v1/workspaces/{slug}/stickies/": [{}, {"query": P["query"]}],
    "/api/v1/workspaces/{slug}/stickies.{format}": [{}],
    "/api/v1/workspaces/{slug}/stickies/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/stickies/{pk}.{format}": [{}],
    "/api/v1/workspaces/{slug}/assets/{asset_id}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/": [{}, {"cycle_view": P["cycle_view"]}, {"order_by": P["order_by_created"]}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/cycles-lite/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/": [{}, {"per_page": P["per_page"]}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/cycles/{cycle_id}/cycle-issues/{issue_id}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/archived-cycles/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/": [{}, {"per_page": P["per_page"]}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/intake-issues/{issue_id}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/labels/": [{}, {"per_page": P["per_page"]}, {"order_by": P["order_by_name"]}, {"search": P["search"]}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/labels/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/members/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/members/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/project-members/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/project-members-lite/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/project-members/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/modules/": [{}, {"per_page": P["per_page"]}, {"order_by": P["order_by_created"]}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/modules-lite/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/modules/{module_id}/module-issues/": [{}, {"per_page": P["per_page"]}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/archived-modules/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/states/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/states/{state_id}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/": [
        {},
        {"per_page": P["per_page"]},
        {"order_by": P["order_by_created"]},
        {"search": P["search"]},
        {"external_source": P["external_source"]},
        {"limit": P["limit"]},
    ],
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/links/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/comments/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/activities/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/issues/{issue_id}/issue-attachments/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/work-items/search/": [{"search": P["search"]}],
    "/api/v1/workspaces/{slug}/issues/search/": [{"search": P["search"]}],
    "/api/v1/workspaces/{slug}/work-items/{project_identifier}-{issue_identifier}/": [{}],
    "/api/v1/workspaces/{slug}/issues/{project_identifier}-{issue_identifier}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/": [
        {},
        {"per_page": P["per_page"]},
        {"order_by": P["order_by_created"]},
    ],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/links/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/comments/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/activities/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/attachments/{pk}/": [{}],
    "/api/v1/workspaces/{slug}/projects/{project_id}/work-items/{issue_id}/relations/": [{}],
    # --- public (space) ----------------------------------------------------
    "/api/public/workspaces/{slug}/project-boards/": [{}],
    "/api/public/workspaces/{slug}/projects/{project_id}/anchor/": [{}],
    "/api/public/anchor/{anchor}/meta/": [{}],
    "/api/public/anchor/{anchor}/settings/": [{}],
    "/api/public/anchor/{anchor}/issues/": [
        {},
        {"per_page": P["per_page"]},
        {"order_by": P["order_by_created"]},
        {"group_by": P["group_by_state"]},
    ],
    "/api/public/anchor/{anchor}/issues/{issue_id}/": [{}],
    "/api/public/anchor/{anchor}/issues/{issue_id}/comments/": [{}],
    "/api/public/anchor/{anchor}/issues/{issue_id}/comments/{pk}/": [{}],
    "/api/public/anchor/{anchor}/issues/{issue_id}/reactions/": [{}],
    "/api/public/anchor/{anchor}/comments/{comment_id}/reactions/": [{}],
    "/api/public/anchor/{anchor}/issues/{issue_id}/votes/": [{}],
    "/api/public/anchor/{anchor}/cycles/": [{}],
    "/api/public/anchor/{anchor}/modules/": [{}],
    "/api/public/anchor/{anchor}/states/": [{}],
    "/api/public/anchor/{anchor}/labels/": [{}],
    "/api/public/anchor/{anchor}/members/": [{}],
    "/api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/": [{}],
    "/api/public/anchor/{anchor}/intakes/{intake_id}/intake-issues/{pk}/": [{}],
    "/api/public/anchor/{anchor}/intakes/{intake_id}/inbox-issues/": [{}],
    "/api/public/assets/v2/anchor/{anchor}/": [{}],
    "/api/public/assets/v2/anchor/{anchor}/{pk}/": [{}],
    # --- auth oauth --------------------------------------------------------
    "/auth/google/": [{}, {"next_path": P["next_path"]}],
    "/auth/google/callback/": [{"code": P["code"], "state": P["state_code"]}, {}],
    "/auth/github/": [{}],
    "/auth/github/callback/": [{"code": P["code"], "state": P["state_code"]}, {}],
    "/auth/gitlab/": [{}],
    "/auth/gitlab/callback/": [{"code": P["code"], "state": P["state_code"]}, {}],
    "/auth/gitea/": [{}],
    "/auth/gitea/callback/": [{"code": P["code"], "state": P["state_code"]}, {}],
}

# The spaces variants share the app handlers' behaviour with a different
# redirect target; one anonymous request each is enough.
for _path in (
    "/auth/spaces/google/",
    "/auth/spaces/google/callback/",
    "/auth/spaces/github/",
    "/auth/spaces/github/callback/",
    "/auth/spaces/gitlab/",
    "/auth/spaces/gitlab/callback/",
    "/auth/spaces/gitea/",
    "/auth/spaces/gitea/callback/",
):
    USERS[_path] = ["anon"]
    QUERIES[_path] = [{}]


# ---------------------------------------------------------------------------
# Generation
# ---------------------------------------------------------------------------


def load_groups_module() -> Any:
    import sys as _sys

    spec = importlib.util.spec_from_file_location("plane_gen_caddy", GEN_CADDY)
    if spec is None or spec.loader is None:
        raise SystemExit(f"cannot import {GEN_CADDY}")
    module = importlib.util.module_from_spec(spec)
    _sys.modules["plane_gen_caddy"] = module
    spec.loader.exec_module(module)
    return module


def load_routes() -> tuple[list[dict], dict[int, str]]:
    gen_caddy = load_groups_module()
    _, routes = gen_caddy.load_inventory(ROUTES_JSON)
    groups = gen_caddy.load_groups(GROUPS_FILE, routes)
    by_order, _ = gen_caddy.assign_groups(routes, groups)
    assignments = {order: group.name for order, group in by_order.items()}
    unique: list[dict] = []
    seen: set[tuple[str, str, str]] = set()
    for route in routes:
        if "get" not in route["methods"]:
            continue
        key = (assignments[route["order"]], route["path"], route["view"])
        if key in seen:
            continue
        seen.add(key)
        unique.append(route)
    return unique, assignments


PARAM_RE = re.compile(r"\{([a-zA-Z_][a-zA-Z0-9_]*)\}")


def parameters(path: str) -> list[str]:
    return [name for name in PARAM_RE.findall(path) if name != "format"]


def resolve_parameter(name: str, path: str, group: str) -> str:
    override = BIND.get(path, {})
    if name in override:
        return override[name]
    if name == "slug":
        return "ws"
    if name == "workspace_id":
        return "ws_id"
    if name == "project_id":
        return "project_alpha"
    if name in ("project_identifier", "issue_identifier"):
        raise SystemExit(f"{path}: {name} needs an explicit BIND entry")
    if name == "pk":
        raise SystemExit(f"{path}: `pk` needs an explicit BIND entry")
    defaults = {
        "asset_key": "attachment_asset_key",
        "asset_id": "attachment_id",
        "anchor": "anchor_alpha",
        "intake_id": "intake_alpha",
        "issue_id": "issue_alpha_commented",
        "work_item_id": "issue_alpha_commented",
        "comment_id": "comment_alpha",
        "cycle_id": "cycle_alpha_current",
        "module_id": "module_alpha_in_progress",
        "page_id": "page_public",
        "estimate_id": "estimate_alpha_points",
        "analytic_id": "analytic_view_default",
        "favorite_id": "favorite_project",
        "webhook_id": "webhook_active",
        "user_id": "user_member_id",
        "member_id": "user_member_id",
        "state_id": "state_alpha_todo",
        "key": "=quick_links",
        "format": "json",
    }
    if name in defaults:
        return defaults[name]
    raise SystemExit(f"{path}: no binding for parameter {{{name}}}")


def render_path(path: str, group: str) -> str:
    def replace(match: re.Match) -> str:
        name = match.group(1)
        if name == "format":
            return "json"
        value = resolve_parameter(name, path, group)
        # A binding that starts with "=" is a literal URL segment, not a
        # fixture variable (e.g. the `{key}` of a home preference).
        if value.startswith("="):
            return value[1:]
        return "{{" + value + "}}"

    return PARAM_RE.sub(replace, path)


def render_query(query: dict[str, str]) -> dict[str, str]:
    rendered = {}
    for key, value in query.items():
        rendered[key] = re.sub(r"\{([a-zA-Z_][a-zA-Z0-9_]*)\}", lambda m: "{{" + m.group(1) + "}}", value)
    return rendered


def users_for(path: str) -> list[str]:
    if path in USERS:
        return USERS[path]
    if path.startswith("/api/v1/"):
        return ["api_admin", "api_member", "anon"]
    if path.startswith("/api/public/"):
        return ["admin", "member", "anon"]
    if path.startswith("/api/instances/"):
        return ["instance_admin", "member", "anon"]
    if path.startswith("/api/users/"):
        return ["admin", "member"]
    if "/projects/{project_id}/" in path or "{project_id}" in path:
        return PROJECT_USERS
    if path.startswith("/api/workspaces/"):
        return WS_USERS
    return ["admin", "member"]


def queries_for(path: str) -> list[dict[str, str]]:
    if path in QUERIES:
        return QUERIES[path]
    return [{}]


class Step:
    def __init__(
        self,
        index: int,
        name: str,
        route: str,
        path: str,
        query: dict[str, str],
        identity: str,
        jar: str,
    ) -> None:
        self.index = index
        self.name = name
        # The `routes.json` path template, so coverage can group every step of
        # one route even though the emitted path has fixture ids substituted.
        self.route = route
        self.path = path
        self.query = query
        self.identity = identity
        self.jar = jar


def rotate_identity(identity: str, counters: dict[str, int]) -> str:
    names = API_ROTATION.get(identity)
    if not names:
        return identity
    index = counters.get(identity, 0)
    counters[identity] = index + 1
    return names[index % len(names)]


def build_steps(routes: list[dict], assignments: dict[int, str]) -> dict[str, list[Step]]:
    by_group: dict[str, list[Step]] = OrderedDict()
    counter = 0
    rotation: dict[str, int] = {}
    for route in routes:
        group = assignments[route["order"]]
        path = render_path(route["path"], group)
        identities = [rotate_identity(identity, rotation) for identity in users_for(route["path"])]
        queries = queries_for(route["path"])
        extra = EXTRA.get(route["path"], [])
        steps: list[Step] = []
        for identity in identities:
            for query in queries[:2] if len(queries) > 1 else queries:
                counter += 1
                steps.append(
                    Step(
                        counter,
                        f"{group}-{counter:04d}-{slugify(route['view'].rsplit('.', 1)[-1])}",
                        route["path"],
                        path,
                        render_query(query),
                        identity,
                        "default",
                    )
                )
        if len(queries) > 2:
            identity = identities[0]
            for query in queries[2:]:
                counter += 1
                steps.append(
                    Step(
                        counter,
                        f"{group}-{counter:04d}-{slugify(route['view'].rsplit('.', 1)[-1])}",
                        route["path"],
                        path,
                        render_query(query),
                        identity,
                        "default",
                    )
                )
        for query in extra:
            counter += 1
            steps.append(
                Step(
                    counter,
                    f"{group}-{counter:04d}-{slugify(route['view'].rsplit('.', 1)[-1])}",
                    route["path"],
                    path,
                    render_query(query),
                    identities[0],
                    "default",
                )
            )
        by_group.setdefault(group, []).extend(steps)
    return by_group


def slugify(text: str) -> str:
    text = re.sub(r"(?<!^)(?=[A-Z])", "-", text).lower()
    return re.sub(r"[^a-z0-9]+", "-", text).strip("-")[:40]


def render_scenario(group: str, steps: list[Step], fixture: dict) -> str:
    variables = OrderedDict()
    for step in steps:
        for token in re.findall(r"\{\{([a-zA-Z0-9_]+)\}\}", step.path):
            variables[token] = fixture[token]
        for value in step.query.values():
            for token in re.findall(r"\{\{([a-zA-Z0-9_]+)\}\}", value):
                variables[token] = fixture[token]
    for step in steps:
        if step.identity != "anon":
            _header, _prefix, var = IDENTITIES[step.identity]
            variables[var] = fixture[var]

    lines: list[str] = []
    lines.append(f"# Copyright (c) 2023-present Plane Software, Inc. and contributors")
    lines.append("# SPDX-License-Identifier: AGPL-3.0-only")
    lines.append("# See the LICENSE file for details.")
    lines.append("#")
    lines.append("# GENERATED by apps/server/tools/scenarios/gen_read.py. Do not edit by hand.")
    lines.append(f"# One GET scenario for the `{group}` route group, run against the rich read")
    lines.append("# fixture template `plane_fix_tpl`. Regenerate after rebuilding the fixture:")
    lines.append("#   apps/api/.venv/bin/python apps/server/tools/scenarios/gen_read.py")
    lines.append("#")
    lines.append("# Sessions are supplied as an explicit Cookie header (the fixture already has")
    lines.append("# signed session rows); anonymous steps send a distinct X-Forwarded-For.")
    lines.append("")
    lines.append(f"name: read-{group}")
    lines.append("template_db: plane_fix_tpl")
    # Dedicated indexes: the harness default [1, 2] is used by every agent on
    # the shared dev stack, and a concurrent run writing a `cache_page` key
    # between this run's A and B steps shows up as a Redis DIFF that is not a
    # difference in the backend.
    lines.append("redis_db: [6, 7]")
    lines.append("target_env:")
    lines.append("  # Some views build their response from a Python `set` (for example")
    lines.append("  # `ProjectSummaryAPIEndpoint` iterates `requested_fields`), so JSON key")
    lines.append("  # order follows the process's string-hash seed. Pinning the seed makes")
    lines.append("  # the two targets agree without normalising the body.")
    lines.append('  PYTHONHASHSEED: "0"')
    lines.append("vars:")
    for key, value in variables.items():
        lines.append(f"  {yaml_key(key)}: {yaml_scalar(value)}")
    lines.append("steps:")
    anon_counter = 0
    for step in steps:
        lines.append(f"  - name: {step.name}")
        lines.append("    method: GET")
        lines.append(f"    path: {yaml_scalar(step.path)}")
        if step.query:
            lines.append("    query:")
            for key, value in step.query.items():
                lines.append(f"      {yaml_key(key)}: {yaml_scalar(value)}")
        identity = IDENTITIES[step.identity]
        if identity is None:
            anon_counter += 1
            lines.append(f"    jar: anon{anon_counter}")
            lines.append("    headers:")
            lines.append(f"      Host: {FIXED_HOST}")
            lines.append(f"      X-Forwarded-For: 198.51.100.{anon_counter % 250 + 1}")
        else:
            header, prefix, var = identity
            lines.append(f"    jar: {step.identity}")
            lines.append("    headers:")
            # A fixed Host keeps every host-derived value identical on both
            # targets: Django's `cache_page` key hashes the absolute URI, the
            # S3 presigner signs `request.get_host()` (plane/settings/storage.py
            # uses `http://{request.get_host()}` as the MinIO endpoint), and the
            # API-token log message carries the Host header.
            lines.append(f"      Host: {FIXED_HOST}")
            lines.append(f"      {header}: {yaml_scalar(prefix + '{{' + var + '}}')}")
        if step.query == {} and "{pk}" in step.path:
            pass
    lines.append("")
    return "\n".join(lines)


def yaml_scalar(value: str) -> str:
    # Always a double-quoted JSON string: an unquoted `1` or `true` would be
    # parsed as a number/boolean, and the harness requires `String` query
    # values and variables.
    return json.dumps(value)


def yaml_key(value: str) -> str:
    if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", value):
        return value
    return json.dumps(value)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--manifest", type=Path, default=MANIFEST)
    parser.add_argument("--out-dir", type=Path, default=OUT_DIR)
    parser.add_argument("--only", default=None, help="generate one group only")
    parser.add_argument("--check", action="store_true", help="fail when a generated file is stale")
    args = parser.parse_args()

    manifest = json.loads(args.manifest.read_text())
    fixture = manifest["vars"]
    routes, assignments = load_routes()
    by_group = build_steps(routes, assignments)

    args.out_dir.mkdir(parents=True, exist_ok=True)
    index: dict = {
        "generated_by": "apps/server/tools/scenarios/gen_read.py",
        "template_db": manifest["template_db"],
        "base_time": manifest["base_time"],
        "routes": {},
    }
    written = 0
    stale: list[str] = []
    for group, steps in by_group.items():
        if args.only and group != args.only:
            continue
        text = render_scenario(group, steps, fixture)
        target = args.out_dir / f"{group}.yaml"
        if args.check:
            if not target.is_file() or target.read_text() != text:
                stale.append(str(target))
            continue
        target.write_text(text, encoding="utf-8", newline="\n")
        written += 1
        index["routes"][group] = [
            {
                "step": step.name,
                "route": step.route,
                "path": step.path,
                "identity": step.identity,
                "query": step.query,
            }
            for step in steps
        ]
        print(f"{target.relative_to(SERVER_DIR)}: {len(steps)} steps")

    if args.check:
        if stale:
            print("stale generated scenarios:", file=sys.stderr)
            for path in stale:
                print(f"  {path}", file=sys.stderr)
            return 1
        print("all generated read scenarios are up to date")
        return 0

    (args.out_dir / "route-index.json").write_text(
        json.dumps(index, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n"
    )
    total = sum(len(steps) for steps in by_group.values())
    print(f"{written} files, {total} steps, {len(routes)} unique GET routes")
    print(f"wrote {args.out_dir / 'route-index.json'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
