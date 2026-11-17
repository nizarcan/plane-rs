// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `assets` group.
//!
//! Every statement reproduces a `QuerySet` from `plane/app/views/asset/` or
//! `plane/app/views/issue/attachment.py`:
//!
//! * the default manager's predicate `deleted_at IS NULL`
//!   (`plane/db/mixins.py:57`) is part of every statement except
//!   [`FILE_ASSET_EXISTS_IN_WORKSPACE`], which says it explicitly because the
//!   Python spells `deleted_at__isnull=True` on `all_objects`
//!   (`v2.py:834`);
//! * `Meta.ordering = ("-created_at",)` (`plane/db/models/asset.py:72`) is the
//!   `ORDER BY` of every multi-row statement, as the ORM emits it;
//! * a `filter(<relation>__field=…)` join does **not** apply the related
//!   model's soft-delete predicate — only the base model's manager does. The
//!   `INNER JOIN workspaces` below therefore filters on `slug` alone, exactly
//!   like the ORM (`packages/…` has no `w.deleted_at IS NULL`).
//!
//! `assets` has no dynamic SQL: every route filters on URL kwargs only, so
//! every statement here is prepared with `prepare_cached` (`DECISIONS.md` D1).

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `FileAsset.objects.filter(asset=asset_key)`
    /// (`plane/app/views/asset/base.py:30`), then filtered in Python by
    /// `is_project_accessible_to` (`plane/db/models/asset.py:106`).
    ///
    /// Bind order: `$1` the asset key (`str(workspace_id) + "/" + asset_key`).
    FILE_ASSETS_BY_ASSET = "assets_file_assets_by_asset",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"asset\" = $1 \
     ORDER BY fa.\"created_at\" DESC",
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

plane_query! {
    /// `FileAsset.objects.filter(asset=asset_key, created_by=request.user)`
    /// (`plane/app/views/asset/base.py:88`).
    ///
    /// Bind order: `$1` asset key, `$2` the caller's user id.
    USER_FILE_ASSETS_BY_ASSET = "assets_user_file_assets_by_asset",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"asset\" = $1 AND fa.\"created_by_id\" = $2 \
     ORDER BY fa.\"created_at\" DESC",
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

plane_query! {
    /// `FileAsset.objects.get(id=asset_id)`
    /// (`plane/app/views/asset/v2.py:532`, the `AllowAny` static route, which
    /// has no workspace in its URL).
    ///
    /// Bind order: `$1` asset id.
    FILE_ASSET_BY_ID = "assets_file_asset_by_id",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = $1 \
     LIMIT 1",
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

plane_query! {
    /// `FileAsset.objects.get(id=asset_id, workspace__slug=slug)`
    /// (`plane/app/views/asset/v2.py:498`).
    ///
    /// Bind order: `$1` asset id, `$2` workspace slug.
    FILE_ASSET_BY_ID_IN_WORKSPACE = "assets_file_asset_by_id_in_workspace",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     INNER JOIN workspaces w ON (fa.\"workspace_id\" = w.\"id\") \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = $1 AND w.\"slug\" = $2 \
     LIMIT 1",
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

plane_query! {
    /// `FileAsset.objects.get(workspace__slug=slug, project_id=project_id, pk=pk)`
    /// (`plane/app/views/asset/v2.py:723`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` asset id.
    FILE_ASSET_IN_PROJECT = "assets_file_asset_in_project",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     INNER JOIN workspaces w ON (fa.\"workspace_id\" = w.\"id\") \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"project_id\" = $2 \
       AND fa.\"id\" = $3 AND w.\"slug\" = $1 \
     LIMIT 1",
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

plane_query! {
    /// `FileAsset.all_objects.filter(id=asset_id, workspace__slug=slug,
    /// deleted_at__isnull=True).first()` (`plane/app/views/asset/v2.py:834`).
    ///
    /// `all_objects` is Django's plain manager, so the statement carries the
    /// explicit `deleted_at IS NULL` the view wrote instead of a manager
    /// predicate.
    ///
    /// Bind order: `$1` asset id, `$2` workspace slug.
    FILE_ASSET_EXISTS_IN_WORKSPACE = "assets_file_asset_exists_in_workspace",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     INNER JOIN workspaces w ON (fa.\"workspace_id\" = w.\"id\") \
     WHERE fa.\"id\" = $1 AND w.\"slug\" = $2 AND fa.\"deleted_at\" IS NULL \
     ORDER BY fa.\"created_at\" DESC \
     LIMIT 1",
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

plane_query! {
    /// `FileAsset.objects.get(id=asset_id, workspace__slug=slug,
    /// is_uploaded=True)` (`plane/app/views/asset/v2.py:965`).
    ///
    /// Bind order: `$1` asset id, `$2` workspace slug.
    FILE_ASSET_UPLOADED_IN_WORKSPACE = "assets_file_asset_uploaded_in_workspace",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     INNER JOIN workspaces w ON (fa.\"workspace_id\" = w.\"id\") \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = $1 AND w.\"slug\" = $2 \
       AND fa.\"is_uploaded\" \
     LIMIT 1",
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

plane_query! {
    /// `FileAsset.objects.get(id=asset_id, workspace__slug=slug,
    /// project_id=project_id, is_uploaded=True)`
    /// (`plane/app/views/asset/v2.py:999`).
    ///
    /// Bind order: `$1` asset id, `$2` workspace slug, `$3` project id.
    FILE_ASSET_UPLOADED_IN_PROJECT = "assets_file_asset_uploaded_in_project",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     INNER JOIN workspaces w ON (fa.\"workspace_id\" = w.\"id\") \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = $1 AND w.\"slug\" = $2 \
       AND fa.\"project_id\" = $3 AND fa.\"is_uploaded\" \
     LIMIT 1",
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

plane_query! {
    /// `FileAsset.objects.filter(issue_id=issue_id,
    /// entity_type=ISSUE_ATTACHMENT, workspace__slug=slug,
    /// project_id=project_id, is_uploaded=True)`
    /// (`plane/app/views/issue/attachment.py:232-238`).
    ///
    /// The extra `w."slug"` column is what `IssueAttachmentSerializer`'s
    /// `asset_url` property reads as `self.workspace.slug`
    /// (`plane/db/models/asset.py:94`); Django pays one query per row for it.
    ///
    /// Bind order: `$1` issue id, `$2` workspace slug, `$3` project id.
    ISSUE_ATTACHMENTS_BY_ISSUE = "assets_issue_attachments_by_issue",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\", w.\"slug\" \
     FROM file_assets fa \
     INNER JOIN workspaces w ON (fa.\"workspace_id\" = w.\"id\") \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"issue_id\" = $1 \
       AND fa.\"entity_type\" = 'ISSUE_ATTACHMENT' AND w.\"slug\" = $2 \
       AND fa.\"project_id\" = $3 AND fa.\"is_uploaded\" \
     ORDER BY fa.\"created_at\" DESC",
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
    ]
}

plane_query! {
    /// `FileAsset.objects.get(id=pk, workspace__slug=slug,
    /// project_id=project_id, issue_id=issue_id)`
    /// (`plane/app/views/issue/attachment.py:214`).
    ///
    /// The extra `w."slug"` column feeds `asset_url`, as in
    /// [`ISSUE_ATTACHMENTS_BY_ISSUE`].
    ///
    /// Bind order: `$1` asset id, `$2` workspace slug, `$3` project id,
    /// `$4` issue id.
    ISSUE_ATTACHMENT_BY_ID = "assets_issue_attachment_by_id",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
     fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", fa.\"attributes\", \
     fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", fa.\"draft_issue_id\", \
     fa.\"project_id\", fa.\"issue_id\", fa.\"comment_id\", fa.\"page_id\", \
     fa.\"entity_type\", fa.\"entity_identifier\", fa.\"is_deleted\", \
     fa.\"is_archived\", fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
     fa.\"is_uploaded\", fa.\"storage_metadata\", w.\"slug\" \
     FROM file_assets fa \
     INNER JOIN workspaces w ON (fa.\"workspace_id\" = w.\"id\") \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = $1 AND w.\"slug\" = $2 \
       AND fa.\"project_id\" = $3 AND fa.\"issue_id\" = $4 \
     LIMIT 1",
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
    ]
}

plane_query! {
    /// The membership half of `FileAsset.is_project_accessible_to`
    /// (`plane/db/models/asset.py:124-133`):
    /// `ProjectMember.objects.filter(member=user, workspace_id=…,
    /// project_id=…, is_active=True).values("role",
    /// "project__guest_view_all_features").first()`.
    ///
    /// One statement for the whole asset list instead of one per asset: the
    /// Python calls `.first()` inside a list comprehension, so it runs N+1
    /// queries; `ORDER BY created_at DESC` keeps the same "newest row wins"
    /// tie-break for a duplicated membership.
    ///
    /// Bind order: `$1` member id, `$2` project ids, `$3` workspace ids.
    PROJECT_MEMBERSHIPS_FOR_USER = "assets_project_memberships_for_user",
    "SELECT pm.\"project_id\", pm.\"role\", p.\"guest_view_all_features\" \
     FROM project_members pm \
     INNER JOIN projects p ON (pm.\"project_id\" = p.\"id\") \
     WHERE pm.\"deleted_at\" IS NULL AND pm.\"member_id\" = $1 \
       AND pm.\"is_active\" AND pm.\"project_id\" = ANY($2) \
       AND pm.\"workspace_id\" = ANY($3) \
     ORDER BY pm.\"created_at\" DESC",
    [
        ("project_id", Uuid),
        ("role", Int2),
        ("guest_view_all_features", Bool),
    ]
}

plane_query! {
    /// `plane/app/permissions/base.py:124-131`: whether the caller is a
    /// *restricted* project guest (`role=GUEST`, `is_active`, project's
    /// `guest_view_all_features=False`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id, `$3` member id.
    PROJECT_IS_RESTRICTED_GUEST = "assets_project_is_restricted_guest",
    "SELECT EXISTS ( \
       SELECT 1 FROM project_members pm \
       INNER JOIN projects p ON (pm.\"project_id\" = p.\"id\") \
       INNER JOIN workspaces w ON (pm.\"workspace_id\" = w.\"id\") \
       WHERE pm.\"deleted_at\" IS NULL AND pm.\"role\" = 5 AND pm.\"is_active\" \
         AND w.\"slug\" = $1 AND pm.\"project_id\" = $2 \
         AND pm.\"member_id\" = $3 AND p.\"guest_view_all_features\" = FALSE \
     )",
    [("exists", Bool)]
}

plane_query! {
    /// `plane/app/permissions/base.py:135-140`: whether the guest created the
    /// issue. `Issue.objects` is the soft-deletion manager, so a deleted issue
    /// does not count.
    ///
    /// Bind order: `$1` issue id, `$2` workspace slug, `$3` project id,
    /// `$4` member id.
    ISSUE_OWNED_BY_USER_IN_WORKSPACE = "assets_issue_owned_by_user_in_workspace",
    "SELECT EXISTS ( \
       SELECT 1 FROM issues i \
       INNER JOIN workspaces w ON (i.\"workspace_id\" = w.\"id\") \
       WHERE i.\"deleted_at\" IS NULL AND i.\"id\" = $1 AND w.\"slug\" = $2 \
         AND i.\"project_id\" = $3 AND i.\"created_by_id\" = $4 \
     )",
    [("exists", Bool)]
}

plane_query! {
    /// `plane/db/models/asset.py:148`: the last branch of
    /// `is_project_accessible_to`
    /// (`Issue.objects.filter(pk=…, project_id=…, created_by=…)`), batched for
    /// a whole asset list — the Python runs it per restricted-guest asset.
    ///
    /// Bind order: `$1` issue ids, `$2` member id.
    ISSUES_OWNED_BY_USER = "assets_issues_owned_by_user",
    "SELECT i.\"id\", i.\"project_id\" \
     FROM issues i \
     WHERE i.\"deleted_at\" IS NULL AND i.\"id\" = ANY($1) \
       AND i.\"created_by_id\" = $2",
    [("id", Uuid), ("project_id", Uuid)]
}

plane_query! {
    /// `plane/db/models/asset.py:139` — `self.comment.issue_id` for an asset
    /// bound to a comment, which the ORM loads with one query per asset.
    ///
    /// Bind order: `$1` comment ids.
    COMMENT_ISSUE_IDS = "assets_comment_issue_ids",
    "SELECT c.\"id\", c.\"issue_id\" \
     FROM issue_comments c \
     WHERE c.\"deleted_at\" IS NULL AND c.\"id\" = ANY($1)",
    [("id", Uuid), ("issue_id", Uuid)]
}

plane_query! {
    /// The project half of `can_modify_asset` (`plane/app/views/asset/v2.py:50-56`):
    /// `ProjectMember.objects.filter(member=user, workspace_id=asset.workspace_id,
    /// project_id=asset.project_id, is_active=True).values_list("role",
    /// flat=True).first()`.
    ///
    /// The `PROJECT_MEMBER_ROLE` variant in `plane-permissions` joins on the
    /// workspace **slug**; this one takes the asset's `workspace_id` because
    /// `can_modify_asset` already has the row and no slug.
    ///
    /// `ProjectMember.Meta.ordering` is `("-created_at",)`
    /// (`plane/db/models/project.py:305`), and `.first()` applies it, so the
    /// statement ends with `ORDER BY pm."created_at" DESC LIMIT 1`.
    ///
    /// Bind order: `$1` member id, `$2` project id, `$3` workspace id.
    PROJECT_MEMBER_ROLE_BY_IDS = "assets_project_member_role_by_ids",
    "SELECT pm.\"role\" \
     FROM project_members pm \
     WHERE pm.\"deleted_at\" IS NULL AND pm.\"is_active\" \
       AND pm.\"member_id\" = $1 AND pm.\"project_id\" = $2 \
       AND pm.\"workspace_id\" = $3 \
     ORDER BY pm.\"created_at\" DESC \
     LIMIT 1",
    [("role", Int2)]
}

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[
    &FILE_ASSETS_BY_ASSET,
    &USER_FILE_ASSETS_BY_ASSET,
    &FILE_ASSET_BY_ID,
    &FILE_ASSET_BY_ID_IN_WORKSPACE,
    &FILE_ASSET_IN_PROJECT,
    &FILE_ASSET_EXISTS_IN_WORKSPACE,
    &FILE_ASSET_UPLOADED_IN_WORKSPACE,
    &FILE_ASSET_UPLOADED_IN_PROJECT,
    &ISSUE_ATTACHMENTS_BY_ISSUE,
    &ISSUE_ATTACHMENT_BY_ID,
    &PROJECT_MEMBERSHIPS_FOR_USER,
    &PROJECT_IS_RESTRICTED_GUEST,
    &ISSUE_OWNED_BY_USER_IN_WORKSPACE,
    &ISSUES_OWNED_BY_USER,
    &COMMENT_ISSUE_IDS,
    &PROJECT_MEMBER_ROLE_BY_IDS,
];
