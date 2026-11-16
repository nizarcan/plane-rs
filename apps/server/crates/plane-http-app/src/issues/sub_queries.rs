// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the issue sub-resource handlers.
//!
//! Every statement the `sub_*` handlers send has a fixed shape, so they are
//! declared here with [`plane_query!`](plane_db::plane_query) and prepared
//! through the per-connection statement cache (`DECISIONS.md` D1). They are
//! separate from `issues::queries` because the two halves of the route group
//! were ported in parallel; `tests/schema_check.rs` checks both catalogs.
//!
//! The column lists are derived from the ORM SQL the Django view emits; each
//! statement names the Python file/line it reproduces.

use plane_db::Query;
use plane_db::plane_query;

// ---------------------------------------------------------------------------
// Permission helpers the handlers own
// ---------------------------------------------------------------------------
//
// `issue_hidden_from_guest`'s two statements moved to `plane-permissions`
// (`crates/plane-permissions/src/queries.rs`), which owns and schema-checks
// them; the guard itself is called from `sub_common`.

plane_query! {
    /// `issue_in_project(..., include_archived=True)`
    /// (`plane/app/views/issue/relation.py:34-56`): `Issue.objects.filter(pk,
    /// workspace__slug, project_id).exists()`.
    ISSUE_IN_PROJECT = "issues_sub_issue_in_project",
    "SELECT EXISTS (SELECT 1 FROM issues i \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND i.id = $1 AND i.project_id = $2 AND w.slug = $3) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `WorkItemDescriptionVersionEndpoint.get`'s role check
    /// (`plane/app/views/issue/version.py:104-111`): an active GUEST
    /// `ProjectMember` row on the URL project (the `guest_view_all_features`
    /// half is read off the `Project` row the view already loaded).
    GUEST_MEMBERSHIP = "issues_sub_guest_membership",
    "SELECT EXISTS (SELECT 1 FROM project_members pm \
     INNER JOIN workspaces w ON w.id = pm.workspace_id \
     WHERE pm.deleted_at IS NULL AND pm.is_active AND pm.member_id = $1 AND pm.role = 5 \
       AND pm.project_id = $2 AND w.slug = $3) AS exists",
    [("exists", Bool)]
}

// ---------------------------------------------------------------------------
// Issue links (routes 74/75)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueLinkViewSet.get_queryset` (`plane/app/views/issue/link.py:39-54`):
    /// `order_by("-created_at").distinct()`. The membership join Django emits
    /// is an `EXISTS` here, which yields the same rows without the `DISTINCT`
    /// sort (a soft-deleted duplicate `project_members` row is the only way the
    /// join could duplicate a link).
    ///
    /// `$5` is the detail pk; `NULL` means "list".
    LINKS = "issues_sub_links",
    "SELECT l.created_at, l.updated_at, l.created_by_id, l.updated_by_id, l.deleted_at, l.id, l.project_id, l.workspace_id, l.title, l.url, l.issue_id, l.metadata, u.id, u.first_name, u.last_name, u.avatar, u.is_bot, u.display_name, ua.id, ua.entity_type, ua.project_id, ua.issue_id, uaw.slug FROM issue_links l INNER JOIN workspaces w ON w.id = l.workspace_id INNER JOIN projects pj ON pj.id = l.project_id LEFT JOIN users u ON u.id = l.created_by_id LEFT JOIN file_assets ua ON ua.id = u.avatar_asset_id LEFT JOIN workspaces uaw ON uaw.id = ua.workspace_id WHERE l.deleted_at IS NULL AND w.slug = $1 AND l.project_id = $2 AND l.issue_id = $3 AND pj.archived_at IS NULL AND EXISTS (SELECT 1 FROM project_members pm WHERE pm.project_id = pj.id AND pm.is_active AND pm.member_id = $4) AND ($5::uuid IS NULL OR l.id = $5) ORDER BY l.created_at DESC",
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
    ],
}

// ---------------------------------------------------------------------------
// Issue attachments (route 76)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueAttachmentEndpoint.get`'s collection branch
    /// (`plane/app/views/issue/attachment.py:104-112`): every `ISSUE_ATTACHMENT`
    /// `FileAsset` of the issue, no `is_uploaded` filter (that one is the V2
    /// endpoint), default `-created_at` ordering.
    ATTACHMENTS = "issues_sub_attachments",
    "SELECT f.created_at, f.updated_at, f.created_by_id, f.updated_by_id, f.deleted_at, f.id, \
     f.attributes, f.asset, f.user_id, f.workspace_id, f.draft_issue_id, f.project_id, \
     f.issue_id, f.comment_id, f.page_id, f.entity_type, f.entity_identifier, f.is_deleted, \
     f.is_archived, f.external_id, f.external_source, f.size, f.is_uploaded, f.storage_metadata, \
     w.slug \
     FROM file_assets f \
     INNER JOIN workspaces w ON w.id = f.workspace_id \
     WHERE f.deleted_at IS NULL AND f.entity_type = 'ISSUE_ATTACHMENT' AND f.issue_id = $1 \
       AND f.project_id = $2 AND w.slug = $3 \
     ORDER BY f.created_at DESC",
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
        ("slug", Varchar),
    ],
}

// ---------------------------------------------------------------------------
// History (route 80)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueActivityEndpoint.get`'s activity queryset
    /// (`plane/app/views/issue/activity.py:44-56`), with the
    /// `~Q(field__in=[...])` written the way Django compiles it (a NULL `field`
    /// row is kept). `$5` is the optional `created_at__gt` filter.
    ACTIVITIES = "issues_sub_activities",
    "SELECT a.created_at, a.updated_at, a.created_by_id, a.updated_by_id, a.deleted_at, a.id, a.project_id, a.workspace_id, a.issue_id, a.verb, a.field, a.old_value, a.new_value, a.comment, a.attachments, a.issue_comment_id, a.actor_id, a.old_identifier, a.new_identifier, a.epoch, i.id, i.name, i.description_json, i.description_html, i.priority, i.start_date, i.target_date, i.sequence_id, i.sort_order, i.is_draft, pj.id, pj.identifier, pj.name, pj.cover_image, pj.logo_props, pj.description, pca.id, pca.entity_type, pca.project_id, pca.issue_id, pcaw.slug, w.name, w.slug, w.id, w.logo, wla.id, wla.entity_type, wla.project_id, wla.issue_id, wlaw.slug, u.id, u.first_name, u.last_name, u.avatar, u.is_bot, u.display_name, ua.id, ua.entity_type, ua.project_id, ua.issue_id, uaw.slug FROM issue_activities a INNER JOIN issues i ON i.id = a.issue_id INNER JOIN projects pj ON pj.id = a.project_id INNER JOIN project_members pm ON pm.project_id = pj.id INNER JOIN workspaces w ON w.id = a.workspace_id LEFT JOIN file_assets pca ON pca.id = pj.cover_image_asset_id LEFT JOIN workspaces pcaw ON pcaw.id = pca.workspace_id LEFT JOIN file_assets wla ON wla.id = w.logo_asset_id LEFT JOIN workspaces wlaw ON wlaw.id = wla.workspace_id LEFT JOIN users u ON u.id = a.actor_id LEFT JOIN file_assets ua ON ua.id = u.avatar_asset_id LEFT JOIN workspaces uaw ON uaw.id = ua.workspace_id WHERE a.deleted_at IS NULL AND a.issue_id = $1 AND (a.field IS NULL OR a.field NOT IN ('comment', 'vote', 'reaction', 'draft')) AND pj.archived_at IS NULL AND pm.is_active AND pm.member_id = $2 AND a.project_id = $3 AND w.slug = $4 AND ($5::timestamptz IS NULL OR a.created_at > $5::timestamptz) ORDER BY a.created_at ASC",
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
        ("verb", Varchar),
        ("field", Varchar),
        ("old_value", Text),
        ("new_value", Text),
        ("comment", Text),
        ("attachments", ArrayOf(Varchar)),
        ("issue_comment_id", Uuid),
        ("actor_id", Uuid),
        ("old_identifier", Uuid),
        ("new_identifier", Uuid),
        ("epoch", Float8),
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
    ],
}

plane_query! {
    /// `IssueActivityEndpoint.get`'s comment queryset
    /// (`plane/app/views/issue/activity.py:58-73`). No `is_member` annotation
    /// here, so `IssueCommentSerializer` skips that field (DRF `SkipField`).
    HISTORY_COMMENTS = "issues_sub_history_comments",
    "SELECT c.created_at, c.updated_at, c.created_by_id, c.updated_by_id, c.deleted_at, c.id, c.project_id, c.workspace_id, c.comment_stripped, c.comment_json, c.comment_html, c.description_id, c.attachments, c.issue_id, c.actor_id, c.access, c.external_source, c.external_id, c.edited_at, c.parent_id, i.id, i.name, i.description_json, i.description_html, i.priority, i.start_date, i.target_date, i.sequence_id, i.sort_order, i.is_draft, pj.id, pj.identifier, pj.name, pj.cover_image, pj.logo_props, pj.description, pca.id, pca.entity_type, pca.project_id, pca.issue_id, pcaw.slug, w.name, w.slug, w.id, w.logo, wla.id, wla.entity_type, wla.project_id, wla.issue_id, wlaw.slug, u.id, u.first_name, u.last_name, u.avatar, u.is_bot, u.display_name, ua.id, ua.entity_type, ua.project_id, ua.issue_id, uaw.slug FROM issue_comments c INNER JOIN issues i ON i.id = c.issue_id INNER JOIN projects pj ON pj.id = c.project_id INNER JOIN project_members pm ON pm.project_id = pj.id INNER JOIN workspaces w ON w.id = c.workspace_id LEFT JOIN file_assets pca ON pca.id = pj.cover_image_asset_id LEFT JOIN workspaces pcaw ON pcaw.id = pca.workspace_id LEFT JOIN file_assets wla ON wla.id = w.logo_asset_id LEFT JOIN workspaces wlaw ON wlaw.id = wla.workspace_id LEFT JOIN users u ON u.id = c.actor_id LEFT JOIN file_assets ua ON ua.id = u.avatar_asset_id LEFT JOIN workspaces uaw ON uaw.id = ua.workspace_id WHERE c.deleted_at IS NULL AND c.issue_id = $1 AND pj.archived_at IS NULL AND pm.is_active AND pm.member_id = $2 AND c.project_id = $3 AND w.slug = $4 AND ($5::timestamptz IS NULL OR c.created_at > $5::timestamptz) ORDER BY c.created_at ASC",
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
    ],
}

plane_query! {
    /// `IntakeIssue.objects.only("source_email", "source", "extra")` prefetched
    /// through `issue__issue_intake` with `to_attr="source_data"`
    /// (`plane/app/views/issue/activity.py:75-83`); default `-created_at`.
    INTAKE_SOURCES = "issues_sub_intake_sources",
    "SELECT it.source, it.source_email, it.extra \
     FROM intake_issues it \
     WHERE it.deleted_at IS NULL AND it.issue_id = $1 \
     ORDER BY it.created_at DESC",
    [
        ("source", Varchar),
        ("source_email", Text),
        ("extra", Jsonb),
    ],
}

// ---------------------------------------------------------------------------
// Comments (routes 81/82) and comment reactions (route 88)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueCommentViewSet.get_queryset` (`plane/app/views/issue/comment.py:46-71`),
    /// including the `is_member` `Exists` annotation. `$5` is the detail pk.
    COMMENTS = "issues_sub_comments",
    "SELECT c.created_at, c.updated_at, c.created_by_id, c.updated_by_id, c.deleted_at, c.id, c.project_id, c.workspace_id, c.comment_stripped, c.comment_json, c.comment_html, c.description_id, c.attachments, c.issue_id, c.actor_id, c.access, c.external_source, c.external_id, c.edited_at, c.parent_id, i.id, i.name, i.description_json, i.description_html, i.priority, i.start_date, i.target_date, i.sequence_id, i.sort_order, i.is_draft, pj.id, pj.identifier, pj.name, pj.cover_image, pj.logo_props, pj.description, pca.id, pca.entity_type, pca.project_id, pca.issue_id, pcaw.slug, w.name, w.slug, w.id, w.logo, wla.id, wla.entity_type, wla.project_id, wla.issue_id, wlaw.slug, u.id, u.first_name, u.last_name, u.avatar, u.is_bot, u.display_name, ua.id, ua.entity_type, ua.project_id, ua.issue_id, uaw.slug, EXISTS (SELECT 1 FROM project_members m2 INNER JOIN workspaces w2 ON w2.id = m2.workspace_id WHERE m2.deleted_at IS NULL AND m2.is_active AND m2.member_id = $4 AND m2.project_id = $2 AND w2.slug = $1) AS is_member FROM issue_comments c INNER JOIN issues i ON i.id = c.issue_id INNER JOIN projects pj ON pj.id = c.project_id INNER JOIN project_members pm ON pm.project_id = pj.id INNER JOIN workspaces w ON w.id = c.workspace_id LEFT JOIN file_assets pca ON pca.id = pj.cover_image_asset_id LEFT JOIN workspaces pcaw ON pcaw.id = pca.workspace_id LEFT JOIN file_assets wla ON wla.id = w.logo_asset_id LEFT JOIN workspaces wlaw ON wlaw.id = wla.workspace_id LEFT JOIN users u ON u.id = c.actor_id LEFT JOIN file_assets ua ON ua.id = u.avatar_asset_id LEFT JOIN workspaces uaw ON uaw.id = ua.workspace_id WHERE c.deleted_at IS NULL AND w.slug = $1 AND c.project_id = $2 AND i.project_id = $2 AND c.issue_id = $3 AND pj.archived_at IS NULL AND pm.is_active AND pm.member_id = $4 AND ($5::uuid IS NULL OR c.id = $5) AND ($6::uuid IS NULL OR c.issue_id = $6) AND ($7::uuid IS NULL OR c.workspace_id = $7) ORDER BY c.created_at DESC",
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
        ("is_member", Bool),
    ],
}

plane_query! {
    /// The `comment_reactions` prefetch in `IssueActivityEndpoint.get`
    /// (`plane/app/views/issue/activity.py:64-72`) and the per-comment related
    /// manager `IssueCommentSerializer` reads in the comments list: every
    /// reaction of the given comments with its actor's `display_name`, in the
    /// model's `-created_at` order.
    COMMENT_REACTIONS = "issues_sub_comment_reactions",
    "SELECT r.created_at, r.updated_at, r.created_by_id, r.updated_by_id, r.deleted_at, r.id, r.project_id, r.workspace_id, r.actor_id, r.comment_id, r.reaction, ru.display_name FROM comment_reactions r INNER JOIN users ru ON ru.id = r.actor_id WHERE r.deleted_at IS NULL AND r.comment_id = ANY($1::uuid[]) ORDER BY r.created_at DESC",
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
    ],
}

plane_query! {
    /// `CommentReactionViewSet._comment_issue_hidden_from_guest`'s parent issue
    /// lookup (`plane/app/views/issue/comment.py:250-257`).
    COMMENT_ISSUE = "issues_sub_comment_issue",
    "SELECT c.issue_id FROM issue_comments c \
     INNER JOIN workspaces w ON w.id = c.workspace_id \
     WHERE c.deleted_at IS NULL AND c.id = $1 AND c.project_id = $2 AND w.slug = $3 \
     LIMIT 1",
    [("issue_id", Uuid)]
}

plane_query! {
    /// `CommentReactionViewSet.get_queryset` (`plane/app/views/issue/comment.py:227-243`).
    COMMENT_REACTIONS_BY_COMMENT = "issues_sub_comment_reactions_by_comment",
    "SELECT r.created_at, r.updated_at, r.created_by_id, r.updated_by_id, r.deleted_at, r.id, r.project_id, r.workspace_id, r.actor_id, r.comment_id, r.reaction, ru.display_name FROM comment_reactions r INNER JOIN workspaces w ON w.id = r.workspace_id INNER JOIN projects pj ON pj.id = r.project_id INNER JOIN users ru ON ru.id = r.actor_id WHERE r.deleted_at IS NULL AND w.slug = $1 AND r.project_id = $2 AND r.comment_id = $3 AND pj.archived_at IS NULL AND EXISTS (SELECT 1 FROM project_members pm WHERE pm.project_id = pj.id AND pm.is_active AND pm.member_id = $4) ORDER BY r.created_at DESC",
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
    ],
}

// ---------------------------------------------------------------------------
// Subscribers (routes 83/85)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueSubscriberViewSet.subscription_status`
    /// (`plane/app/views/issue/subscriber.py:120-127`).
    SUBSCRIPTION_STATUS = "issues_sub_subscription_status",
    "SELECT EXISTS (SELECT 1 FROM issue_subscribers s \
     INNER JOIN workspaces w ON w.id = s.workspace_id \
     WHERE s.deleted_at IS NULL AND s.issue_id = $1 AND s.subscriber_id = $2 \
       AND s.project_id = $3 AND w.slug = $4) AS exists",
    [("exists", Bool)]
}

plane_query! {
    /// `IssueSubscriberViewSet.list` (`plane/app/views/issue/subscriber.py:74-82`):
    /// active project members, `-created_at`.
    SUBSCRIBERS = "issues_sub_subscribers",
    "SELECT m.created_at, m.updated_at, m.created_by_id, m.updated_by_id, m.deleted_at, m.id, m.project_id, m.workspace_id, m.member_id, m.comment, m.role, m.view_props, m.default_props, m.preferences, m.sort_order, m.is_active, u.id, u.first_name, u.last_name, u.avatar, u.is_bot, u.display_name, ua.id, ua.entity_type, ua.project_id, ua.issue_id, uaw.slug FROM project_members m INNER JOIN workspaces w ON w.id = m.workspace_id LEFT JOIN users u ON u.id = m.member_id LEFT JOIN file_assets ua ON ua.id = u.avatar_asset_id LEFT JOIN workspaces uaw ON uaw.id = ua.workspace_id WHERE m.deleted_at IS NULL AND m.is_active AND m.project_id = $1 AND w.slug = $2 ORDER BY m.created_at DESC",
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
    ],
}

// ---------------------------------------------------------------------------
// Issue reactions (route 86)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueReactionViewSet.get_queryset` (`plane/app/views/issue/reaction.py:21-35`).
    ISSUE_REACTIONS = "issues_sub_issue_reactions",
    "SELECT r.created_at, r.updated_at, r.created_by_id, r.updated_by_id, r.deleted_at, r.id, r.project_id, r.workspace_id, r.actor_id, r.issue_id, r.reaction, u.id, u.first_name, u.last_name, u.avatar, u.is_bot, u.display_name, ua.id, ua.entity_type, ua.project_id, ua.issue_id, uaw.slug FROM issue_reactions r INNER JOIN workspaces w ON w.id = r.workspace_id INNER JOIN projects pj ON pj.id = r.project_id LEFT JOIN users u ON u.id = r.actor_id LEFT JOIN file_assets ua ON ua.id = u.avatar_asset_id LEFT JOIN workspaces uaw ON uaw.id = ua.workspace_id WHERE r.deleted_at IS NULL AND w.slug = $1 AND r.project_id = $2 AND r.issue_id = $3 AND pj.archived_at IS NULL AND EXISTS (SELECT 1 FROM project_members pm WHERE pm.project_id = pj.id AND pm.is_active AND pm.member_id = $4) ORDER BY r.created_at DESC",
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
    ],
}

// ---------------------------------------------------------------------------
// Issue relations (route 93)
// ---------------------------------------------------------------------------

plane_query! {
    /// The `issue_relations` queryset `IssueRelationViewSet.list` derives its
    /// eight id sets from (`plane/app/views/issue/relation.py:100-107`).
    RELATIONS = "issues_sub_relations",
    "SELECT r.issue_id, r.related_issue_id, r.relation_type \
     FROM issue_relations r \
     INNER JOIN workspaces w ON w.id = r.workspace_id \
     WHERE r.deleted_at IS NULL AND (r.issue_id = $1 OR r.related_issue_id = $1) AND w.slug = $2 \
     ORDER BY r.created_at DESC",
    [
        ("issue_id", Uuid),
        ("related_issue_id", Uuid),
        ("relation_type", Varchar),
    ],
}

plane_query! {
    /// The `.values(*fields)` projection `IssueRelationViewSet.list` builds for
    /// each relation group (`plane/app/views/issue/relation.py:193-215`), over
    /// `Issue.issue_objects` (`plane/db/models/issue.py:92-101`), with no
    /// `ORDER BY` — Django emits `GROUP BY 1` and Postgres decides the order.
    /// `$2` is the union of the ids the eight groups reference.
    RELATED_ISSUES = "issues_sub_related_issues",
    "SELECT i.id, i.name, i.state_id, i.sort_order, i.priority, i.sequence_id, i.project_id, \
     COALESCE(ARRAY_AGG(DISTINCT l.label_id) \
              FILTER (WHERE l.label_id IS NOT NULL AND l.deleted_at IS NULL), '{}'::uuid[]) \
       AS label_ids, \
     COALESCE(ARRAY_AGG(DISTINCT ag.assignee_id) \
              FILTER (WHERE ag.assignee_id IS NOT NULL AND pmg.is_active AND ag.deleted_at IS NULL), \
              '{}'::uuid[]) AS assignee_ids, \
     i.created_at, i.updated_at, i.created_by_id AS created_by, \
     i.updated_by_id AS updated_by \
     FROM issues i \
     LEFT JOIN states s ON s.id = i.state_id \
     INNER JOIN projects pj ON pj.id = i.project_id \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     LEFT JOIN issue_labels l ON l.issue_id = i.id \
     LEFT JOIN issue_assignees ag ON ag.issue_id = i.id \
     LEFT JOIN users ug ON ug.id = ag.assignee_id \
     LEFT JOIN project_members pmg ON pmg.member_id = ug.id \
     WHERE i.deleted_at IS NULL AND (s.\"group\" IS NULL OR s.\"group\" <> 'triage') \
       AND i.archived_at IS NULL AND pj.archived_at IS NULL AND i.is_draft = false \
       AND w.slug = $1 AND i.id = ANY($2::uuid[]) \
     GROUP BY i.id",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("state_id", Uuid),
        ("sort_order", Float8),
        ("priority", Varchar),
        ("sequence_id", Int4),
        ("project_id", Uuid),
        ("label_ids", ArrayOf(Uuid)),
        ("assignee_ids", ArrayOf(Uuid)),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by", Uuid),
        ("updated_by", Uuid),
    ],
}

// ---------------------------------------------------------------------------
// Issue versions (routes 97/98) and description versions (routes 99/100)
// ---------------------------------------------------------------------------

plane_query! {
    /// `paginate(base_queryset=…).count()` for `IssueVersionEndpoint.get`
    /// (`plane/app/views/issue/version.py:76-96`).
    VERSIONS_COUNT = "issues_sub_versions_count",
    "SELECT COUNT(*) AS \"__count\" FROM issue_versions v \
     INNER JOIN workspaces w ON w.id = v.workspace_id \
     WHERE v.deleted_at IS NULL AND v.issue_id = $1 AND v.project_id = $2 AND w.slug = $3",
    [("__count", Int8)]
}

plane_query! {
    /// `paginate`'s page for `IssueVersionEndpoint.get`: `.values(*required_fields)`
    /// over the default `-created_at` ordering.
    VERSIONS_PAGE = "issues_sub_versions_page",
    "SELECT v.id AS id, v.workspace_id AS workspace, v.project_id AS project, \
     v.issue_id AS issue, v.last_saved_at AS last_saved_at, v.owned_by_id AS owned_by, \
     v.created_at AS created_at, v.updated_at AS updated_at, \
     v.created_by_id AS created_by, v.updated_by_id AS updated_by \
     FROM issue_versions v \
     INNER JOIN workspaces w ON w.id = v.workspace_id \
     WHERE v.deleted_at IS NULL AND v.issue_id = $1 AND v.project_id = $2 AND w.slug = $3 \
     ORDER BY v.created_at DESC LIMIT $4 OFFSET $5",
    [
        ("id", Uuid),
        ("workspace", Uuid),
        ("project", Uuid),
        ("issue", Uuid),
        ("last_saved_at", Timestamptz),
        ("owned_by", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by", Uuid),
        ("updated_by", Uuid),
    ],
}

plane_query! {
    /// `IssueVersion.objects.get(workspace__slug, project_id, issue_id, pk)`
    /// (`plane/app/views/issue/version.py:59-63`), projected in
    /// `IssueVersionDetailSerializer` field order.
    VERSION_BY_ID = "issues_sub_version_by_id",
    "SELECT v.id, v.workspace_id, v.project_id, v.issue_id, v.parent, v.state, v.estimate_point, \
     v.name, v.priority, v.start_date, v.target_date, v.assignees, v.sequence_id, v.labels, \
     v.sort_order, v.completed_at, v.archived_at, v.is_draft, v.external_source, v.external_id, \
     v.type, v.cycle, v.modules, v.meta, v.last_saved_at, v.owned_by_id, v.created_at, \
     v.updated_at, v.created_by_id, v.updated_by_id \
     FROM issue_versions v \
     INNER JOIN workspaces w ON w.id = v.workspace_id \
     WHERE v.deleted_at IS NULL AND v.id = $1 AND v.issue_id = $2 AND v.project_id = $3 \
       AND w.slug = $4",
    [
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("parent", Uuid),
        ("state", Uuid),
        ("estimate_point", Uuid),
        ("name", Varchar),
        ("priority", Varchar),
        ("start_date", Date),
        ("target_date", Date),
        ("assignees", ArrayOf(Uuid)),
        ("sequence_id", Int4),
        ("labels", ArrayOf(Uuid)),
        ("sort_order", Float8),
        ("completed_at", Timestamptz),
        ("archived_at", Date),
        ("is_draft", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("type", Uuid),
        ("cycle", Uuid),
        ("modules", ArrayOf(Uuid)),
        ("meta", Jsonb),
        ("last_saved_at", Timestamptz),
        ("owned_by_id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
    ],
}

plane_query! {
    /// `WorkItemDescriptionVersionEndpoint.get`'s count
    /// (`plane/app/views/issue/version.py:137-150`).
    DESC_VERSIONS_COUNT = "issues_sub_desc_versions_count",
    "SELECT COUNT(*) AS \"__count\" FROM issue_description_versions v \
     INNER JOIN workspaces w ON w.id = v.workspace_id \
     WHERE v.deleted_at IS NULL AND v.issue_id = $1 AND v.project_id = $2 AND w.slug = $3",
    [("__count", Int8)]
}

plane_query! {
    /// `WorkItemDescriptionVersionEndpoint.get`'s page, explicitly
    /// `order_by("-created_at")` (`plane/app/views/issue/version.py:141-143`).
    DESC_VERSIONS_PAGE = "issues_sub_desc_versions_page",
    "SELECT v.id AS id, v.workspace_id AS workspace, v.project_id AS project, \
     v.issue_id AS issue, v.last_saved_at AS last_saved_at, v.owned_by_id AS owned_by, \
     v.created_at AS created_at, v.updated_at AS updated_at, \
     v.created_by_id AS created_by, v.updated_by_id AS updated_by \
     FROM issue_description_versions v \
     INNER JOIN workspaces w ON w.id = v.workspace_id \
     WHERE v.deleted_at IS NULL AND v.issue_id = $1 AND v.project_id = $2 AND w.slug = $3 \
     ORDER BY v.created_at DESC LIMIT $4 OFFSET $5",
    [
        ("id", Uuid),
        ("workspace", Uuid),
        ("project", Uuid),
        ("issue", Uuid),
        ("last_saved_at", Timestamptz),
        ("owned_by", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by", Uuid),
        ("updated_by", Uuid),
    ],
}

plane_query! {
    /// `IssueDescriptionVersion.objects.get(workspace__slug, project_id,
    /// issue_id, pk)` (`plane/app/views/issue/version.py:113-120`), projected in
    /// `IssueDescriptionVersionDetailSerializer` field order.
    DESC_VERSION_BY_ID = "issues_sub_desc_version_by_id",
    "SELECT v.id, v.workspace_id, v.project_id, v.issue_id, v.description_binary, \
     v.description_html, v.description_stripped, v.description_json, v.last_saved_at, \
     v.owned_by_id, v.created_at, v.updated_at, v.created_by_id, v.updated_by_id \
     FROM issue_description_versions v \
     INNER JOIN workspaces w ON w.id = v.workspace_id \
     WHERE v.deleted_at IS NULL AND v.id = $1 AND v.issue_id = $2 AND v.project_id = $3 \
       AND w.slug = $4",
    [
        ("id", Uuid),
        ("workspace_id", Uuid),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("description_binary", Bytea),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_json", Jsonb),
        ("last_saved_at", Timestamptz),
        ("owned_by_id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
    ],
}

plane_query! {
    /// `Project.objects.get(pk=project_id)` in
    /// `WorkItemDescriptionVersionEndpoint.get` (`plane/app/views/issue/version.py:100`).
    PROJECT_GUEST_VIEW = "issues_sub_project_guest_view",
    "SELECT pj.guest_view_all_features FROM projects pj \
     WHERE pj.id = $1 AND pj.deleted_at IS NULL",
    [("guest_view_all_features", Bool)]
}

plane_query! {
    /// `Issue.objects.get(workspace__slug, project_id, pk)` in
    /// `WorkItemDescriptionVersionEndpoint.get` (`plane/app/views/issue/version.py:101`).
    ISSUE_CREATED_BY = "issues_sub_issue_created_by",
    "SELECT i.created_by_id FROM issues i \
     INNER JOIN workspaces w ON w.id = i.workspace_id \
     WHERE i.deleted_at IS NULL AND w.slug = $1 AND i.project_id = $2 AND i.id = $3",
    [("created_by_id", Uuid)]
}

/// Every catalogued statement of the `sub_` handlers, in declaration order.
///
/// Listed in `crates/plane-http-app/tests/schema_check.rs` with the crate's
/// other catalogs; the handlers read the individual statics.
#[allow(dead_code)]
pub static ALL_QUERIES: &[&Query] = &[
    &ISSUE_IN_PROJECT,
    &GUEST_MEMBERSHIP,
    &LINKS,
    &ATTACHMENTS,
    &ACTIVITIES,
    &HISTORY_COMMENTS,
    &INTAKE_SOURCES,
    &COMMENTS,
    &COMMENT_REACTIONS,
    &COMMENT_ISSUE,
    &COMMENT_REACTIONS_BY_COMMENT,
    &SUBSCRIPTION_STATUS,
    &SUBSCRIBERS,
    &ISSUE_REACTIONS,
    &RELATIONS,
    &RELATED_ISSUES,
    &VERSIONS_COUNT,
    &VERSIONS_PAGE,
    &VERSION_BY_ID,
    &DESC_VERSIONS_COUNT,
    &DESC_VERSIONS_PAGE,
    &DESC_VERSION_BY_ID,
    &PROJECT_GUEST_VIEW,
    &ISSUE_CREATED_BY,
];
