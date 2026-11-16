// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the comment/reaction write routes (81, 82, 86,
//! 87, 88, 89).
//!
//! The reads are the ORM's own predicates: the default manager's
//! `deleted_at IS NULL`, the `workspace__slug` join, and — for the write
//! responses — the same joined projection the comment readers use
//! ([`super::sub_queries::COMMENTS`]) so [`super::sub_common::CommentRow`]
//! decodes it. Unlike the readers, the write routes do not filter by
//! membership or `archived_at`: the Python views look the row up with
//! `IssueComment.objects.get(...)` only.

use plane_db::Query;
use plane_db::plane_query;

// ---------------------------------------------------------------------------
// Lookups
// ---------------------------------------------------------------------------

plane_query! {
    /// `Project.objects.get(pk=project_id)` — route 81's guest rule
    /// (`plane/app/views/issue/comment.py:123`). The view reads the workspace id
    /// and `guest_view_all_features`; no slug filter.
    ///
    /// Bind order: `$1` project id.
    PROJECT_FOR_COMMENT = "comments_project_for_comment",
    "SELECT p.id, p.workspace_id, p.guest_view_all_features FROM projects p \
     WHERE p.deleted_at IS NULL AND p.id = $1 LIMIT 1",
    [("id", Uuid), ("workspace_id", Uuid), ("guest_view_all_features", Bool)]
}

plane_query! {
    /// `Issue.objects.filter(pk=issue_id, workspace__slug=slug,
    /// project_id=project_id).first()` (`comment.py:131-132`).
    ///
    /// Bind order: `$1` issue id, `$2` workspace slug, `$3` project id.
    ISSUE_FOR_COMMENT = "comments_issue_for_comment",
    "SELECT i.id, i.workspace_id, i.created_by_id FROM issues i \
     INNER JOIN workspaces w ON (i.workspace_id = w.id) \
     WHERE i.deleted_at IS NULL AND i.id = $1 AND w.slug = $2 \
       AND i.project_id = $3 LIMIT 1",
    [("id", Uuid), ("workspace_id", Uuid), ("created_by_id", Uuid)]
}

plane_query! {
    /// The requesting user's active project membership with role 5 — the
    /// `ProjectMember.objects.filter(..., role=5, is_active=True).exists()`
    /// half of the guest rule (`comment.py:134-141`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id.
    PROJECT_GUEST_MEMBERSHIP = "comments_project_guest_membership",
    "SELECT 1 AS one FROM project_members pm \
     INNER JOIN workspaces w ON (pm.workspace_id = w.id) \
     WHERE pm.deleted_at IS NULL AND w.slug = $1 AND pm.project_id = $2 \
       AND pm.member_id = $3 AND pm.role = 5 AND pm.is_active LIMIT 1",
    [("one", Int4)]
}

plane_query! {
    /// One `IssueComment` with the nested relations `IssueCommentSerializer`
    /// renders — [`super::sub_queries::COMMENTS`]'s projection without the
    /// `is_member` annotation and without its membership/archived filters.
    ///
    /// Bind order: `$1` comment id, `$2` workspace slug, `$3` project id,
    /// `$4` issue id.
    COMMENT_ROW_BY_ID = "comments_comment_row_by_id",
    "SELECT c.created_at, c.updated_at, c.created_by_id, c.updated_by_id, \
       c.deleted_at, c.id, c.project_id, c.workspace_id, c.comment_stripped, \
       c.comment_json, c.comment_html, c.description_id, c.attachments, \
       c.issue_id, c.actor_id, c.access, c.external_source, c.external_id, \
       c.edited_at, c.parent_id, \
       i.id, i.name, i.description_json, i.description_html, i.priority, \
       i.start_date, i.target_date, i.sequence_id, i.sort_order, i.is_draft, \
       pj.id, pj.identifier, pj.name, pj.cover_image, pj.logo_props, \
       pj.description, \
       pca.id, pca.entity_type, pca.project_id, pca.issue_id, pcaw.slug, \
       w.name, w.slug, w.id, w.logo, \
       wla.id, wla.entity_type, wla.project_id, wla.issue_id, wlaw.slug, \
       u.id, u.first_name, u.last_name, u.avatar, u.is_bot, u.display_name, \
       ua.id, ua.entity_type, ua.project_id, ua.issue_id, uaw.slug \
     FROM issue_comments c \
     INNER JOIN issues i ON (i.id = c.issue_id) \
     INNER JOIN projects pj ON (pj.id = c.project_id) \
     INNER JOIN workspaces w ON (w.id = c.workspace_id) \
     LEFT JOIN file_assets pca ON (pca.id = pj.cover_image_asset_id) \
     LEFT JOIN workspaces pcaw ON (pcaw.id = pca.workspace_id) \
     LEFT JOIN file_assets wla ON (wla.id = w.logo_asset_id) \
     LEFT JOIN workspaces wlaw ON (wlaw.id = wla.workspace_id) \
     LEFT JOIN users u ON (u.id = c.actor_id) \
     LEFT JOIN file_assets ua ON (ua.id = u.avatar_asset_id) \
     LEFT JOIN workspaces uaw ON (uaw.id = ua.workspace_id) \
     WHERE c.deleted_at IS NULL AND c.id = $1 AND w.slug = $2 \
       AND c.project_id = $3 AND c.issue_id = $4 AND i.project_id = $3 \
     LIMIT 1",
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
        ("id", Uuid),
        ("name", Varchar),
        ("description_json", Jsonb),
        ("description_html", Text),
        ("priority", Varchar),
        ("start_date", Date),
        ("target_date", Date),
        ("sequence_id", Int4),
        ("sort_order", Float8),
        ("is_draft", Bool),
        ("id", Uuid),
        ("identifier", Varchar),
        ("name", Varchar),
        ("cover_image", Text),
        ("logo_props", Jsonb),
        ("description", Text),
        ("id", Uuid),
        ("entity_type", Varchar),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("slug", Varchar),
        ("name", Varchar),
        ("slug", Varchar),
        ("id", Uuid),
        ("logo", Text),
        ("id", Uuid),
        ("entity_type", Varchar),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("slug", Varchar),
        ("id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("avatar", Text),
        ("is_bot", Bool),
        ("display_name", Varchar),
        ("id", Uuid),
        ("entity_type", Varchar),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("slug", Varchar),
    ]
}

// ---------------------------------------------------------------------------
// Comments
// ---------------------------------------------------------------------------

plane_query! {
    /// The first `super(IssueComment, self).save()` on create
    /// (`plane/db/models/issue.py:504`): `description_id` is NULL here; the
    /// second, column-only save sets it.
    ///
    /// Bind order: created_at, updated_at, created_by, updated_by, id,
    /// project, workspace, comment_stripped, comment_json, comment_html,
    /// attachments, issue, actor, access.
    COMMENT_INSERT = "comments_comment_insert",
    "INSERT INTO issue_comments \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, comment_stripped, comment_json, comment_html, \
      description_id, attachments, issue_id, actor_id, access, external_source, \
      external_id, edited_at, parent_id) \
     VALUES ($1, $2, $3, $4, NULL, $5, $6, $7, $8, $9, $10, NULL, $11, $12, \
       $13, $14, NULL, NULL, NULL, NULL)",
    []
}

plane_query! {
    /// The second save: `super().save(update_fields=["description_id"])`
    /// (`issue.py:510`). A column-only UPDATE: no `updated_at`, no audit.
    ///
    /// Bind order: `$1` description id, `$2` comment id.
    COMMENT_SET_DESCRIPTION = "comments_set_description",
    "UPDATE issue_comments SET description_id = $1 WHERE id = $2",
    []
}

plane_query! {
    /// `IssueComment.save()` on an existing row: a full-row UPDATE (no
    /// `update_fields`), after H14 recomputed `comment_stripped` and H01 set
    /// `updated_by`/`updated_at`.
    ///
    /// Bind order: every column but the id, then the id.
    COMMENT_UPDATE = "comments_comment_update",
    "UPDATE issue_comments SET created_at = $1, updated_at = $2, \
       created_by_id = $3, updated_by_id = $4, deleted_at = $5, id = $6, \
       project_id = $7, workspace_id = $8, comment_stripped = $9, \
       comment_json = $10, comment_html = $11, description_id = $12, \
       attachments = $13, issue_id = $14, actor_id = $15, access = $16, \
       external_source = $17, external_id = $18, edited_at = $19, \
       parent_id = $20 \
     WHERE id = $21",
    []
}

plane_query! {
    /// `Description.objects.create(...)` from `IssueComment.save`
    /// (`issue.py:508`); H28 derives `description_stripped`, H01 stamps the
    /// request user in `created_by_id`.
    ///
    /// Bind order: created_at, updated_at, id, description_json,
    /// description_html, description_stripped, created_by, project, workspace.
    DESCRIPTION_INSERT = "comments_description_insert",
    "INSERT INTO descriptions \
     (created_at, updated_at, deleted_at, id, description_json, \
      description_html, description_binary, description_stripped, \
      created_by_id, project_id, updated_by_id, workspace_id) \
     VALUES ($1, $2, NULL, $3, $4, $5, NULL, $6, $7, $8, NULL, $9)",
    []
}

// ---------------------------------------------------------------------------
// Issue reactions (routes 86/87)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueReaction.objects.create(...)` after the serializer save
    /// (`plane/app/views/issue/reaction.py:62`). The workspace comes from
    /// `ProjectBaseModel.save` (`issue.project.workspace`).
    ///
    /// Bind order: created_at, updated_at, id, reaction, actor, created_by,
    /// issue, project, workspace.
    ISSUE_REACTION_INSERT = "comments_issue_reaction_insert",
    "INSERT INTO issue_reactions \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, actor_id, issue_id, reaction) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, $9)",
    []
}

plane_query! {
    /// The created reaction + its actor, with the same projection
    /// `super::sub_queries::ISSUE_REACTIONS` uses (reaction row then the user
    /// and its avatar asset).
    ///
    /// Bind order: `$1` reaction id.
    ISSUE_REACTION_BY_ID = "comments_issue_reaction_by_id",
    "SELECT r.created_at, r.updated_at, r.created_by_id, r.updated_by_id, \
       r.deleted_at, r.id, r.project_id, r.workspace_id, r.actor_id, \
       r.issue_id, r.reaction, u.id, u.first_name, u.last_name, u.avatar, \
       u.is_bot, u.display_name, ua.id, ua.entity_type, ua.project_id, \
       ua.issue_id, uaw.slug \
     FROM issue_reactions r \
     LEFT JOIN users u ON (u.id = r.actor_id) \
     LEFT JOIN file_assets ua ON (ua.id = u.avatar_asset_id) \
     LEFT JOIN workspaces uaw ON (uaw.id = ua.workspace_id) \
     WHERE r.deleted_at IS NULL AND r.id = $1 LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("actor_id", Uuid),
        ("issue_id", Uuid),
        ("reaction", Text),
        ("id", Uuid),
        ("first_name", Varchar),
        ("last_name", Varchar),
        ("avatar", Text),
        ("is_bot", Bool),
        ("display_name", Varchar),
        ("id", Uuid),
        ("entity_type", Varchar),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("slug", Varchar),
    ]
}

plane_query! {
    /// `Issue.objects.get(pk=issue_id, workspace__slug=slug,
    /// project_id=project_id)` for the reaction create
    /// (`reaction.py:59`). A miss is `DoesNotExist` → the family 404.
    ///
    /// Bind order: `$1` issue id, `$2` workspace slug, `$3` project id.
    ISSUE_FOR_REACTION = "comments_issue_for_reaction",
    "SELECT i.id, i.workspace_id FROM issues i \
     INNER JOIN workspaces w ON (i.workspace_id = w.id) \
     WHERE i.deleted_at IS NULL AND i.id = $1 AND w.slug = $2 \
       AND i.project_id = $3 LIMIT 1",
    [("id", Uuid), ("workspace_id", Uuid)]
}

plane_query! {
    /// `IssueReaction.objects.get(workspace__slug=slug, project_id=…,
    /// issue_id=…, reaction=code, actor=request.user)` — the destroy lookup
    /// (`reaction.py:79-85`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` issue id,
    /// `$4` reaction code, `$5` actor id.
    ISSUE_REACTION_TARGET = "comments_issue_reaction_target",
    "SELECT r.id, r.reaction FROM issue_reactions r \
     INNER JOIN workspaces w ON (r.workspace_id = w.id) \
     WHERE r.deleted_at IS NULL AND w.slug = $1 AND r.project_id = $2 \
       AND r.issue_id = $3 AND r.reaction = $4 AND r.actor_id = $5 LIMIT 1",
    [("id", Uuid), ("reaction", Text)]
}

// ---------------------------------------------------------------------------
// Comment reactions (routes 88/89)
// ---------------------------------------------------------------------------

plane_query! {
    /// `CommentReaction.objects.create(...)` (`comment.py:264-269`); the
    /// workspace comes from the comment's project.
    ///
    /// Bind order: created_at, updated_at, id, reaction, actor, created_by,
    /// comment, project, workspace.
    COMMENT_REACTION_INSERT = "comments_comment_reaction_insert",
    "INSERT INTO comment_reactions \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, actor_id, comment_id, reaction) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, $9)",
    []
}

plane_query! {
    /// The created reaction with `display_name` (`CommentReactionSerializer`'s
    /// `source="actor.display_name"`).
    ///
    /// Bind order: `$1` reaction id.
    COMMENT_REACTION_BY_ID = "comments_comment_reaction_by_id",
    "SELECT r.created_at, r.updated_at, r.created_by_id, r.updated_by_id, \
       r.deleted_at, r.id, r.project_id, r.workspace_id, r.actor_id, \
       r.comment_id, r.reaction, u.display_name \
     FROM comment_reactions r \
     INNER JOIN users u ON (u.id = r.actor_id) \
     WHERE r.deleted_at IS NULL AND r.id = $1 LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("actor_id", Uuid),
        ("comment_id", Uuid),
        ("reaction", Text),
        ("display_name", Varchar),
    ]
}

plane_query! {
    /// `IssueComment.objects.get(pk=comment_id, workspace__slug=slug,
    /// project_id=project_id)` for the reaction create (`comment.py:261-262`).
    ///
    /// Bind order: `$1` comment id, `$2` workspace slug, `$3` project id.
    COMMENT_FOR_REACTION = "comments_comment_for_reaction",
    "SELECT c.id, c.workspace_id, c.issue_id FROM issue_comments c \
     INNER JOIN workspaces w ON (c.workspace_id = w.id) \
     WHERE c.deleted_at IS NULL AND c.id = $1 AND w.slug = $2 \
       AND c.project_id = $3 LIMIT 1",
    [("id", Uuid), ("workspace_id", Uuid), ("issue_id", Uuid)]
}

plane_query! {
    /// `CommentReaction.objects.get(workspace__slug=slug, project_id=…,
    /// comment_id=…, reaction=code, actor=request.user)` — the destroy lookup
    /// (`comment.py:281-287`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` comment id,
    /// `$4` reaction code, `$5` actor id.
    COMMENT_REACTION_TARGET = "comments_comment_reaction_target",
    "SELECT r.id, r.reaction FROM comment_reactions r \
     INNER JOIN workspaces w ON (r.workspace_id = w.id) \
     WHERE r.deleted_at IS NULL AND w.slug = $1 AND r.project_id = $2 \
       AND r.comment_id = $3 AND r.reaction = $4 AND r.actor_id = $5 LIMIT 1",
    [("id", Uuid), ("reaction", Text)]
}

/// Every catalogued statement of this file, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &PROJECT_FOR_COMMENT,
    &ISSUE_FOR_COMMENT,
    &PROJECT_GUEST_MEMBERSHIP,
    &COMMENT_ROW_BY_ID,
    &COMMENT_INSERT,
    &COMMENT_SET_DESCRIPTION,
    &COMMENT_UPDATE,
    &DESCRIPTION_INSERT,
    &ISSUE_REACTION_INSERT,
    &ISSUE_REACTION_BY_ID,
    &ISSUE_FOR_REACTION,
    &ISSUE_REACTION_TARGET,
    &COMMENT_REACTION_INSERT,
    &COMMENT_REACTION_BY_ID,
    &COMMENT_FOR_REACTION,
    &COMMENT_REACTION_TARGET,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete() {
        let source = include_str!("write_comment_queries.rs");
        let declared = source.matches(concat!("plane_query!", " {")).count();
        assert_eq!(declared, ALL_QUERIES.len());
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(names.insert(query.name), "duplicate name {:?}", query.name);
        }
    }
}
