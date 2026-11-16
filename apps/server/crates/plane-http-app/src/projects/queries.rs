// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `projects` group.
//!
//! The statements are the ORM's own: the same joins (including the ones only
//! `select_related` produced, because an `INNER JOIN` on a missing row drops the
//! project), the same `GROUP BY` on the primary key, the same correlated
//! subqueries for the annotations, and the same ordering. Where a route's result
//! has ties (these lists order by an annotation or by `created_at` alone) the
//! tie order is the plan's, and the plan follows the statement.
//!
//! The three variants of the project list are the three `if`s of
//! `ProjectViewSet.list`/`list_detail` (`plane/app/views/project/base.py:106-129,200-223`):
//! a workspace guest is filtered to their own project memberships, a workspace
//! member to those plus public projects, and everyone else sees the workspace.

use plane_db::Query;
use plane_db::plane_query;

// ---------------------------------------------------------------------------
// Project list (`.values()` projection) — three role variants
// ---------------------------------------------------------------------------

plane_query! {
    /// `ProjectViewSet.list` for a caller who is neither a workspace guest nor
    /// a workspace member (an admin, or a member by workspace role that is
    /// neither 5 nor 15) (`plane/app/views/project/base.py:147-224`).
    ///
    /// Bind order: `$1` workspace slug, `$2` member id.
    PROJECT_LIST_ADMIN = "projects_list_admin",
    "SELECT DISTINCT p.id, p.name, p.identifier, \
       (SELECT pup.sort_order FROM project_user_properties pup \
          INNER JOIN workspaces w2 ON (pup.workspace_id = w2.id) \
          WHERE pup.deleted_at IS NULL AND pup.project_id = p.id \
            AND pup.user_id = $2 AND w2.slug = $1 \
          ORDER BY pup.created_at DESC) AS sort_order, \
       p.logo_props, \
       (SELECT pm.role FROM project_members pm \
          WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $2 \
            AND pm.project_id = p.id \
          ORDER BY pm.created_at DESC) AS member_role, \
       COUNT(i.id) FILTER (WHERE i.deleted_at IS NULL AND i.status = -2) AS intake_count, \
       p.archived_at, p.workspace_id, p.cycle_view, p.issue_views_view, \
       p.module_view, p.page_view, p.intake_view, p.guest_view_all_features, \
       p.project_lead_id, p.network, p.created_at, p.updated_at, \
       p.created_by_id, p.updated_by_id \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     LEFT OUTER JOIN intake_issues i ON (p.id = i.project_id) \
     WHERE p.deleted_at IS NULL AND w.slug = $1 \
     GROUP BY p.id",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("identifier", Varchar),
        ("sort_order", Float8),
        ("logo_props", Jsonb),
        ("member_role", Int2),
        ("intake_count", Int8),
        ("archived_at", Timestamptz),
        ("workspace_id", Uuid),
        ("cycle_view", Bool),
        ("issue_views_view", Bool),
        ("module_view", Bool),
        ("page_view", Bool),
        ("intake_view", Bool),
        ("guest_view_all_features", Bool),
        ("project_lead_id", Uuid),
        ("network", Int2),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
    ]
}

plane_query! {
    /// The same for a workspace **guest**: only projects they are an active
    /// member of (`plane/app/views/project/base.py:106-115`). The join is
    /// `INNER`, as Django's `filter(project_projectmember__…)` compiles it.
    ///
    /// Bind order: `$1` workspace slug, `$2` member id.
    PROJECT_LIST_GUEST = "projects_list_guest",
    "SELECT DISTINCT p.id, p.name, p.identifier, \
       (SELECT pup.sort_order FROM project_user_properties pup \
          INNER JOIN workspaces w2 ON (pup.workspace_id = w2.id) \
          WHERE pup.deleted_at IS NULL AND pup.project_id = p.id \
            AND pup.user_id = $2 AND w2.slug = $1 \
          ORDER BY pup.created_at DESC) AS sort_order, \
       p.logo_props, \
       (SELECT pm.role FROM project_members pm \
          WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $2 \
            AND pm.project_id = p.id \
          ORDER BY pm.created_at DESC) AS member_role, \
       COUNT(i.id) FILTER (WHERE i.deleted_at IS NULL AND i.status = -2) AS intake_count, \
       p.archived_at, p.workspace_id, p.cycle_view, p.issue_views_view, \
       p.module_view, p.page_view, p.intake_view, p.guest_view_all_features, \
       p.project_lead_id, p.network, p.created_at, p.updated_at, \
       p.created_by_id, p.updated_by_id \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     LEFT OUTER JOIN intake_issues i ON (p.id = i.project_id) \
     INNER JOIN project_members pmf ON (p.id = pmf.project_id) \
     WHERE p.deleted_at IS NULL AND w.slug = $1 \
       AND pmf.is_active AND pmf.member_id = $2 \
     GROUP BY p.id",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("identifier", Varchar),
        ("sort_order", Float8),
        ("logo_props", Jsonb),
        ("member_role", Int2),
        ("intake_count", Int8),
        ("archived_at", Timestamptz),
        ("workspace_id", Uuid),
        ("cycle_view", Bool),
        ("issue_views_view", Bool),
        ("module_view", Bool),
        ("page_view", Bool),
        ("intake_view", Bool),
        ("guest_view_all_features", Bool),
        ("project_lead_id", Uuid),
        ("network", Int2),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
    ]
}

plane_query! {
    /// The same for a workspace **member**: their projects plus every public
    /// (`network = 2`) project (`plane/app/views/project/base.py:117-129`).
    /// Django compiles `Q(…) | Q(network=2)` to a `LEFT OUTER JOIN` with the
    /// membership predicate in the join condition and the disjunction in the
    /// `WHERE`.
    ///
    /// Bind order: `$1` workspace slug, `$2` member id.
    PROJECT_LIST_MEMBER = "projects_list_member",
    "SELECT DISTINCT p.id, p.name, p.identifier, \
       (SELECT pup.sort_order FROM project_user_properties pup \
          INNER JOIN workspaces w2 ON (pup.workspace_id = w2.id) \
          WHERE pup.deleted_at IS NULL AND pup.project_id = p.id \
            AND pup.user_id = $2 AND w2.slug = $1 \
          ORDER BY pup.created_at DESC) AS sort_order, \
       p.logo_props, \
       (SELECT pm.role FROM project_members pm \
          WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $2 \
            AND pm.project_id = p.id \
          ORDER BY pm.created_at DESC) AS member_role, \
       COUNT(i.id) FILTER (WHERE i.deleted_at IS NULL AND i.status = -2) AS intake_count, \
       p.archived_at, p.workspace_id, p.cycle_view, p.issue_views_view, \
       p.module_view, p.page_view, p.intake_view, p.guest_view_all_features, \
       p.project_lead_id, p.network, p.created_at, p.updated_at, \
       p.created_by_id, p.updated_by_id \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     LEFT OUTER JOIN intake_issues i ON (p.id = i.project_id) \
     LEFT OUTER JOIN project_members pmf ON (p.id = pmf.project_id) \
     WHERE p.deleted_at IS NULL AND w.slug = $1 \
       AND ((pmf.is_active AND pmf.member_id = $2) OR p.network = 2) \
     GROUP BY p.id",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("identifier", Varchar),
        ("sort_order", Float8),
        ("logo_props", Jsonb),
        ("member_role", Int2),
        ("intake_count", Int8),
        ("archived_at", Timestamptz),
        ("workspace_id", Uuid),
        ("cycle_view", Bool),
        ("issue_views_view", Bool),
        ("module_view", Bool),
        ("page_view", Bool),
        ("intake_view", Bool),
        ("guest_view_all_features", Bool),
        ("project_lead_id", Uuid),
        ("network", Int2),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
    ]
}

plane_query! {
    /// `WorkspaceMember.objects.filter(role=…).exists()`, the two role probes
    /// `ProjectViewSet.list`/`list_detail` run before filtering
    /// (`plane/app/views/project/base.py:106-129,200-223`). The statement is the
    /// ORM's `.exists()`: `SELECT 1 … LIMIT 1`.
    ///
    /// Bind order: `$1` member id, `$2` role, `$3` workspace slug.
    WORKSPACE_MEMBER_HAS_ROLE = "projects_workspace_member_has_role",
    "SELECT 1 AS \"a\" FROM workspace_members wm \
     INNER JOIN workspaces w ON (wm.workspace_id = w.id) \
     WHERE wm.deleted_at IS NULL AND wm.is_active AND wm.member_id = $1 \
       AND wm.role = $2 AND w.slug = $3 \
     LIMIT 1",
    [("a", Int4)]
}

// ---------------------------------------------------------------------------
// Project detail (ProjectListSerializer)
// ---------------------------------------------------------------------------

plane_query! {
    /// `ProjectViewSet.retrieve`'s lookup
    /// (`plane/app/views/project/base.py:226-256`): `get_queryset()`
    /// `.filter(archived_at__isnull=True).filter(pk=pk).first()`.
    ///
    /// The annotations are `get_queryset()`'s:
    /// `is_favorite` (`Exists(UserFavorite…)`), `member_role`, `anchor`
    /// (`DeployBoard.anchor`), `sort_order` (`ProjectUserProperty.sort_order`).
    /// `select_related("workspace", "workspace__owner", "default_assignee",
    /// "project_lead")` becomes the four `users` joins; the `INNER JOIN` on the
    /// workspace owner is Django's and drops a workspace with no owner.
    ///
    /// Bind order: `$1` workspace slug, `$2` member id, `$3` project id.
    ///
    /// The list variants are assembled from this statement's prefix (see
    /// [`detail_sql`]) so the projection exists once.
    PROJECT_DETAIL_ONE = "projects_detail_one",
    "SELECT DISTINCT p.created_at, p.updated_at, p.created_by_id, p.updated_by_id, \
       p.deleted_at, p.id, p.name, p.description, p.description_text, \
       p.description_html, p.network, p.workspace_id, p.identifier, \
       p.default_assignee_id, p.project_lead_id, p.emoji, p.icon_prop, \
       p.module_view, p.cycle_view, p.issue_views_view, p.page_view, \
       p.intake_view, p.is_time_tracking_enabled, p.is_issue_type_enabled, \
       p.guest_view_all_features, p.cover_image, p.cover_image_asset_id, \
       p.estimate_id, p.archive_in, p.close_in, p.logo_props, \
       p.default_state_id, p.archived_at, p.timezone, p.external_source, \
       p.external_id, \
       EXISTS (SELECT 1 AS \"a\" FROM user_favorites uf \
          WHERE uf.deleted_at IS NULL AND uf.entity_identifier = p.id \
            AND uf.entity_type = 'project' AND uf.project_id = p.id \
            AND uf.user_id = $2 LIMIT 1) AS is_favorite, \
       (SELECT pm.role FROM project_members pm \
          WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $2 \
            AND pm.project_id = p.id \
          ORDER BY pm.created_at DESC) AS member_role, \
       (SELECT db.anchor FROM deploy_boards db \
          INNER JOIN workspaces w3 ON (db.workspace_id = w3.id) \
          WHERE db.deleted_at IS NULL AND db.entity_identifier = p.id \
            AND db.entity_name = 'project' AND w3.slug = $1 \
          ORDER BY db.created_at DESC) AS anchor, \
       (SELECT pup.sort_order FROM project_user_properties pup \
          INNER JOIN workspaces w2 ON (pup.workspace_id = w2.id) \
          WHERE pup.deleted_at IS NULL AND pup.project_id = p.id \
            AND pup.user_id = $2 AND w2.slug = $1 \
          ORDER BY pup.created_at DESC) AS sort_order \
     FROM projects p \
     INNER JOIN workspaces w ON (p.workspace_id = w.id) \
     INNER JOIN users wu ON (w.owner_id = wu.id) \
     LEFT OUTER JOIN users da ON (p.default_assignee_id = da.id) \
     LEFT OUTER JOIN users pl ON (p.project_lead_id = pl.id) \
     WHERE p.deleted_at IS NULL AND w.slug = $1 AND p.archived_at IS NULL \
       AND p.id = $3 \
     ORDER BY p.created_at DESC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("name", Varchar),
        ("description", Text),
        ("description_text", Jsonb),
        ("description_html", Jsonb),
        ("network", Int2),
        ("workspace_id", Uuid),
        ("identifier", Varchar),
        ("default_assignee_id", Uuid),
        ("project_lead_id", Uuid),
        ("emoji", Varchar),
        ("icon_prop", Jsonb),
        ("module_view", Bool),
        ("cycle_view", Bool),
        ("issue_views_view", Bool),
        ("page_view", Bool),
        ("intake_view", Bool),
        ("is_time_tracking_enabled", Bool),
        ("is_issue_type_enabled", Bool),
        ("guest_view_all_features", Bool),
        ("cover_image", Text),
        ("cover_image_asset_id", Uuid),
        ("estimate_id", Uuid),
        ("archive_in", Int4),
        ("close_in", Int4),
        ("logo_props", Jsonb),
        ("default_state_id", Uuid),
        ("archived_at", Timestamptz),
        ("timezone", Varchar),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("is_favorite", Bool),
        ("member_role", Int2),
        ("anchor", Varchar),
        ("sort_order", Float8),
    ]
}

/// Assemble a list variant of [`PROJECT_DETAIL_ONE`]: the same projection and
/// `select_related` joins, plus the role filter `where` and the request's
/// ordering.
///
/// The projection and its `select_related` joins are taken from the catalogued
/// statement, so a column the serializer reads cannot drift between the detail
/// route and the list route. The assembled text is sent as an **unnamed**
/// statement (HANDLERS.md §2): its `ORDER BY` depends on `?order_by=`.
pub fn detail_sql(join: &str, filter: &str, order: &str) -> Option<String> {
    // The projection contains `WHERE` clauses inside its correlated subqueries,
    // so the *top-level* one is the last.
    let (head, _) = PROJECT_DETAIL_ONE.sql.rsplit_once(" WHERE ")?;
    Some(format!(
        "{head} {join} WHERE p.deleted_at IS NULL AND w.slug = $1 {filter} {order}"
    ))
}

plane_query! {
    /// `prefetch_related(Prefetch("project_projectmember", … to_attr="members_list"))`
    /// (`plane/app/views/project/base.py:90-98`): one query for the whole page,
    /// ordered by `ProjectMember.Meta.ordering = ("-created_at",)`.
    ///
    /// `users.is_bot` comes from the prefetch's `select_related("member")`, and
    /// the join is `LEFT OUTER` as Django emits it.
    ///
    /// Bind order: `$1` workspace slug, `$2` the page's project ids.
    PROJECT_MEMBERS_PREFETCH = "projects_members_prefetch",
    "SELECT pm.project_id, pm.member_id, u.is_bot \
     FROM project_members pm \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     LEFT OUTER JOIN users u ON (pm.member_id = u.id) \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND w.slug = $1 \
       AND pm.project_id = ANY($2) \
     ORDER BY pm.created_at DESC",
    [
        ("project_id", Uuid),
        ("member_id", Uuid),
        ("is_bot", Bool),
    ]
}

plane_query! {
    /// `IssueSequence.objects.filter(project_id=obj.id).aggregate(max_seq=Max("sequence"))`
    /// — `ProjectListSerializer.get_next_work_item_sequence`
    /// (`plane/app/serializers/project.py:145-148`). Django runs it once per
    /// project; one grouped statement covers the page with the same result.
    ///
    /// Bind: `$1` the page's project ids.
    ISSUE_SEQUENCE_MAX = "projects_issue_sequence_max",
    "SELECT project_id, MAX(sequence) AS max_seq FROM issue_sequences \
     WHERE deleted_at IS NULL AND project_id = ANY($1) \
     GROUP BY project_id",
    [("project_id", Uuid), ("max_seq", Int8)]
}

// ---------------------------------------------------------------------------
// Project identifiers
// ---------------------------------------------------------------------------

plane_query! {
    /// `ProjectIdentifier.objects.filter(name=name, workspace__slug=slug).values("id","name","project")`
    /// (`plane/app/views/project/base.py:453`).
    ///
    /// Bind order: `$1` name, `$2` workspace slug.
    PROJECT_IDENTIFIERS = "projects_identifiers",
    "SELECT pi.id, pi.name, pi.project_id FROM project_identifiers pi \
     INNER JOIN workspaces w ON (pi.workspace_id = w.id) \
     WHERE pi.deleted_at IS NULL AND pi.name = $1 AND w.slug = $2 \
     ORDER BY pi.created_at DESC",
    [("id", Int8), ("name", Varchar), ("project_id", Uuid)]
}

// ---------------------------------------------------------------------------
// Project invitations
// ---------------------------------------------------------------------------

plane_query! {
    /// `ProjectInvitationsViewset.get_queryset()` + `list`
    /// (`plane/app/views/project/invite.py:45-59`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id.
    PROJECT_INVITATIONS = "projects_invitations",
    "SELECT pmi.created_at, pmi.updated_at, pmi.created_by_id, pmi.updated_by_id, \
       pmi.deleted_at, pmi.id, pmi.project_id, pmi.workspace_id, pmi.email, \
       pmi.accepted, pmi.token, pmi.message, pmi.responded_at, pmi.role, \
       p.id AS p_id, p.identifier AS p_identifier, p.name AS p_name, \
       p.cover_image AS p_cover_image, \
       p.cover_image_asset_id AS p_cover_image_asset_id, \
       p.logo_props AS p_logo_props, p.description AS p_description, \
       w.name AS w_name, w.slug AS w_slug, w.id AS w_id, w.logo AS w_logo, \
       w.logo_asset_id AS w_logo_asset_id \
     FROM project_member_invites pmi \
     INNER JOIN workspaces w ON (pmi.workspace_id = w.id) \
     INNER JOIN projects p ON (pmi.project_id = p.id) \
     INNER JOIN users wu ON (w.owner_id = wu.id) \
     WHERE pmi.deleted_at IS NULL AND w.slug = $1 AND pmi.project_id = $2 \
     ORDER BY pmi.created_at DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("email", Varchar),
        ("accepted", Bool),
        ("token", Varchar),
        ("message", Text),
        ("responded_at", Timestamptz),
        ("role", Int2),
        ("p_id", Uuid),
        ("p_identifier", Varchar),
        ("p_name", Varchar),
        ("p_cover_image", Text),
        ("p_cover_image_asset_id", Uuid),
        ("p_logo_props", Jsonb),
        ("p_description", Text),
        ("w_name", Varchar),
        ("w_slug", Varchar),
        ("w_id", Uuid),
        ("w_logo", Text),
        ("w_logo_asset_id", Uuid),
    ]
}

plane_query! {
    /// The same queryset plus `pk=`, which is `retrieve`
    /// (`plane/app/views/project/invite.py:61-63`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` invitation id.
    PROJECT_INVITATION_ONE = "projects_invitation_one",
    "SELECT pmi.created_at, pmi.updated_at, pmi.created_by_id, pmi.updated_by_id, \
       pmi.deleted_at, pmi.id, pmi.project_id, pmi.workspace_id, pmi.email, \
       pmi.accepted, pmi.token, pmi.message, pmi.responded_at, pmi.role, \
       p.id AS p_id, p.identifier AS p_identifier, p.name AS p_name, \
       p.cover_image AS p_cover_image, \
       p.cover_image_asset_id AS p_cover_image_asset_id, \
       p.logo_props AS p_logo_props, p.description AS p_description, \
       w.name AS w_name, w.slug AS w_slug, w.id AS w_id, w.logo AS w_logo, \
       w.logo_asset_id AS w_logo_asset_id \
     FROM project_member_invites pmi \
     INNER JOIN workspaces w ON (pmi.workspace_id = w.id) \
     INNER JOIN projects p ON (pmi.project_id = p.id) \
     INNER JOIN users wu ON (w.owner_id = wu.id) \
     WHERE pmi.deleted_at IS NULL AND w.slug = $1 AND pmi.project_id = $2 \
       AND pmi.id = $3",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("email", Varchar),
        ("accepted", Bool),
        ("token", Varchar),
        ("message", Text),
        ("responded_at", Timestamptz),
        ("role", Int2),
        ("p_id", Uuid),
        ("p_identifier", Varchar),
        ("p_name", Varchar),
        ("p_cover_image", Text),
        ("p_cover_image_asset_id", Uuid),
        ("p_logo_props", Jsonb),
        ("p_description", Text),
        ("w_name", Varchar),
        ("w_slug", Varchar),
        ("w_id", Uuid),
        ("w_logo", Text),
        ("w_logo_asset_id", Uuid),
    ]
}

plane_query! {
    /// `ProjectJoinEndpoint.get`: `ProjectMemberInvite.objects.get(workspace__slug=…, project_id=…, pk=…)`
    /// (`plane/app/views/project/invite.py:302-305`) — no `select_related`, so
    /// the nested project and workspace are loaded separately (see
    /// [`PROJECTS_BY_IDS`] and [`WORKSPACES_BY_IDS`]).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` invitation id.
    PROJECT_INVITATION_PUBLIC = "projects_invitation_public",
    "SELECT pmi.created_at, pmi.updated_at, pmi.created_by_id, pmi.updated_by_id, \
       pmi.deleted_at, pmi.id, pmi.project_id, pmi.workspace_id, pmi.email, \
       pmi.accepted, pmi.token, pmi.message, pmi.responded_at, pmi.role, \
       p.id AS p_id, p.identifier AS p_identifier, p.name AS p_name, \
       p.cover_image AS p_cover_image, \
       p.cover_image_asset_id AS p_cover_image_asset_id, \
       p.logo_props AS p_logo_props, p.description AS p_description, \
       w.name AS w_name, w.slug AS w_slug, w.id AS w_id, w.logo AS w_logo, \
       w.logo_asset_id AS w_logo_asset_id \
     FROM project_member_invites pmi \
     INNER JOIN workspaces w ON (pmi.workspace_id = w.id) \
     LEFT OUTER JOIN projects p ON (pmi.project_id = p.id) \
     WHERE pmi.deleted_at IS NULL AND w.slug = $1 AND pmi.project_id = $2 \
       AND pmi.id = $3",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("email", Varchar),
        ("accepted", Bool),
        ("token", Varchar),
        ("message", Text),
        ("responded_at", Timestamptz),
        ("role", Int2),
        ("p_id", Uuid),
        ("p_identifier", Varchar),
        ("p_name", Varchar),
        ("p_cover_image", Text),
        ("p_cover_image_asset_id", Uuid),
        ("p_logo_props", Jsonb),
        ("p_description", Text),
        ("w_name", Varchar),
        ("w_slug", Varchar),
        ("w_id", Uuid),
        ("w_logo", Text),
        ("w_logo_asset_id", Uuid),
    ]
}

// ---------------------------------------------------------------------------
// Project members
// ---------------------------------------------------------------------------

plane_query! {
    /// `ProjectMemberViewSet.list` (`plane/app/views/project/member.py:166-181`):
    /// active, non-bot members of the project who are also active members of the
    /// workspace, serialised by `ProjectMemberRoleSerializer`.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id.
    PROJECT_MEMBERS = "projects_members",
    "SELECT pm.id, pm.member_id, pm.role, pm.created_at \
     FROM project_members pm \
     INNER JOIN users u ON (pm.member_id = u.id) \
     INNER JOIN workspace_members wm ON (u.id = wm.member_id) \
     INNER JOIN workspaces w ON (wm.workspace_id = w.id) \
     INNER JOIN projects p ON (pm.project_id = p.id) \
     INNER JOIN workspaces w6 ON (pm.workspace_id = w6.id) \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND NOT u.is_bot \
       AND wm.is_active AND w.slug = $1 AND pm.project_id = $2 \
       AND w6.slug = $1 \
     ORDER BY pm.created_at DESC",
    [
        ("id", Uuid),
        ("member_id", Uuid),
        ("role", Int2),
        ("created_at", Timestamptz),
    ]
}

plane_query! {
    /// The requesting user's own active membership, which `retrieve` reads
    /// first (`plane/app/views/project/member.py:185-190`) and whose `role`
    /// decides which serializer the target member gets.
    ///
    /// Bind order: `$1` project id, `$2` member id, `$3` workspace slug.
    PROJECT_MEMBER_SELF_ROLE = "projects_member_self_role",
    "SELECT pm.role FROM project_members pm \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $2 \
       AND pm.project_id = $1 AND w.slug = $3 \
     LIMIT 1",
    [("role", Int2)]
}

plane_query! {
    /// The target membership of `retrieve`
    /// (`plane/app/views/project/member.py:192-202`) with
    /// `select_related("project", "member", "workspace")`.
    ///
    /// The columns are exactly what the two serializers read:
    /// `ProjectMemberAdminSerializer` (a `ProjectLiteSerializer`, a
    /// `UserAdminLiteSerializer`, a `WorkspaceLiteSerializer`) and
    /// `ProjectMemberRoleSerializer` (`id`, `member`, `role`, `created_at`).
    ///
    /// Bind order: `$1` member row id, `$2` project id, `$3` workspace slug.
    PROJECT_MEMBER_ONE = "projects_member_one",
    "SELECT pm.created_at AS pm_created_at, pm.updated_at AS pm_updated_at, \
       pm.created_by_id AS pm_created_by_id, pm.updated_by_id AS pm_updated_by_id, \
       pm.deleted_at AS pm_deleted_at, pm.id AS pm_id, \
       pm.project_id AS pm_project_id, pm.workspace_id AS pm_workspace_id, \
       pm.member_id AS pm_member_id, pm.comment AS pm_comment, \
       pm.role AS pm_role, pm.view_props AS pm_view_props, \
       pm.default_props AS pm_default_props, pm.preferences AS pm_preferences, \
       pm.sort_order AS pm_sort_order, pm.is_active AS pm_is_active, \
       p.id AS p_id, p.identifier AS p_identifier, p.name AS p_name, \
       p.cover_image AS p_cover_image, \
       p.cover_image_asset_id AS p_cover_image_asset_id, \
       p.logo_props AS p_logo_props, p.description AS p_description, \
       u.id AS u_id, u.first_name AS u_first_name, u.last_name AS u_last_name, \
       u.avatar AS u_avatar, u.avatar_asset_id AS u_avatar_asset_id, \
       u.is_bot AS u_is_bot, u.display_name AS u_display_name, \
       u.email AS u_email, u.last_login_medium AS u_last_login_medium, \
       w.name AS w_name, w.slug AS w_slug, w.id AS w_id, w.logo AS w_logo, \
       w.logo_asset_id AS w_logo_asset_id \
     FROM project_members pm \
     INNER JOIN users u ON (pm.member_id = u.id) \
     INNER JOIN projects p ON (pm.project_id = p.id) \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND NOT u.is_bot \
       AND pm.id = $1 AND pm.project_id = $2 AND w.slug = $3 \
     ORDER BY pm.created_at DESC LIMIT 1",
    [
        ("pm_created_at", Timestamptz),
        ("pm_updated_at", Timestamptz),
        ("pm_created_by_id", Uuid),
        ("pm_updated_by_id", Uuid),
        ("pm_deleted_at", Timestamptz),
        ("pm_id", Uuid),
        ("pm_project_id", Uuid),
        ("pm_workspace_id", Uuid),
        ("pm_member_id", Uuid),
        ("pm_comment", Text),
        ("pm_role", Int2),
        ("pm_view_props", Jsonb),
        ("pm_default_props", Jsonb),
        ("pm_preferences", Jsonb),
        ("pm_sort_order", Float8),
        ("pm_is_active", Bool),
        ("p_id", Uuid),
        ("p_identifier", Varchar),
        ("p_name", Varchar),
        ("p_cover_image", Text),
        ("p_cover_image_asset_id", Uuid),
        ("p_logo_props", Jsonb),
        ("p_description", Text),
        ("u_id", Uuid),
        ("u_first_name", Varchar),
        ("u_last_name", Varchar),
        ("u_avatar", Text),
        ("u_avatar_asset_id", Uuid),
        ("u_is_bot", Bool),
        ("u_display_name", Varchar),
        ("u_email", Varchar),
        ("u_last_login_medium", Varchar),
        ("w_name", Varchar),
        ("w_slug", Varchar),
        ("w_id", Uuid),
        ("w_logo", Text),
        ("w_logo_asset_id", Uuid),
    ]
}

plane_query! {
    /// `ProjectMemberUserEndpoint.get`
    /// (`plane/app/views/project/member.py:378-388`): the caller's own active
    /// membership, serialised by `ProjectMemberSerializer` with
    /// `select_related`-free nested project/workspace/user, which therefore
    /// arrive as three more queries ([`PROJECTS_BY_IDS`],
    /// [`WORKSPACES_BY_IDS`], [`USERS_BY_IDS`]).
    ///
    /// Bind order: `$1` project id, `$2` member id, `$3` workspace slug.
    PROJECT_MEMBER_SELF = "projects_member_self",
    "SELECT pm.created_at, pm.updated_at, pm.created_by_id, pm.updated_by_id, \
       pm.deleted_at, pm.id, pm.project_id, pm.workspace_id, pm.member_id, \
       pm.comment, pm.role, pm.view_props, pm.default_props, pm.preferences, \
       pm.sort_order, pm.is_active, \
       p.id AS p_id, p.identifier AS p_identifier, p.name AS p_name, \
       p.cover_image AS p_cover_image, \
       p.cover_image_asset_id AS p_cover_image_asset_id, \
       p.logo_props AS p_logo_props, p.description AS p_description, \
       u.id AS u_id, u.first_name AS u_first_name, u.last_name AS u_last_name, \
       u.avatar AS u_avatar, u.avatar_asset_id AS u_avatar_asset_id, \
       u.is_bot AS u_is_bot, u.display_name AS u_display_name, \
       u.email AS u_email, u.last_login_medium AS u_last_login_medium, \
       w.name AS w_name, w.slug AS w_slug, w.id AS w_id, w.logo AS w_logo, \
       w.logo_asset_id AS w_logo_asset_id \
     FROM project_members pm \
     INNER JOIN users u ON (pm.member_id = u.id) \
     INNER JOIN projects p ON (pm.project_id = p.id) \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $2 \
       AND pm.project_id = $1 AND w.slug = $3",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("member_id", Uuid),
        ("comment", Text),
        ("role", Int2),
        ("view_props", Jsonb),
        ("default_props", Jsonb),
        ("preferences", Jsonb),
        ("sort_order", Float8),
        ("is_active", Bool),
        ("p_id", Uuid),
        ("p_identifier", Varchar),
        ("p_name", Varchar),
        ("p_cover_image", Text),
        ("p_cover_image_asset_id", Uuid),
        ("p_logo_props", Jsonb),
        ("p_description", Text),
        ("u_id", Uuid),
        ("u_first_name", Varchar),
        ("u_last_name", Varchar),
        ("u_avatar", Text),
        ("u_avatar_asset_id", Uuid),
        ("u_is_bot", Bool),
        ("u_display_name", Varchar),
        ("u_email", Varchar),
        ("u_last_login_medium", Varchar),
        ("w_name", Varchar),
        ("w_slug", Varchar),
        ("w_id", Uuid),
        ("w_logo", Text),
        ("w_logo_asset_id", Uuid),
    ]
}

plane_query! {
    /// `ProjectMemberPreferenceEndpoint.get_queryset`
    /// (`plane/app/views/project/member.py:409-414`): `ProjectMember.objects.get`
    /// with no `is_active` filter.
    ///
    /// Bind order: `$1` project id, `$2` member id, `$3` workspace slug.
    PROJECT_MEMBER_PREFERENCE = "projects_member_preference",
    "SELECT pm.preferences, pm.project_id, pm.member_id, pm.workspace_id \
     FROM project_members pm \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND pm.member_id = $2 AND pm.project_id = $1 \
       AND w.slug = $3",
    [
        ("preferences", Jsonb),
        ("project_id", Uuid),
        ("member_id", Uuid),
        ("workspace_id", Uuid),
    ]
}

// ---------------------------------------------------------------------------
// Deploy boards
// ---------------------------------------------------------------------------

plane_query! {
    /// `DeployBoardViewSet.list` (`plane/app/views/project/base.py:555-561`):
    /// the project's board, `.first()`.
    ///
    /// Bind order: `$1` project id (the `entity_identifier`), `$2` workspace
    /// slug.
    PROJECT_DEPLOY_BOARD = "projects_deploy_board",
    "SELECT db.created_at, db.updated_at, db.created_by_id, db.updated_by_id, \
       db.deleted_at, db.id, db.workspace_id, db.project_id, \
       db.entity_identifier, db.entity_name, db.anchor, \
       db.is_comments_enabled, db.is_reactions_enabled, db.intake_id, \
       db.is_votes_enabled, db.view_props, db.is_activity_enabled, \
       db.is_disabled, \
       p.id AS p_id, p.identifier AS p_identifier, p.name AS p_name, \
       p.cover_image AS p_cover_image, \
       p.cover_image_asset_id AS p_cover_image_asset_id, \
       p.logo_props AS p_logo_props, p.description AS p_description, \
       w.name AS w_name, w.slug AS w_slug, w.id AS w_id, w.logo AS w_logo, \
       w.logo_asset_id AS w_logo_asset_id \
     FROM deploy_boards db \
     LEFT OUTER JOIN projects p ON (db.project_id = p.id) \
     INNER JOIN workspaces w ON (db.workspace_id = w.id) \
     WHERE db.deleted_at IS NULL AND db.entity_identifier = $1 \
       AND db.entity_name = 'project' AND w.slug = $2 \
     ORDER BY db.created_at DESC LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("entity_identifier", Uuid),
        ("entity_name", Varchar),
        ("anchor", Varchar),
        ("is_comments_enabled", Bool),
        ("is_reactions_enabled", Bool),
        ("intake_id", Uuid),
        ("is_votes_enabled", Bool),
        ("view_props", Jsonb),
        ("is_activity_enabled", Bool),
        ("is_disabled", Bool),
        ("p_id", Uuid),
        ("p_identifier", Varchar),
        ("p_name", Varchar),
        ("p_cover_image", Text),
        ("p_cover_image_asset_id", Uuid),
        ("p_logo_props", Jsonb),
        ("p_description", Text),
        ("w_name", Varchar),
        ("w_slug", Varchar),
        ("w_id", Uuid),
        ("w_logo", Text),
        ("w_logo_asset_id", Uuid),
    ]
}

plane_query! {
    /// `DeployBoardViewSet.retrieve`, through `get_queryset()` + `get_object()`
    /// (`plane/app/views/project/base.py:544-553`).
    ///
    /// Bind order: `$1` project id, `$2` project id again (the `project_id`
    /// filter), `$3` workspace slug, `$4` board id.
    PROJECT_DEPLOY_BOARD_ONE = "projects_deploy_board_one",
    "SELECT db.created_at, db.updated_at, db.created_by_id, db.updated_by_id, \
       db.deleted_at, db.id, db.workspace_id, db.project_id, \
       db.entity_identifier, db.entity_name, db.anchor, \
       db.is_comments_enabled, db.is_reactions_enabled, db.intake_id, \
       db.is_votes_enabled, db.view_props, db.is_activity_enabled, \
       db.is_disabled, \
       p.id AS p_id, p.identifier AS p_identifier, p.name AS p_name, \
       p.cover_image AS p_cover_image, \
       p.cover_image_asset_id AS p_cover_image_asset_id, \
       p.logo_props AS p_logo_props, p.description AS p_description, \
       w.name AS w_name, w.slug AS w_slug, w.id AS w_id, w.logo AS w_logo, \
       w.logo_asset_id AS w_logo_asset_id \
     FROM deploy_boards db \
     LEFT OUTER JOIN projects p ON (db.project_id = p.id) \
     INNER JOIN workspaces w ON (db.workspace_id = w.id) \
     WHERE db.deleted_at IS NULL AND db.entity_identifier = $1 \
       AND db.entity_name = 'project' AND db.project_id = $2 \
       AND w.slug = $3 AND db.id = $4",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("entity_identifier", Uuid),
        ("entity_name", Varchar),
        ("anchor", Varchar),
        ("is_comments_enabled", Bool),
        ("is_reactions_enabled", Bool),
        ("intake_id", Uuid),
        ("is_votes_enabled", Bool),
        ("view_props", Jsonb),
        ("is_activity_enabled", Bool),
        ("is_disabled", Bool),
        ("p_id", Uuid),
        ("p_identifier", Varchar),
        ("p_name", Varchar),
        ("p_cover_image", Text),
        ("p_cover_image_asset_id", Uuid),
        ("p_logo_props", Jsonb),
        ("p_description", Text),
        ("w_name", Varchar),
        ("w_slug", Varchar),
        ("w_id", Uuid),
        ("w_logo", Text),
        ("w_logo_asset_id", Uuid),
    ]
}

// ---------------------------------------------------------------------------
// Nested serializers: one query per relation, for the whole page
// ---------------------------------------------------------------------------

plane_query! {
    /// `Workspace.objects.filter(pk__in=…)` for a `WorkspaceLiteSerializer`.
    ///
    /// Bind: `$1` workspace ids.
    WORKSPACES_BY_IDS = "projects_workspaces_by_ids",
    "SELECT w.name, w.slug, w.id, w.logo, w.logo_asset_id FROM workspaces w \
     WHERE w.deleted_at IS NULL AND w.id = ANY($1)",
    [
        ("name", Varchar),
        ("slug", Varchar),
        ("id", Uuid),
        ("logo", Text),
        ("logo_asset_id", Uuid),
    ]
}

plane_query! {
    /// `Project.objects.filter(pk__in=…)` for a `ProjectLiteSerializer`.
    ///
    /// Bind: `$1` project ids.
    PROJECTS_BY_IDS = "projects_projects_by_ids",
    "SELECT p.id, p.identifier, p.name, p.cover_image, p.cover_image_asset_id, \
       p.logo_props, p.description \
     FROM projects p \
     WHERE p.deleted_at IS NULL AND p.id = ANY($1)",
    [
        ("id", Uuid),
        ("identifier", Varchar),
        ("name", Varchar),
        ("cover_image", Text),
        ("cover_image_asset_id", Uuid),
        ("logo_props", Jsonb),
        ("description", Text),
    ]
}

plane_query! {
    /// `User` rows for `UserLiteSerializer` / `UserAdminLiteSerializer`.
    ///
    /// Bind: `$1` user ids.
    USERS_BY_IDS = "projects_users_by_ids",
    "SELECT u.id, u.first_name, u.last_name, u.avatar, u.avatar_asset_id, \
       u.is_bot, u.display_name, u.email, u.last_login_medium \
     FROM users u WHERE u.id = ANY($1)",
    [
        ("id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("avatar", Text),
        ("avatar_asset_id", Uuid),
        ("is_bot", Bool),
        ("display_name", Varchar),
        ("email", Varchar),
        ("last_login_medium", Varchar),
    ]
}

plane_query! {
    /// `FileAsset` rows for the `*_url` properties of the nested serializers
    /// (`Project.cover_image_url`, `Workspace.logo_url`, `User.avatar_url`).
    ///
    /// `FileAsset.asset_url` needs the workspace slug for the issue-asset
    /// branches, so it is joined here (Django loads it lazily, one query per
    /// asset).
    ///
    /// Bind: `$1` asset ids.
    ASSETS_BY_IDS = "projects_assets_by_ids",
    "SELECT fa.id, fa.entity_type, fa.project_id, fa.issue_id, w.slug \
     FROM file_assets fa \
     LEFT OUTER JOIN workspaces w ON (fa.workspace_id = w.id) \
     WHERE fa.deleted_at IS NULL AND fa.id = ANY($1)",
    [
        ("id", Uuid),
        ("entity_type", Varchar),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("slug", Varchar),
    ]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT_LIST_ADMIN,
    &PROJECT_LIST_GUEST,
    &PROJECT_LIST_MEMBER,
    &WORKSPACE_MEMBER_HAS_ROLE,
    &PROJECT_DETAIL_ONE,
    &PROJECT_MEMBERS_PREFETCH,
    &ISSUE_SEQUENCE_MAX,
    &PROJECT_IDENTIFIERS,
    &PROJECT_INVITATIONS,
    &PROJECT_INVITATION_ONE,
    &PROJECT_INVITATION_PUBLIC,
    &PROJECT_MEMBERS,
    &PROJECT_MEMBER_SELF_ROLE,
    &PROJECT_MEMBER_ONE,
    &PROJECT_MEMBER_SELF,
    &PROJECT_MEMBER_PREFERENCE,
    &PROJECT_DEPLOY_BOARD,
    &PROJECT_DEPLOY_BOARD_ONE,
    &WORKSPACES_BY_IDS,
    &PROJECTS_BY_IDS,
    &USERS_BY_IDS,
    &ASSETS_BY_IDS,
];
