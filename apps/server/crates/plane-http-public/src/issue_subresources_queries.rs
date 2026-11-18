// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Static statements of the v1 work-item sub-resource **writes**
//! (`plane/api/views/issue.py:1274-1451,1551-1822,2025-2417,2715-2801`).
//!
//! Inserts and full-row updates are imported from the app-surface catalogs
//! (`plane_http_app::issues::write_sub_queries`,
//! `plane_http_app::issues::write_comment_queries` and
//! `plane_http_app::assets::write_queries`): the models and the column sets are
//! the same, and those statements are already `schema_check`ed there. This file
//! declares the v1-specific **lookups**, which differ from the app views': v1
//! reads the pre-image from the default manager with no membership or
//! archived-project filter (`IssueLink.objects.get(workspace__slug=…, …)`), and
//! the activity writers need the exact rows the Python re-reads.

use plane_db::catalog::Query;

plane_db::plane_query! {
    /// `Issue.objects.get(workspace__slug=slug, project_id=project_id, pk=…)`
    /// — the bind every sub-resource write opens with (`issue.py:1278-1282`,
    /// `:1555-1559`, `:2028`, `:2221`, `:2739`). `Issue.objects` is the
    /// soft-deletion manager, **not** `issue_objects`: drafts, triage and
    /// archived work items are reachable here.
    ISSUE_ANY = "v1_sub_issue_any",
    "SELECT issues.id, issues.created_by_id FROM issues \
     INNER JOIN workspaces ON workspaces.id = issues.workspace_id \
     WHERE issues.deleted_at IS NULL AND workspaces.slug = $1 \
       AND issues.project_id = $2 AND issues.id = $3",
    [("id", Uuid), ("created_by_id", Uuid)]
}

// ---------------------------------------------------------------------------
// Links (319/331, 320/332)
// ---------------------------------------------------------------------------

plane_db::plane_query! {
    /// `IssueLink.objects.get(workspace__slug=…, project_id=…, issue_id=…,
    /// pk=…)` — the PATCH/DELETE pre-image (`issue.py:1376`,`:1441`). No
    /// membership or archived-project filter.
    LINK_BY_PK = "v1_sub_link_by_pk",
    "SELECT l.created_at, l.updated_at, l.created_by_id, l.updated_by_id, \
       l.deleted_at, l.id, l.project_id, l.workspace_id, l.title, l.url, \
       l.issue_id, l.metadata \
     FROM issue_links l \
     INNER JOIN workspaces w ON w.id = l.workspace_id \
     WHERE l.deleted_at IS NULL AND w.slug = $1 AND l.project_id = $2 \
       AND l.issue_id = $3 AND l.id = $4",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("title", Varchar),
        ("url", Text),
        ("issue_id", Uuid),
        ("metadata", Jsonb),
    ]
}

plane_db::plane_query! {
    /// `IssueLinkCreateSerializer.create`'s duplicate check
    /// (`plane/api/serializers/issue.py:461-464`):
    /// `IssueLink.objects.filter(url=…, issue_id=…).exists()`. The `issue_id`
    /// is the view's kwargs value from `save(issue_id=…)`, not the body.
    LINK_URL_TAKEN = "v1_sub_link_url_taken",
    "SELECT EXISTS (SELECT 1 FROM issue_links l \
     WHERE l.deleted_at IS NULL AND l.url = $1 AND l.issue_id = $2) AS exists",
    [("exists", Bool)]
}

// ---------------------------------------------------------------------------
// Comments (321/333, 322/334)
// ---------------------------------------------------------------------------

plane_db::plane_query! {
    /// `IssueComment.objects.get(workspace__slug=…, project_id=…, issue_id=…,
    /// pk=…)` (`issue.py:1715`,`:1784` and the two re-reads after a write).
    /// No membership filter and no `is_member` annotation, so the write
    /// responses omit that key (DRF `SkipField`).
    COMMENT_BY_PK = "v1_sub_comment_by_pk",
    "SELECT c.created_at, c.updated_at, c.created_by_id, c.updated_by_id, \
       c.deleted_at, c.id, c.project_id, c.workspace_id, c.comment_stripped, \
       c.comment_json, c.comment_html, c.description_id, c.attachments, \
       c.issue_id, c.actor_id, c.access, c.external_source, c.external_id, \
       c.edited_at, c.parent_id \
     FROM issue_comments c \
     INNER JOIN workspaces w ON w.id = c.workspace_id \
     WHERE c.deleted_at IS NULL AND w.slug = $1 AND c.project_id = $2 \
       AND c.issue_id = $3 AND c.id = $4",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("comment_stripped", Text),
        ("comment_json", Jsonb),
        ("comment_html", Text),
        ("description_id", Uuid),
        ("attachments", ArrayOf(Varchar)),
        ("issue_id", Uuid),
        ("actor_id", Uuid),
        ("access", Varchar),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("edited_at", Timestamptz),
        ("parent_id", Uuid),
    ]
}

plane_db::plane_query! {
    /// The first `super(IssueComment, self).save()` on create
    /// (`plane/db/models/issue.py:503-512`).
    ///
    /// The app surface's `comments_comment_insert` hard-codes the four columns
    /// its serializer cannot write (`app/views/issue/comment.py:25`); the v1
    /// `IssueCommentCreateSerializer` accepts `external_source`/`external_id`
    /// (`api/serializers/issue.py:723-753`), so this statement binds them.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, id,
    /// project, workspace, comment_stripped, comment_json, comment_html,
    /// attachments, issue, actor, access, external_source, external_id.
    COMMENT_INSERT = "v1_sub_comment_insert",
    "INSERT INTO issue_comments \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, comment_stripped, comment_json, comment_html, \
      description_id, attachments, issue_id, actor_id, access, external_source, \
      external_id, edited_at, parent_id) \
     VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10, NULL, $11, $12, \
       $13, $14, $15, $16, NULL, NULL)",
    []
}

plane_db::plane_query! {
    /// `IssueCommentListCreateAPIEndpoint.post`'s duplicate check
    /// (`issue.py:1568-1578`): the `.first()` row of the
    /// `(project, workspace, external_source, external_id)` filter, in
    /// `-created_at` order, whose id goes into the 409 body.
    COMMENT_EXTERNAL_FIRST = "v1_sub_comment_external_first",
    "SELECT c.id FROM issue_comments c \
     INNER JOIN workspaces w ON w.id = c.workspace_id \
     WHERE c.deleted_at IS NULL AND c.project_id = $1 AND w.slug = $2 \
       AND c.external_source = $3 AND c.external_id = $4 \
     ORDER BY c.created_at DESC LIMIT 1",
    [("id", Uuid)]
}

plane_db::plane_query! {
    /// `IssueCommentDetailAPIEndpoint.patch`'s duplicate check
    /// (`issue.py:1731-1745`): the same filter `.exists()`. The 409 body
    /// reports the URL object's id, not the conflicting row's.
    COMMENT_EXTERNAL_EXISTS = "v1_sub_comment_external_exists",
    // `$3` is an `Optional<String>`: Django's `.filter(external_source=None)`
    // is `IS NULL`, not `= NULL`, and the PATCH check forwards the instance's
    // source when the body does not name one.
    "SELECT EXISTS (SELECT 1 FROM issue_comments c \
     INNER JOIN workspaces w ON w.id = c.workspace_id \
     WHERE c.deleted_at IS NULL AND c.project_id = $1 AND w.slug = $2 \
       AND ((c.external_source = $3) \
            OR (c.external_source IS NULL AND $3::varchar IS NULL)) \
       AND c.external_id = $4) AS exists",
    [("exists", Bool)]
}

// ---------------------------------------------------------------------------
// Permission helpers (`issue.py:168-213`)
// ---------------------------------------------------------------------------

plane_db::plane_query! {
    /// `is_active_project_member` (`issue.py:183-185`):
    /// `ProjectMember.objects.filter(project_id=…, member_id=…,
    /// is_active=True).exists()`.
    PROJECT_MEMBER_ACTIVE = "v1_sub_member_active",
    "SELECT EXISTS (SELECT 1 FROM project_members pm \
     WHERE pm.project_id = $1 AND pm.member_id = $2 AND pm.is_active \
       AND pm.deleted_at IS NULL) AS exists",
    [("exists", Bool)]
}

plane_db::plane_query! {
    /// `user_has_issue_permission(…, allowed_roles=[ADMIN, MEMBER, GUEST])`
    /// (`issue.py:168-180`) — admin, member or guest, active membership only.
    PROJECT_MEMBER_ROLE_ALLOWED = "v1_sub_member_role_allowed",
    "SELECT EXISTS (SELECT 1 FROM project_members pm \
     WHERE pm.project_id = $1 AND pm.member_id = $2 AND pm.is_active \
       AND pm.deleted_at IS NULL AND pm.role IN (20, 15, 5)) AS exists",
    [("exists", Bool)]
}

plane_db::plane_query! {
    /// `is_project_or_workspace_admin` (`issue.py:187-196`): the caller's
    /// project row, then `role == 20` or an active workspace admin of the same
    /// workspace. `ProjectMember.objects.first()` is `-created_at` order.
    PROJECT_MEMBER_ADMIN = "v1_sub_member_admin",
    "SELECT pm.role, \
       EXISTS (SELECT 1 FROM workspace_members wm \
               WHERE wm.workspace_id = pm.workspace_id \
                 AND wm.member_id = pm.member_id AND wm.role = 20 \
                 AND wm.is_active AND wm.deleted_at IS NULL) AS workspace_admin \
     FROM project_members pm \
     INNER JOIN workspaces w ON w.id = pm.workspace_id \
     WHERE w.slug = $1 AND pm.project_id = $2 AND pm.member_id = $3 \
       AND pm.is_active AND pm.deleted_at IS NULL \
     ORDER BY pm.created_at DESC LIMIT 1",
    [("role", Int2), ("workspace_admin", Bool)]
}

// ---------------------------------------------------------------------------
// Attachments (325/337, 326/338)
// ---------------------------------------------------------------------------

plane_db::plane_query! {
    /// `Workspace.objects.get(slug=slug)` (`issue.py:2089`): the default
    /// (soft-deletion) manager.
    WORKSPACE_ID_BY_SLUG = "v1_sub_workspace_by_slug",
    "SELECT w.id FROM workspaces w \
     WHERE w.slug = $1 AND w.deleted_at IS NULL",
    [("id", Uuid)]
}

plane_db::plane_query! {
    /// The attachment POST's duplicate check (`issue.py:2100-2114`): the
    /// `.first()` `FileAsset` row (in `-created_at` order) whose
    /// `(project, workspace, external_source, external_id, issue,
    /// entity_type)` matches, whose id goes into the 409 body.
    FILE_ASSET_EXTERNAL_FIRST = "v1_sub_asset_external_first",
    "SELECT fa.id FROM file_assets fa \
     INNER JOIN workspaces w ON w.id = fa.workspace_id \
     WHERE fa.deleted_at IS NULL AND fa.project_id = $1 AND w.slug = $2 \
       AND fa.external_source = $3 AND fa.external_id = $4 \
       AND fa.issue_id = $5 AND fa.entity_type = 'ISSUE_ATTACHMENT' \
     ORDER BY fa.created_at DESC LIMIT 1",
    [("id", Uuid)]
}

plane_db::plane_query! {
    /// `FileAsset.objects.get(id=…, workspace__slug=…, project_id=…,
    /// issue_id=…, entity_type=…ISSUE_ATTACHMENT)` — the PATCH/DELETE
    /// pre-image (`issue.py:2217-2223`,`:2345-2351`,`:2374-2380`). The
    /// soft-deletion manager applies; `is_deleted` is **not** filtered.
    FILE_ASSET_BY_PK = "v1_sub_asset_by_pk",
    "SELECT fa.created_at, fa.updated_at, fa.created_by_id, fa.updated_by_id, \
       fa.deleted_at, fa.id, fa.attributes, fa.asset, fa.user_id, \
       fa.workspace_id, fa.draft_issue_id, fa.project_id, fa.issue_id, \
       fa.comment_id, fa.page_id, fa.entity_type, fa.entity_identifier, \
       fa.is_deleted, fa.is_archived, fa.external_id, fa.external_source, \
       fa.size, fa.is_uploaded, fa.storage_metadata \
     FROM file_assets fa \
     INNER JOIN workspaces w ON w.id = fa.workspace_id \
     WHERE fa.deleted_at IS NULL AND w.slug = $1 AND fa.project_id = $2 \
       AND fa.issue_id = $3 AND fa.entity_type = 'ISSUE_ATTACHMENT' \
       AND fa.id = $4",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("attributes", Jsonb),
        ("asset", Varchar),
        ("user_id", Uuid),
        ("workspace_id", Uuid),
        ("draft_issue_id", Uuid),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("comment_id", Uuid),
        ("page_id", Uuid),
        ("entity_type", Varchar),
        ("entity_identifier", Varchar),
        ("is_deleted", Bool),
        ("is_archived", Bool),
        ("external_id", Varchar),
        ("external_source", Varchar),
        ("size", Float8),
        ("is_uploaded", Bool),
        ("storage_metadata", Jsonb),
    ]
}

// ---------------------------------------------------------------------------
// Relations (339)
// ---------------------------------------------------------------------------

plane_db::plane_query! {
    /// `Project.objects.get(pk=project_id, workspace__slug=slug)`
    /// (`issue.py:2736`) — the soft-deletion manager with the workspace join.
    PROJECT_IN_SLUG = "v1_sub_project_in_slug",
    "SELECT p.id, p.workspace_id FROM projects p \
     INNER JOIN workspaces w ON w.id = p.workspace_id \
     WHERE p.deleted_at IS NULL AND p.id = $1 AND w.slug = $2",
    [("id", Uuid), ("workspace_id", Uuid)]
}

plane_db::plane_query! {
    /// The reverse-type refetch (`issue.py:2777-2790`): rows whose
    /// `issue_id` is a **body** issue and whose `related_issue_id` is the URL
    /// issue, joined to the reply side (`RelatedIssueSerializer` reads `issue`,
    /// `serializers/issue.py:657-700`) and to its state/type. `-created_at`
    /// is `IssueRelation.Meta.ordering` (`db/models/issue.py:317`).
    RELATION_REFETCH_REVERSE = "v1_sub_relation_refetch_reverse",
    "SELECT r.created_at, r.updated_at, r.created_by_id, r.updated_by_id, \
       r.deleted_at, r.id, r.project_id, r.workspace_id, r.issue_id, \
       r.related_issue_id, r.relation_type, \
       i.project_id, i.sequence_id, i.name, i.state_id, i.priority, \
       i.type_id, it.is_epic \
     FROM issue_relations r \
     INNER JOIN workspaces w ON w.id = r.workspace_id \
     INNER JOIN issues i ON i.id = r.issue_id \
     LEFT JOIN issue_types it ON it.id = i.type_id \
     WHERE r.deleted_at IS NULL AND w.slug = $1 \
       AND r.issue_id = ANY($2) AND r.related_issue_id = $3 \
       AND r.relation_type = $4 \
     ORDER BY r.created_at DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("issue_id", Uuid),
        ("related_issue_id", Uuid),
        ("relation_type", Varchar),
        ("project_id", Uuid),
        ("sequence_id", Int4),
        ("name", Varchar),
        ("state_id", Uuid),
        ("priority", Varchar),
        ("type_id", Uuid),
        ("is_epic", Bool),
    ]
}

plane_db::plane_query! {
    /// The forward-type refetch (`issue.py:2777-2790`): rows whose `issue_id`
    /// is the URL issue and whose `related_issue_id` is a body issue, joined to
    /// the reply side (`IssueRelationSerializer` reads `related_issue`,
    /// `serializers/issue.py:616-654`).
    RELATION_REFETCH_FORWARD = "v1_sub_relation_refetch_forward",
    "SELECT r.created_at, r.updated_at, r.created_by_id, r.updated_by_id, \
       r.deleted_at, r.id, r.project_id, r.workspace_id, r.issue_id, \
       r.related_issue_id, r.relation_type, \
       i.project_id, i.sequence_id, i.name, i.state_id, i.priority, \
       i.type_id, it.is_epic \
     FROM issue_relations r \
     INNER JOIN workspaces w ON w.id = r.workspace_id \
     INNER JOIN issues i ON i.id = r.related_issue_id \
     LEFT JOIN issue_types it ON it.id = i.type_id \
     WHERE r.deleted_at IS NULL AND w.slug = $1 \
       AND r.issue_id = $3 AND r.related_issue_id = ANY($2) \
       AND r.relation_type = $4 \
     ORDER BY r.created_at DESC",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("issue_id", Uuid),
        ("related_issue_id", Uuid),
        ("relation_type", Varchar),
        ("project_id", Uuid),
        ("sequence_id", Int4),
        ("name", Varchar),
        ("state_id", Uuid),
        ("priority", Varchar),
        ("type_id", Uuid),
        ("is_epic", Bool),
    ]
}

/// Every statement this file declares, for `tests/schema_check.rs`.
pub static ALL_QUERIES: &[&Query] = &[
    &ISSUE_ANY,
    &LINK_BY_PK,
    &LINK_URL_TAKEN,
    &COMMENT_INSERT,
    &COMMENT_BY_PK,
    &COMMENT_EXTERNAL_FIRST,
    &COMMENT_EXTERNAL_EXISTS,
    &PROJECT_MEMBER_ACTIVE,
    &PROJECT_MEMBER_ROLE_ALLOWED,
    &PROJECT_MEMBER_ADMIN,
    &WORKSPACE_ID_BY_SLUG,
    &FILE_ASSET_EXTERNAL_FIRST,
    &FILE_ASSET_BY_PK,
    &PROJECT_IN_SLUG,
    &RELATION_REFETCH_REVERSE,
    &RELATION_REFETCH_FORWARD,
];
