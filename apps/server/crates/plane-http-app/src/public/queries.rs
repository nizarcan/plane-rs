// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL of the `public` group's shared board resolution.
//!
//! Every public view starts by resolving the deploy board for its `anchor`
//! (`PUBLIC_INSTANCES.md` §2a), and the seven statements here are the three
//! lookups and the three write-scope predicates plus their two variants, each
//! copied from the ORM SQL `apps/api/plane/space/` emits:
//!
//! | Statement | Python |
//! | --- | --- |
//! | `PUBLISHED_BOARD_BY_ANCHOR` | `.filter(anchor=anchor, entity_name="project").first()` (`space/views/issue.py:113`) |
//! | `BOARD_BY_ANCHOR` | `.filter(anchor=anchor).first()` (`space/views/asset.py:48`, `space/views/project.py:69`) |
//! | `ISSUE_IN_BOARD_SCOPE` | `_issue_in_board_scope` (`space/views/issue.py:76-87`) |
//! | `COMMENT_IN_BOARD_SCOPE` | `_comment_in_board_scope` (`space/views/issue.py:90-97`) |
//! | `OWN_BOARD_COMMENT` | `_is_own_board_comment` (`space/views/asset.py:27-35`) |
//!
//! The two `.get()` lookups (`intake.py:58`, `meta.py:21`, `project.py:23`,
//! `issue.py:690`, `asset.py:48`) are the same two statements: `anchor` is
//! `unique` (`plane/db/models/deploy_board.py:19`), so `.get()` cannot match
//! more than one row and differs from `.first()` only in what a miss raises
//! (a `DoesNotExist` the view's `handle_exception` turns into a 404, see
//! [`super::board::require_published_board`]).
//!
//! Three facts come from the models, not the views:
//!
//! * `DeployBoard.Meta.ordering = ("-created_at",)` (`deploy_board.py:33`), so
//!   `.first()` is `ORDER BY created_at DESC LIMIT 1`;
//! * `DeployBoard.objects` is `SoftDeletionManager` (`plane/db/mixins.py:57`),
//!   so every lookup carries `deleted_at IS NULL` — a soft-deleted board is a
//!   404, not a hit;
//! * `Issue.issue_objects` is `IssueManager` (`plane/db/models/issue.py:92-101`),
//!   whose four exclusions [`ISSUE_IN_BOARD_SCOPE`] reproduces in full: the
//!   public board writes only to what the public board shows.

use plane_db::Query;
use plane_db::plane_query;

plane_query! {
    /// `DeployBoard.objects.filter(anchor=anchor, entity_name="project").first()`
    /// (`plane/space/views/issue.py:113`, `asset.py:207,233,258`).
    ///
    /// Bind order: `$1` anchor.
    PUBLISHED_BOARD_BY_ANCHOR = "public_published_board_by_anchor",
    "SELECT b.\"created_at\", b.\"updated_at\", b.\"created_by_id\", \
     b.\"updated_by_id\", b.\"deleted_at\", b.\"id\", b.\"workspace_id\", \
     b.\"project_id\", b.\"entity_identifier\", b.\"entity_name\", \
     b.\"anchor\", b.\"is_comments_enabled\", b.\"is_reactions_enabled\", \
     b.\"intake_id\", b.\"is_votes_enabled\", b.\"view_props\", \
     b.\"is_activity_enabled\", b.\"is_disabled\" \
     FROM deploy_boards b \
     WHERE b.\"deleted_at\" IS NULL AND b.\"anchor\" = $1 \
       AND b.\"entity_name\" = 'project' \
     ORDER BY b.\"created_at\" DESC LIMIT 1",
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
    ]
}

plane_query! {
    /// `DeployBoard.objects.filter(anchor=anchor).first()` — no `entity_name`
    /// filter (`plane/space/views/asset.py:48`, `project.py:69`). A board for a
    /// page still answers, which is how the asset route reaches a page board's
    /// workspace.
    ///
    /// Bind order: `$1` anchor.
    BOARD_BY_ANCHOR = "public_board_by_anchor",
    "SELECT b.\"created_at\", b.\"updated_at\", b.\"created_by_id\", \
     b.\"updated_by_id\", b.\"deleted_at\", b.\"id\", b.\"workspace_id\", \
     b.\"project_id\", b.\"entity_identifier\", b.\"entity_name\", \
     b.\"anchor\", b.\"is_comments_enabled\", b.\"is_reactions_enabled\", \
     b.\"intake_id\", b.\"is_votes_enabled\", b.\"view_props\", \
     b.\"is_activity_enabled\", b.\"is_disabled\" \
     FROM deploy_boards b \
     WHERE b.\"deleted_at\" IS NULL AND b.\"anchor\" = $1 \
     ORDER BY b.\"created_at\" DESC LIMIT 1",
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
    ]
}

plane_query! {
    /// `_issue_in_board_scope(issue_id, board)`
    /// (`plane/space/views/issue.py:76-87`):
    ///
    /// ```python
    /// Issue.issue_objects.filter(
    ///     id=issue_id,
    ///     project_id=project_deploy_board.project_id,
    ///     workspace_id=project_deploy_board.workspace_id,
    /// ).exists()
    /// ```
    ///
    /// `IssueManager`'s four exclusions are inlined verbatim from
    /// [`plane_db::models::issue::Issue::ISSUE_OBJECTS`] under the alias `i`
    /// (the unit test below proves the text is the generated predicate).
    ///
    /// Bind order: `$1` issue id, `$2` project id, `$3` workspace id. A board
    /// without a project binds `NULL`: `project_id = NULL` is `NULL` (not true)
    /// exactly as Django's `project_id IS NULL` is false for a column that is
    /// `NOT NULL`.
    ISSUE_IN_BOARD_SCOPE = "public_issue_in_board_scope",
    "SELECT EXISTS (\
       SELECT 1 FROM issues i \
       WHERE i.\"id\" = $1 AND i.\"project_id\" = $2 AND i.\"workspace_id\" = $3 \
         AND (i.\"deleted_at\" IS NULL) \
         AND (NOT (EXISTS (SELECT 1 FROM \"states\" WHERE \"states\".\"id\" = i.\"state_id\" \
              AND \"states\".\"group\" = 'triage' AND \"states\".\"group\" IS NOT NULL))) \
         AND (NOT ((i.\"archived_at\" IS NOT NULL))) \
         AND (NOT (EXISTS (SELECT 1 FROM \"projects\" WHERE \"projects\".\"id\" = i.\"project_id\" \
              AND \"projects\".\"archived_at\" IS NOT NULL))) \
         AND (NOT ((i.\"is_draft\"))) \
     ) AS \"exists\"",
    [("exists", Bool)]
}

plane_query! {
    /// `_comment_in_board_scope(comment_id, board)`
    /// (`plane/space/views/issue.py:90-97`): an `EXTERNAL` comment in the
    /// board's project and workspace. `IssueComment.objects` is
    /// `SoftDeletionManager`, hence `deleted_at IS NULL`.
    ///
    /// Bind order: `$1` comment id, `$2` project id, `$3` workspace id.
    COMMENT_IN_BOARD_SCOPE = "public_comment_in_board_scope",
    "SELECT EXISTS (\
       SELECT 1 FROM issue_comments \
       WHERE \"id\" = $1 AND \"project_id\" = $2 AND \"workspace_id\" = $3 \
         AND \"access\" = 'EXTERNAL' AND \"deleted_at\" IS NULL\
     ) AS \"exists\"",
    [("exists", Bool)]
}

plane_query! {
    /// `_is_own_board_comment(comment_id, board, user)`
    /// (`plane/space/views/asset.py:27-35`): the same scope as
    /// [`COMMENT_IN_BOARD_SCOPE] plus `actor`.
    ///
    /// Bind order: `$1` comment id, `$2` project id, `$3` workspace id, `$4`
    /// actor id.
    OWN_BOARD_COMMENT = "public_own_board_comment",
    "SELECT EXISTS (\
       SELECT 1 FROM issue_comments \
       WHERE \"id\" = $1 AND \"project_id\" = $2 AND \"workspace_id\" = $3 \
         AND \"access\" = 'EXTERNAL' AND \"actor_id\" = $4 \
         AND \"deleted_at\" IS NULL\
     ) AS \"exists\"",
    [("exists", Bool)]
}

// ---------------------------------------------------------------------------
// Set A: the read routes
// ---------------------------------------------------------------------------

plane_query! {
    /// `Project.objects.get(id=project_id)` with `ProjectLiteSerializer`'s
    /// seven fields (`plane/space/serializer/project.py:11-25`,
    /// `plane/space/views/meta.py:25-31`). A miss is
    /// `Project.DoesNotExist` → the view's own `{"error": "Project is not
    /// published"}` 404.
    ///
    /// `Project.objects` is `SoftDeletionManager` (`plane/db/mixins.py:57-70`),
    /// hence `deleted_at IS NULL`.
    ///
    /// Bind order: `$1` project id.
    PROJECT_LITE_SPACE = "public_project_lite_space",
    "SELECT p.\"id\", p.\"identifier\", p.\"name\", p.\"cover_image\", \
     p.\"icon_prop\", p.\"emoji\", p.\"description\" \
     FROM projects p WHERE p.\"deleted_at\" IS NULL AND p.\"id\" = $1 LIMIT 1",
    [
        ("id", Uuid),
        ("identifier", Varchar),
        ("name", Varchar),
        ("cover_image", Text),
        ("icon_prop", Jsonb),
        ("emoji", Varchar),
        ("description", Text),
    ]
}

plane_query! {
    /// The app `ProjectLiteSerializer`'s row (`plane/app/serializers/
    /// project.py:113-125`), read as `DeployBoardSerializer.project_details`
    /// (`project.py:14`, `source="project"`).
    ///
    /// `cover_image_url` is `Project.cover_image_url`
    /// (`plane/db/models/project.py:129-139`), which reads
    /// `cover_image_asset.asset_url`: the asset's five columns and its
    /// workspace's slug decide it. The joins are Django's own for that
    /// property (`issues/sub_queries.rs`'s `COMMENTS` block has the identical
    /// list).
    ///
    /// Bind order: `$1` project id.
    PROJECT_LITE_APP = "public_project_lite_app",
    "SELECT pj.\"id\", pj.\"identifier\", pj.\"name\", pj.\"cover_image\", \
     pj.\"logo_props\", pj.\"description\", \
     pca.\"id\", pca.\"entity_type\", pca.\"project_id\", pca.\"issue_id\", pca_w.\"slug\" \
     FROM projects pj \
     LEFT JOIN file_assets pca ON pca.\"id\" = pj.\"cover_image_asset_id\" \
     LEFT JOIN workspaces pca_w ON pca_w.\"id\" = pca.\"workspace_id\" \
     WHERE pj.\"deleted_at\" IS NULL AND pj.\"id\" = $1 LIMIT 1",
    [
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
    ]
}

plane_query! {
    /// The app `WorkspaceLiteSerializer`'s row (`plane/app/serializers/
    /// workspace.py:87-91`), read as `DeployBoardSerializer.workspace_detail`
    /// (`project.py:13`, `source="workspace"`). `logo_url` is
    /// `Workspace.logo_url` (`plane/db/models/workspace.py:146-154`).
    ///
    /// Bind order: `$1` workspace id.
    WORKSPACE_LITE = "public_workspace_lite",
    "SELECT w.\"name\", w.\"slug\", w.\"id\", w.\"logo\", \
     wla.\"id\", wla.\"entity_type\", wla.\"project_id\", wla.\"issue_id\", wla_w.\"slug\" \
     FROM workspaces w \
     LEFT JOIN file_assets wla ON wla.\"id\" = w.\"logo_asset_id\" \
     LEFT JOIN workspaces wla_w ON wla_w.\"id\" = wla.\"workspace_id\" \
     WHERE w.\"deleted_at\" IS NULL AND w.\"id\" = $1 LIMIT 1",
    [
        ("name", Varchar),
        ("slug", Varchar),
        ("id", Uuid),
        ("logo", Text),
        ("id", Uuid),
        ("entity_type", Varchar),
        ("project_id", Uuid),
        ("issue_id", Uuid),
        ("slug", Varchar),
    ]
}

plane_query! {
    /// `DeployBoard.objects.get(workspace__slug=slug, project_id=project_id,
    /// entity_identifier=project_id, entity_name="project")`
    /// (`plane/space/views/project.py:58-60`). A miss is
    /// `DeployBoard.DoesNotExist` → the family 404 body.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id (bound three times).
    BOARD_BY_WORKSPACE_PROJECT = "public_board_by_workspace_project",
    "SELECT b.\"created_at\", b.\"updated_at\", b.\"created_by_id\", \
     b.\"updated_by_id\", b.\"deleted_at\", b.\"id\", b.\"workspace_id\", \
     b.\"project_id\", b.\"entity_identifier\", b.\"entity_name\", \
     b.\"anchor\", b.\"is_comments_enabled\", b.\"is_reactions_enabled\", \
     b.\"intake_id\", b.\"is_votes_enabled\", b.\"view_props\", \
     b.\"is_activity_enabled\", b.\"is_disabled\" \
     FROM deploy_boards b \
     INNER JOIN workspaces w ON w.\"id\" = b.\"workspace_id\" \
     WHERE b.\"deleted_at\" IS NULL AND w.\"slug\" = $1 \
       AND b.\"project_id\" = $2 AND b.\"entity_identifier\" = $2 \
       AND b.\"entity_name\" = 'project' \
     ORDER BY b.\"created_at\" DESC LIMIT 1",
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
    ]
}

plane_query! {
    /// `Cycle.objects.filter(workspace__slug=slug, project_id=…).values("id",
    /// "name")` (`plane/space/views/cycle.py:23-26`). `Cycle.Meta.ordering`
    /// is `("-created_at",)` (`plane/db/models/cycle.py:82-86`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id.
    CYCLE_NAMES = "public_cycle_names",
    "SELECT c.\"id\", c.\"name\" FROM cycles c \
     INNER JOIN workspaces w ON w.\"id\" = c.\"workspace_id\" \
     WHERE c.\"deleted_at\" IS NULL AND w.\"slug\" = $1 AND c.\"project_id\" = $2 \
     ORDER BY c.\"created_at\" DESC",
    [("id", Uuid), ("name", Varchar)]
}

plane_query! {
    /// `Module.objects.filter(workspace__slug=slug, project_id=…).values("id",
    /// "name")` (`plane/space/views/module.py:23-26`). `Module.Meta.ordering`
    /// is `("-created_at",)` (`plane/db/models/module.py:113`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id.
    MODULE_NAMES = "public_module_names",
    "SELECT m.\"id\", m.\"name\" FROM modules m \
     INNER JOIN workspaces w ON w.\"id\" = m.\"workspace_id\" \
     WHERE m.\"deleted_at\" IS NULL AND w.\"slug\" = $1 AND m.\"project_id\" = $2 \
     ORDER BY m.\"created_at\" DESC",
    [("id", Uuid), ("name", Varchar)]
}

plane_query! {
    /// `State.objects.filter(~Q(name="Triage"), workspace__slug=…,
    /// project_id=…).values("name", "group", "color", "id", "sequence")`
    /// (`plane/space/views/state.py:26-30`).
    ///
    /// `State.objects` is `StateManager` (`plane/db/models/state.py:65-69`),
    /// which excludes the triage *group*; the view's own `~Q(name="Triage")`
    /// excludes by name. Both are ported, because they are different
    /// predicates (`PUBLIC_INSTANCES.md` §5).
    ///
    /// `State.Meta.ordering` is `("sequence",)`
    /// (`plane/db/models/state.py:115`), which Django renders as the ordinal
    /// `ORDER BY 5 ASC`; the column form is equivalent.
    ///
    /// Bind order: `$1` workspace slug, `$2` project id.
    STATE_ROWS = "public_state_rows",
    "SELECT s.\"name\", s.\"group\", s.\"color\", s.\"id\", s.\"sequence\" \
     FROM states s \
     INNER JOIN workspaces w ON w.\"id\" = s.\"workspace_id\" \
     WHERE s.\"deleted_at\" IS NULL \
       AND (NOT (s.\"group\" = 'triage' AND s.\"group\" IS NOT NULL)) \
       AND (NOT (s.\"name\" = 'Triage')) \
       AND w.\"slug\" = $1 AND s.\"project_id\" = $2 \
     ORDER BY s.\"sequence\" ASC",
    [
        ("name", Varchar),
        ("group", Varchar),
        ("color", Varchar),
        ("id", Uuid),
        ("sequence", Float8),
    ]
}

plane_query! {
    /// `Label.objects.filter(workspace__slug=…, project_id=…).values("id",
    /// "name", "color", "parent")` (`plane/space/views/label.py:23-26`).
    ///
    /// `Label.Meta.ordering` is `("-created_at",)`
    /// (`plane/db/models/label.py:44`).
    ///
    /// Bind order: `$1` workspace slug, `$2` project id.
    LABEL_ROWS = "public_label_rows",
    "SELECT l.\"id\", l.\"name\", l.\"color\", l.\"parent_id\" AS \"parent\" FROM labels l \
     INNER JOIN workspaces w ON w.\"id\" = l.\"workspace_id\" \
     WHERE l.\"deleted_at\" IS NULL AND w.\"slug\" = $1 AND l.\"project_id\" = $2 \
     ORDER BY l.\"created_at\" DESC",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("color", Varchar),
        ("parent", Uuid),
    ]
}

plane_query! {
    /// `ProjectMember.objects.filter(project=board.project,
    /// workspace=board.workspace, is_active=True).values("id", "member",
    /// "member__display_name", "member__avatar")`
    /// (`plane/space/views/project.py:76-85`).
    ///
    /// `member__display_name`/`member__avatar` join `users` (LEFT OUTER, which
    /// is what `.values()` over a relation emits); there is no join for
    /// `project`/`workspace`, which are compared by id. `ProjectMember.Meta.
    /// ordering` is `("-created_at",)` (`plane/db/models/project.py:306`).
    ///
    /// Bind order: `$1` project id, `$2` workspace id.
    MEMBER_ROWS = "public_member_rows",
    "SELECT pm.\"id\", pm.\"member_id\" AS \"member\", u.\"display_name\", u.\"avatar\" \
     FROM project_members pm \
     LEFT OUTER JOIN users u ON u.\"id\" = pm.\"member_id\" \
     WHERE pm.\"deleted_at\" IS NULL AND pm.\"project_id\" = $1 \
       AND pm.\"workspace_id\" = $2 AND pm.\"is_active\" \
     ORDER BY pm.\"created_at\" DESC",
    [
        ("id", Uuid),
        ("member", Uuid),
        ("display_name", Varchar),
        ("avatar", Text),
    ]
}

plane_query! {
    /// `FileAsset.objects.get(workspace_id=…, project_id=…, pk=…,
    /// entity_type__in=[ISSUE_DESCRIPTION, COMMENT_DESCRIPTION])`
    /// (`plane/space/views/asset.py:57-65`).
    ///
    /// `FileAsset.objects` is `SoftDeletionManager` (`plane/db/mixins.py:57`),
    /// so a soft-deleted asset is a miss. A miss is
    /// `FileAsset.DoesNotExist` → the family 404 body, **not** the view's own
    /// `{"error": …}` (`PUBLIC_INSTANCES.md` §5).
    ///
    /// Bind order: `$1` workspace id, `$2` project id, `$3` asset id.
    FILE_ASSET_FOR_BOARD = "public_file_asset_for_board",
    "SELECT a.\"created_at\", a.\"updated_at\", a.\"created_by_id\", \
     a.\"updated_by_id\", a.\"deleted_at\", a.\"id\", a.\"attributes\", a.\"asset\", \
     a.\"user_id\", a.\"workspace_id\", a.\"draft_issue_id\", a.\"project_id\", \
     a.\"issue_id\", a.\"comment_id\", a.\"page_id\", a.\"entity_type\", \
     a.\"entity_identifier\", a.\"is_deleted\", a.\"is_archived\", a.\"external_id\", \
     a.\"external_source\", a.\"size\", a.\"is_uploaded\", a.\"storage_metadata\" \
     FROM file_assets a \
     WHERE a.\"deleted_at\" IS NULL AND a.\"workspace_id\" = $1 \
       AND a.\"project_id\" = $2 AND a.\"id\" = $3 \
       AND a.\"entity_type\" IN ('ISSUE_DESCRIPTION', 'COMMENT_DESCRIPTION') \
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
    /// `IssueCommentPublicViewSet.get_queryset` (`plane/space/views/issue.py:261-289`):
    /// the board's `EXTERNAL` comments on one issue, with the nested
    /// `issue_detail` / `project_detail` / `workspace_detail` / `actor_detail`
    /// rows and the `is_member` annotation, ordered by `created_at` ASC
    /// (`:286`).
    ///
    /// `filterset_fields = ["issue__id", "workspace__id"]` (`:251`) become the
    /// two nullable bounds `$6`/`$7`.
    ///
    /// Bind order: `$1` workspace id, `$2` project id, `$3` issue id,
    /// `$4` caller id (`NULL` when anonymous), `$5` comment id, `$6`
    /// `issue__id` filter, `$7` `workspace__id` filter.
    PUBLIC_COMMENTS = "public_comments",
    "SELECT c.created_at, c.updated_at, c.created_by_id, c.updated_by_id, c.deleted_at, c.id, c.project_id, c.workspace_id, c.comment_stripped, c.comment_json, c.comment_html, c.description_id, c.attachments, c.issue_id, c.actor_id, c.access, c.external_source, c.external_id, c.edited_at, c.parent_id, i.id, i.name, i.description_json, i.description_html, i.priority, i.start_date, i.target_date, i.sequence_id, i.sort_order, i.is_draft, pj.id, pj.identifier, pj.name, pj.cover_image, pj.logo_props, pj.description, pca.id, pca.entity_type, pca.project_id, pca.issue_id, pcaw.slug, w.name, w.slug, w.id, w.logo, wla.id, wla.entity_type, wla.project_id, wla.issue_id, wlaw.slug, u.id, u.first_name, u.last_name, u.avatar, u.is_bot, u.display_name, ua.id, ua.entity_type, ua.project_id, ua.issue_id, uaw.slug, EXISTS (SELECT 1 FROM project_members m2 WHERE m2.deleted_at IS NULL AND m2.is_active AND m2.member_id = $4 AND m2.project_id = $2 AND m2.workspace_id = $1) AS is_member FROM issue_comments c INNER JOIN issues i ON i.id = c.issue_id INNER JOIN projects pj ON pj.id = c.project_id INNER JOIN workspaces w ON w.id = c.workspace_id LEFT JOIN file_assets pca ON pca.id = pj.cover_image_asset_id LEFT JOIN workspaces pcaw ON pcaw.id = pca.workspace_id LEFT JOIN file_assets wla ON wla.id = w.logo_asset_id LEFT JOIN workspaces wlaw ON wlaw.id = wla.workspace_id LEFT JOIN users u ON u.id = c.actor_id LEFT JOIN file_assets ua ON ua.id = u.avatar_asset_id LEFT JOIN workspaces uaw ON uaw.id = ua.workspace_id WHERE c.deleted_at IS NULL AND c.workspace_id = $1 AND c.project_id = $2 AND c.issue_id = $3 AND c.access = 'EXTERNAL' AND ($5::uuid IS NULL OR c.id = $5) AND ($6::uuid IS NULL OR c.issue_id = $6) AND ($7::uuid IS NULL OR c.workspace_id = $7) ORDER BY c.created_at ASC",
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
    ]
}

plane_query! {
    /// `CommentReactionPublicViewSet.get_queryset`
    /// (`plane/space/views/issue.py:498-517`): the given comment's reactions,
    /// restricted to an `EXTERNAL` comment of the board's project, in
    /// `CommentReaction.Meta.ordering`'s `-created_at` order. The join to
    /// `issue_comments` is Django's `comment__access` filter (a join never
    /// applies the related model's manager, so there is no `deleted_at` term).
    ///
    /// Bind order: `$1` workspace id, `$2` project id, `$3` comment id.
    COMMENT_REACTION_ROWS = "public_comment_reaction_rows",
    "SELECT r.created_at, r.updated_at, r.created_by_id, r.updated_by_id, r.deleted_at, r.id, r.project_id, r.workspace_id, r.actor_id, r.comment_id, r.reaction, ru.display_name \
     FROM comment_reactions r \
     INNER JOIN issue_comments c ON c.id = r.comment_id \
     INNER JOIN users ru ON ru.id = r.actor_id \
     WHERE r.deleted_at IS NULL AND r.workspace_id = $1 AND r.project_id = $2 \
       AND r.comment_id = $3 AND c.access = 'EXTERNAL' \
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
        ("actor_id", Uuid),
        ("comment_id", Uuid),
        ("reaction", Text),
        ("display_name", Varchar),
    ]
}

plane_query! {
    /// `IssueRetrievePublicEndpoint.get`'s `.values()` query
    /// (`plane/space/views/issue.py:692-863`), transcribed from the SQL the
    /// ORM emits (a single aggregate statement: `GROUP BY 1, 21`, i.e. the
    /// issue id and `states.group`).
    ///
    /// The three id arrays are `ARRAY_AGG(DISTINCT …)` over the through-table
    /// joins, so `DISTINCT` sorts them; the two `JSONB_BUILD_OBJECT` aggregates
    /// come back in jsonb's canonical key order and are rendered verbatim.
    ///
    /// The `reaction_items` aggregate reads `votes__actor__avatar_asset` for
    /// `avatar_url` (`issue.py:805,813`) while its actor fields come from
    /// `issue_reactions__actor` — a Django bug that makes every reaction repeat
    /// once per vote with the **voter's** avatar (`PUBLIC_INSTANCES.md` §5,
    /// "Django bugs to port as-is"). The join set is therefore the one the ORM
    /// generated, not a tidied version of it.
    ///
    /// Bind order: `$1` issue id, `$2` project id, `$3` workspace slug.
    ISSUE_VALUES = "public_issue_values",
    "SELECT issues.id, issues.name, issues.state_id, issues.sort_order, \
     issues.description_json, issues.description_html, issues.description_stripped, \
     issues.description_binary, \
     COALESCE(ARRAY_AGG(DISTINCT module_issues.module_id) FILTER (WHERE NOT (module_issues.module_id IS NULL) AND modules.archived_at IS NULL AND module_issues.deleted_at IS NULL), ARRAY[]::uuid[]) AS module_ids, \
     COALESCE(ARRAY_AGG(DISTINCT issue_labels.label_id) FILTER (WHERE NOT (issue_labels.label_id IS NULL) AND issue_labels.deleted_at IS NULL), ARRAY[]::uuid[]) AS label_ids, \
     COALESCE(ARRAY_AGG(DISTINCT issue_assignees.assignee_id) FILTER (WHERE NOT (issue_assignees.assignee_id IS NULL) AND project_members.is_active AND issue_assignees.deleted_at IS NULL), ARRAY[]::uuid[]) AS assignee_ids, \
     issues.estimate_point_id AS \"estimate_point\", issues.priority, issues.start_date, issues.target_date, \
     issues.sequence_id, issues.project_id, issues.parent_id, \
     (SELECT U0.cycle_id FROM cycle_issues U0 \
       WHERE U0.deleted_at IS NULL AND U0.deleted_at IS NULL \
         AND U0.issue_id = issues.id ORDER BY U0.created_at DESC LIMIT 1) AS cycle_id, \
     issues.created_by_id AS \"created_by\", states.\"group\" AS \"state__group\", \
     ARRAY_AGG(DISTINCT CASE WHEN issue_votes.deleted_at IS NULL AND issue_votes.id IS NOT NULL THEN \
       JSONB_BUILD_OBJECT('vote', issue_votes.vote, 'actor_details', \
         JSONB_BUILD_OBJECT('id', issue_votes.actor_id, 'first_name', v_actor.first_name, \
           'last_name', v_actor.last_name, 'avatar', v_actor.avatar, \
           'avatar_url', CASE WHEN v_actor.avatar_asset_id IS NOT NULL \
             THEN ('/api/assets/v2/static/' || (v_actor.avatar_asset_id)::varchar || '/') \
             WHEN v_actor.avatar_asset_id IS NULL THEN v_actor.avatar ELSE NULL END, \
           'display_name', v_actor.display_name)) ELSE NULL END) \
       FILTER (WHERE CASE WHEN issue_votes.deleted_at IS NULL AND issue_votes.id IS NOT NULL THEN TRUE ELSE FALSE END) AS vote_items, \
     ARRAY_AGG(DISTINCT CASE WHEN issue_reactions.deleted_at IS NULL AND issue_reactions.id IS NOT NULL THEN \
       JSONB_BUILD_OBJECT('reaction', issue_reactions.reaction, 'actor_details', \
         JSONB_BUILD_OBJECT('id', issue_reactions.actor_id, 'first_name', r_actor.first_name, \
           'last_name', r_actor.last_name, 'avatar', r_actor.avatar, \
           'avatar_url', CASE WHEN v_actor.avatar_asset_id IS NOT NULL \
             THEN ('/api/assets/v2/static/' || (v_actor.avatar_asset_id)::varchar || '/') \
             WHEN v_actor.avatar_asset_id IS NULL THEN v_actor.avatar ELSE NULL END, \
           'display_name', r_actor.display_name)) ELSE NULL END) \
       FILTER (WHERE CASE WHEN issue_reactions.deleted_at IS NULL AND issue_reactions.id IS NOT NULL THEN TRUE ELSE FALSE END) AS reaction_items \
     FROM issues \
     LEFT OUTER JOIN states ON issues.state_id = states.id \
     INNER JOIN projects ON issues.project_id = projects.id \
     INNER JOIN workspaces ON issues.workspace_id = workspaces.id \
     LEFT OUTER JOIN issue_labels ON issues.id = issue_labels.issue_id \
     LEFT OUTER JOIN issue_assignees ON issues.id = issue_assignees.issue_id \
     LEFT OUTER JOIN users ON issue_assignees.assignee_id = users.id \
     LEFT OUTER JOIN project_members ON users.id = project_members.member_id \
     LEFT OUTER JOIN module_issues ON issues.id = module_issues.issue_id \
     LEFT OUTER JOIN modules ON module_issues.module_id = modules.id \
     LEFT OUTER JOIN issue_votes ON issues.id = issue_votes.issue_id \
     LEFT OUTER JOIN users v_actor ON issue_votes.actor_id = v_actor.id \
     LEFT OUTER JOIN issue_reactions ON issues.id = issue_reactions.issue_id \
     LEFT OUTER JOIN users r_actor ON issue_reactions.actor_id = r_actor.id \
     WHERE issues.deleted_at IS NULL \
       AND NOT (states.group = 'triage' AND states.group IS NOT NULL) \
       AND NOT (issues.archived_at IS NOT NULL) \
       AND NOT (projects.archived_at IS NOT NULL) \
       AND NOT (issues.is_draft) \
       AND issues.id = $1 AND issues.project_id = $2 AND workspaces.slug = $3 \
     GROUP BY issues.id, states.group \
     ORDER BY issues.id ASC LIMIT 1",
    [
        ("id", Uuid),
        ("name", Varchar),
        ("state_id", Uuid),
        ("sort_order", Float8),
        ("description_json", Jsonb),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_binary", Bytea),
        ("module_ids", ArrayOf(Uuid)),
        ("label_ids", ArrayOf(Uuid)),
        ("assignee_ids", ArrayOf(Uuid)),
        ("estimate_point", Uuid),
        ("priority", Varchar),
        ("start_date", Date),
        ("target_date", Date),
        ("sequence_id", Int4),
        ("project_id", Uuid),
        ("parent_id", Uuid),
        ("cycle_id", Uuid),
        ("created_by", Uuid),
        ("state__group", Varchar),
        ("vote_items", ArrayOf(Jsonb)),
        ("reaction_items", ArrayOf(Jsonb)),
    ]
}

plane_query! {
    /// `DeployBoard.workspace.slug` — the FK read every view that has a board
    /// row makes with `deploy_board.workspace.slug`
    /// (`plane/space/views/cycle.py:24`, `issue.py:118,695`).
    ///
    /// `Workspace.objects` is `SoftDeletionManager`, so a soft-deleted
    /// workspace is `Workspace.DoesNotExist` → the family 404.
    ///
    /// Bind order: `$1` workspace id.
    WORKSPACE_SLUG = "public_workspace_slug",
    "SELECT w.\"slug\" FROM workspaces w \
     WHERE w.\"deleted_at\" IS NULL AND w.\"id\" = $1 LIMIT 1",
    [("slug", Varchar)]
}

/// The `IntakeIssuePublicViewSet.list` projection, in
/// `IssueStateIntakeSerializer`'s member order: the issue id first, then the 27
/// model columns, `bridge_id`, `sub_issues_count`, the `StateLiteSerializer`
/// block and the app `ProjectLiteSerializer` block.
///
/// The statement is assembled by the handler because `issue_filters` adds
/// joins and predicates dynamically (`plane/space/views/intake.py:65-72`).
pub const INTAKE_LIST_PROJECTION: &str = "issues.\"id\", issues.\"created_at\", issues.\"updated_at\", issues.\"deleted_at\", issues.\"point\", issues.\"name\", issues.\"description_json\", issues.\"description_html\", issues.\"description_stripped\", issues.\"description_binary\", issues.\"priority\", issues.\"start_date\", issues.\"target_date\", issues.\"sequence_id\", issues.\"sort_order\", issues.\"completed_at\", issues.\"archived_at\", issues.\"is_draft\", issues.\"external_source\", issues.\"external_id\", issues.\"created_by_id\", issues.\"updated_by_id\", issues.\"project_id\", issues.\"workspace_id\", issues.\"parent_id\", issues.\"state_id\", issues.\"estimate_point_id\", issues.\"type_id\", ii.\"id\" AS bridge_id, \
     (SELECT Count(U0.\"id\") FROM \"issues\" U0 \
       LEFT OUTER JOIN \"states\" U1 ON (U0.\"state_id\" = U1.\"id\") \
       INNER JOIN \"projects\" U2 ON (U0.\"project_id\" = U2.\"id\") \
       WHERE (U0.\"deleted_at\" IS NULL AND NOT (U1.\"group\" = 'triage' AND U1.\"group\" IS NOT NULL) \
         AND NOT (U0.\"archived_at\" IS NOT NULL) AND NOT (U2.\"archived_at\" IS NOT NULL) \
         AND NOT (U0.\"is_draft\") AND U0.\"parent_id\" = (\"issues\".\"id\"))) AS sub_issues_count, \
     states.\"id\", states.\"name\", states.\"color\", states.\"group\", \
     projects.\"id\", projects.\"identifier\", projects.\"name\", projects.\"cover_image\", projects.\"logo_props\", \
     projects.\"description\", pca.\"id\", pca.\"entity_type\", pca.\"project_id\", \
     pca.\"issue_id\", pca_w.\"slug\" \
    ";

plane_query! {
    /// `IntakeIssue.objects.get(pk, workspace_id, project_id, intake_id)`
    /// (`plane/space/views/intake.py:258-263`) for `retrieve`. A miss is the
    /// family 404.
    ///
    /// Bind order: `$1` pk, `$2` workspace id, `$3` project id, `$4` intake id.
    PUBLIC_INTAKE_ISSUE = "public_intake_issue",
    "SELECT ii.\"issue_id\" FROM intake_issues ii \
     WHERE ii.\"deleted_at\" IS NULL AND ii.\"intake_id\" = $4 AND ii.\"id\" = $1 \
       AND ii.\"project_id\" = $3 AND ii.\"workspace_id\" = $2 LIMIT 1",
    [("issue_id", Uuid)]
}

plane_query! {
    /// `Issue.objects.get(pk=intake_issue.issue_id, workspace_id=…,
    /// project_id=…)` (`plane/space/views/intake.py:264-268`) plus the nested
    /// `state`/`project` blocks the serializer's lazily-read foreign keys
    /// need. There is no `sub_issues_count` annotation on this path, which is
    /// why the field is **absent** from the response
    /// (`IssueStateIntakeSerializer` is a plain `BaseSerializer`, and DRF
    /// skips a missing attribute).
    ///
    /// Bind order: `$1` issue id, `$2` workspace id, `$3` project id.
    PUBLIC_INTAKE_ISSUE_VALUES = "public_intake_issue_values",
    "SELECT issues.\"id\", issues.\"created_at\", issues.\"updated_at\", issues.\"deleted_at\", issues.\"point\", issues.\"name\", issues.\"description_json\", issues.\"description_html\", issues.\"description_stripped\", issues.\"description_binary\", issues.\"priority\", issues.\"start_date\", issues.\"target_date\", issues.\"sequence_id\", issues.\"sort_order\", issues.\"completed_at\", issues.\"archived_at\", issues.\"is_draft\", issues.\"external_source\", issues.\"external_id\", issues.\"created_by_id\", issues.\"updated_by_id\", issues.\"project_id\", issues.\"workspace_id\", issues.\"parent_id\", issues.\"state_id\", issues.\"estimate_point_id\", issues.\"type_id\", NULL::uuid AS bridge_id, NULL::int8 AS sub_issues_count, \
     states.\"id\", states.\"name\", states.\"color\", states.\"group\", \
     pj.\"id\", pj.\"identifier\", pj.\"name\", pj.\"cover_image\", pj.\"logo_props\", \
     pj.\"description\", pca.\"id\", pca.\"entity_type\", pca.\"project_id\", \
     pca.\"issue_id\", pca_w.\"slug\" \
     FROM issues \
     LEFT OUTER JOIN states ON issues.\"state_id\" = states.\"id\" \
     LEFT JOIN projects pj ON issues.\"project_id\" = pj.\"id\" \
     LEFT JOIN file_assets pca ON pca.\"id\" = pj.\"cover_image_asset_id\" \
     LEFT JOIN workspaces pca_w ON pca_w.\"id\" = pca.\"workspace_id\" \
     WHERE issues.\"deleted_at\" IS NULL AND issues.\"id\" = $1 \
       AND issues.\"workspace_id\" = $2 AND issues.\"project_id\" = $3 LIMIT 1",
    [
        ("id", Uuid),
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("deleted_at", Timestamptz),
        ("point", Int4),
        ("name", Varchar),
        ("description_json", Jsonb),
        ("description_html", Text),
        ("description_stripped", Text),
        ("description_binary", Bytea),
        ("priority", Varchar),
        ("start_date", Date),
        ("target_date", Date),
        ("sequence_id", Int4),
        ("sort_order", Float8),
        ("completed_at", Timestamptz),
        ("archived_at", Date),
        ("is_draft", Bool),
        ("external_source", Varchar),
        ("external_id", Varchar),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("parent_id", Uuid),
        ("state_id", Uuid),
        ("estimate_point_id", Uuid),
        ("type_id", Uuid),
        ("bridge_id", Uuid),
        ("sub_issues_count", Int8),
        ("id", Uuid),
        ("name", Varchar),
        ("color", Varchar),
        ("group", Varchar),
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
    ]
}

plane_query! {
    /// `issue.labels.all()` for the serializer's `label_details`
    /// (`Label.Meta.ordering = ("-created_at",)`, `plane/db/models/label.py:44`).
    /// The through table's `deleted_at` is **not** filtered: Django joins
    /// `issue_labels` without the related manager. `labels.deleted_at IS NULL`
    /// comes from `Label.objects` and *is* applied.
    ///
    /// Bind order: `$1` issue ids.
    PUBLIC_INTAKE_LABELS = "public_intake_labels",
    "SELECT il.\"issue_id\", l.\"id\", l.\"name\", l.\"color\" \
     FROM labels l INNER JOIN issue_labels il ON l.\"id\" = il.\"label_id\" \
     WHERE l.\"deleted_at\" IS NULL AND il.\"issue_id\" = ANY($1::uuid[]) \
     ORDER BY l.\"created_at\" DESC",
    [
        ("issue_id", Uuid),
        ("id", Uuid),
        ("name", Varchar),
        ("color", Varchar),
    ]
}

plane_query! {
    /// `issue.assignees.all()` for `assignee_details`
    /// (`User.Meta.ordering = ("-created_at",)`, which is what
    /// `plane/db/models/user.py` declares). The through table is joined
    /// without its manager, as above; `users` has no soft delete.
    ///
    /// Bind order: `$1` issue ids.
    PUBLIC_INTAKE_ASSIGNEES = "public_intake_assignees",
    "SELECT ia.\"issue_id\", u.\"id\", u.\"first_name\", u.\"last_name\", u.\"avatar\", \
     u.\"is_bot\", u.\"display_name\", ua.\"id\", ua.\"entity_type\", ua.\"project_id\", \
     ua.\"issue_id\", ua_w.\"slug\" \
     FROM users u INNER JOIN issue_assignees ia ON u.\"id\" = ia.\"assignee_id\" \
     LEFT JOIN file_assets ua ON ua.\"id\" = u.\"avatar_asset_id\" \
     LEFT JOIN workspaces ua_w ON ua_w.\"id\" = ua.\"workspace_id\" \
     WHERE ia.\"issue_id\" = ANY($1::uuid[]) ORDER BY u.\"created_at\" DESC",
    [
        ("issue_id", Uuid),
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
    /// `issue.issue_intake.all()` for `issue_intake`
    /// (`IntakeIssue.Meta.ordering = ("-created_at",)`,
    /// `plane/db/models/intake.py:86`). The prefetch queryset is
    /// `IntakeIssue.objects.only("status", "duplicate_to", "snoozed_till",
    /// "source")` (`intake.py:98-103`), i.e. the four rendered fields plus the
    /// primary key.
    ///
    /// Bind order: `$1` issue ids.
    PUBLIC_INTAKE_LINKS = "public_intake_links",
    "SELECT ii.\"issue_id\", ii.\"id\", ii.\"status\", ii.\"duplicate_to_id\", \
     ii.\"snoozed_till\", ii.\"source\" \
     FROM intake_issues ii \
     WHERE ii.\"deleted_at\" IS NULL AND ii.\"issue_id\" = ANY($1::uuid[]) \
     ORDER BY ii.\"created_at\" DESC",
    [
        ("issue_id", Uuid),
        ("id", Uuid),
        ("status", Int4),
        ("duplicate_to_id", Uuid),
        ("snoozed_till", Timestamptz),
        ("source", Varchar),
    ]
}

/// Every statement in this catalog, for `tests/schema_check.rs`.
pub const ALL_QUERIES: &[&Query] = &[
    &PUBLISHED_BOARD_BY_ANCHOR,
    &BOARD_BY_ANCHOR,
    &ISSUE_IN_BOARD_SCOPE,
    &COMMENT_IN_BOARD_SCOPE,
    &OWN_BOARD_COMMENT,
    &PROJECT_LITE_SPACE,
    &PROJECT_LITE_APP,
    &WORKSPACE_LITE,
    &BOARD_BY_WORKSPACE_PROJECT,
    &CYCLE_NAMES,
    &MODULE_NAMES,
    &STATE_ROWS,
    &LABEL_ROWS,
    &MEMBER_ROWS,
    &FILE_ASSET_FOR_BOARD,
    &PUBLIC_COMMENTS,
    &COMMENT_REACTION_ROWS,
    &ISSUE_VALUES,
    &WORKSPACE_SLUG,
    &PUBLIC_INTAKE_ISSUE,
    &PUBLIC_INTAKE_ISSUE_VALUES,
    &PUBLIC_INTAKE_LABELS,
    &PUBLIC_INTAKE_ASSIGNEES,
    &PUBLIC_INTAKE_LINKS,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_names_are_unique() {
        let mut names = std::collections::BTreeSet::new();
        for query in ALL_QUERIES {
            assert!(
                names.insert(query.name),
                "duplicate query name {:?}",
                query.name
            );
        }
    }

    /// Both board lookups select `DeployBoard::COLUMNS` under the alias `b`, so
    /// `DeployBoard::from_row(row, 0)` decodes them.
    #[test]
    fn the_board_lookups_select_the_model_columns() {
        let expected: Vec<String> = plane_db::models::deploy_board::DeployBoard::COLUMNS
            .split(", ")
            .map(|column| {
                let column = column.trim();
                let start = column.find('"').expect("quoted column") + 1;
                let end = column.rfind('"').expect("quoted column");
                format!("b.\"{}\"", &column[start..end])
            })
            .collect();
        let expected = format!("SELECT {} FROM", expected.join(", "));
        for query in [&PUBLISHED_BOARD_BY_ANCHOR, &BOARD_BY_ANCHOR] {
            assert!(
                query.sql.starts_with(&expected),
                "{} does not select DeployBoard::COLUMNS under `b`:\n{}",
                query.name,
                query.sql
            );
        }
    }

    /// `IssueManager`'s filters are the generated model's predicate, not a
    /// hand-written approximation: a change to `Issue::ISSUE_OBJECTS` must fail
    /// here instead of silently widening what a public board may write to.
    #[test]
    fn the_issue_scope_is_the_issue_manager() {
        let predicate =
            plane_db::models::with_alias(plane_db::models::issue::Issue::ISSUE_OBJECTS, "i");
        assert!(
            ISSUE_IN_BOARD_SCOPE.sql.contains(&predicate),
            "ISSUE_IN_BOARD_SCOPE does not inline Issue::ISSUE_OBJECTS:\n{}\n\n{}",
            ISSUE_IN_BOARD_SCOPE.sql,
            predicate
        );
    }
}
