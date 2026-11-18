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

// ---------------------------------------------------------------------------
// Set B: the public writes
// ---------------------------------------------------------------------------

plane_query! {
    /// `ProjectMember.objects.filter(project_id=…, member=…, is_active=True)
    /// .exists()` — the "add the user for workspace tracking" guard every
    /// public create runs before `ProjectPublicMember.objects.get_or_create`
    /// (`plane/space/views/issue.py:321-329`, `:442-450`, `:539-547`,
    /// `:643-650`; `intake.py` does not run it).
    ///
    /// Bind order: `$1` project id, `$2` member id.
    PUBLIC_ACTIVE_PROJECT_MEMBER = "public_active_project_member",
    "SELECT 1 FROM project_members pm \
     WHERE pm.deleted_at IS NULL AND pm.project_id = $1 AND pm.member_id = $2 \
       AND pm.is_active LIMIT 1",
    [("?column?", Int4)]
}

plane_query! {
    /// `ProjectPublicMember.objects.get_or_create(project_id=…, member=…)`'s
    /// lookup (`plane/db/models/project.py:370-390`): the manager filters
    /// `deleted_at IS NULL`, the unique constraint covers the live rows.
    ///
    /// Bind order: `$1` project id, `$2` member id.
    PUBLIC_PUBLIC_MEMBER_LOOKUP = "public_public_member_lookup",
    "SELECT ppm.id FROM project_public_members ppm \
     WHERE ppm.deleted_at IS NULL AND ppm.project_id = $1 AND ppm.member_id = $2 \
     LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `ProjectPublicMember.objects.get_or_create(...)`'s create branch:
    /// `ProjectBaseModel.save` derives the workspace from the project and
    /// `UserAuditModel.save` stamps the request user (H01).
    ///
    /// Bind order: created_at, updated_at, created_by, id, project, workspace,
    /// member.
    PUBLIC_PUBLIC_MEMBER_INSERT = "public_public_member_insert",
    "INSERT INTO project_public_members \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, member_id) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7)",
    []
}

// ---------------------------------------------------------------------------
// Votes (route 245's POST/DELETE)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueVote.objects.get_or_create(actor_id=…, project_id=…,
    /// issue_id=…)`'s lookup (`plane/space/views/issue.py:637-641`). The
    /// soft-deletion manager supplies `deleted_at IS NULL`; the model's
    /// `ordering = ("-created_at",)` applies to a plain `.get()` too.
    ///
    /// Bind order: `$1` actor id, `$2` project id, `$3` issue id.
    PUBLIC_VOTE_FOR_ACTOR = "public_vote_for_actor",
    "SELECT v.\"id\" FROM issue_votes v \
     WHERE v.\"deleted_at\" IS NULL AND v.\"actor_id\" = $1 \
       AND v.\"project_id\" = $2 AND v.\"issue_id\" = $3 \
     ORDER BY v.\"created_at\" DESC LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `IssueVote.objects.get_or_create(...)`'s create branch
    /// (`plane/space/views/issue.py:637-641`): `ProjectBaseModel.save` derives
    /// the workspace from the project and `vote` starts at the model default.
    ///
    /// Bind order: created_at, updated_at, created_by, id, project, workspace,
    /// actor, issue, vote.
    PUBLIC_VOTE_INSERT = "public_vote_insert",
    "INSERT INTO issue_votes \
     (created_at, updated_at, created_by_id, updated_by_id, deleted_at, id, \
      project_id, workspace_id, actor_id, issue_id, vote) \
     VALUES ($1, $2, $3, NULL, NULL, $4, $5, $6, $7, $8, $9)",
    []
}

plane_query! {
    /// `IssueVote.vote = …; issue_vote.save()` — a full-row save with H01's
    /// `updated_by`/`updated_at`.
    ///
    /// Bind order: every column but the id, then the id.
    PUBLIC_VOTE_UPDATE = "public_vote_update",
    "UPDATE issue_votes SET created_at = $1, updated_at = $2, \
       created_by_id = $3, updated_by_id = $4, deleted_at = $5, id = $6, \
       project_id = $7, workspace_id = $8, actor_id = $9, issue_id = $10, \
       vote = $11 \
     WHERE id = $12",
    []
}

plane_query! {
    /// The vote row plus its actor for `IssueVoteSerializer`'s
    /// `actor_detail` (`plane/app/serializers/issue.py:698-704`), with the
    /// same user projection `sub_queries::ISSUE_REACTIONS` uses.
    ///
    /// Bind order: `$1` vote id.
    PUBLIC_VOTE_BY_ID = "public_vote_by_id",
    "SELECT v.\"created_at\", v.\"updated_at\", v.\"created_by_id\", \
       v.\"updated_by_id\", v.\"deleted_at\", v.\"id\", v.\"project_id\", \
       v.\"workspace_id\", v.\"issue_id\", v.\"actor_id\", v.\"vote\", \
       u.id, u.first_name, u.last_name, u.avatar, u.is_bot, u.display_name, \
       ua.id, ua.entity_type, ua.project_id, ua.issue_id, uaw.slug \
     FROM issue_votes v \
     LEFT JOIN users u ON (u.id = v.actor_id) \
     LEFT JOIN file_assets ua ON (ua.id = u.avatar_asset_id) \
     LEFT JOIN workspaces uaw ON (uaw.id = ua.workspace_id) \
     WHERE v.\"deleted_at\" IS NULL AND v.\"id\" = $1 LIMIT 1",
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
        ("actor_id", Uuid),
        ("vote", Int4),
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
    /// `IssueVote.objects.get(issue_id=…, actor_id=…, project_id=…,
    /// workspace_id=…)` — the destroy lookup
    /// (`plane/space/views/issue.py:667-672`).
    ///
    /// Bind order: `$1` issue id, `$2` actor id, `$3` project id,
    /// `$4` workspace id.
    PUBLIC_VOTE_TARGET = "public_vote_target",
    "SELECT v.\"id\", v.\"vote\" FROM issue_votes v \
     WHERE v.\"deleted_at\" IS NULL AND v.\"issue_id\" = $1 \
       AND v.\"actor_id\" = $2 AND v.\"project_id\" = $3 \
       AND v.\"workspace_id\" = $4 LIMIT 1",
    [("id", Uuid), ("vote", Int4)]
}

// ---------------------------------------------------------------------------
// Intake writes (routes 234-236's POST/PATCH/DELETE)
// ---------------------------------------------------------------------------

plane_query! {
    /// `State.triage_objects.filter(project_id=…, workspace_id=…).first()` —
    /// the create's triage lookup (`plane/space/views/intake.py:138-140`).
    /// `TriageStateManager` filters `group='triage'`, the soft-deletion
    /// manager adds `deleted_at IS NULL` and `Meta.ordering=("sequence",)`
    /// makes `.first()` the lowest sequence.
    ///
    /// Bind order: `$1` project id, `$2` workspace id.
    PUBLIC_TRIAGE_STATE = "public_triage_state",
    "SELECT s.\"id\" FROM states s \
     WHERE s.\"deleted_at\" IS NULL AND s.\"group\" = 'triage' \
       AND s.\"project_id\" = $1 AND s.\"workspace_id\" = $2 \
     ORDER BY s.\"sequence\" ASC LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `IntakeIssue.objects.get(pk=…, workspace_id=…, project_id=…,
    /// intake_id=…)` — the public PATCH/DELETE lookup
    /// (`plane/space/views/intake.py:197-202`, `:280-285`).
    ///
    /// Bind order: `$1` pk, `$2` workspace id, `$3` project id, `$4` intake id.
    PUBLIC_INTAKE_LINK_BY_PK = "public_intake_link_by_pk",
    "SELECT ii.\"issue_id\", ii.\"created_by_id\" FROM intake_issues ii \
     WHERE ii.\"deleted_at\" IS NULL AND ii.\"id\" = $1 \
       AND ii.\"workspace_id\" = $2 AND ii.\"project_id\" = $3 \
       AND ii.\"intake_id\" = $4 LIMIT 1",
    [("issue_id", Uuid), ("created_by_id", Uuid)]
}

plane_query! {
    /// `Issue.objects.get(pk=intake_issue.issue_id, workspace_id=…,
    /// project_id=…)` on the public PATCH path
    /// (`plane/space/views/intake.py:213-217`). `Issue.objects` is the plain
    /// soft-deletion manager (not `issue_objects`), so a triage/archived row
    /// resolves.
    ///
    /// Bind order: `$1` issue id, `$2` workspace id, `$3` project id.
    PUBLIC_ISSUE_ROW_BY_PK = "public_issue_row_by_pk",
    "SELECT issues.\"created_at\", issues.\"updated_at\", \
       issues.\"created_by_id\", issues.\"updated_by_id\", \
       issues.\"deleted_at\", issues.\"id\", issues.\"project_id\", \
       issues.\"workspace_id\", issues.\"parent_id\", issues.\"state_id\", \
       issues.\"point\", issues.\"estimate_point_id\", issues.\"name\", \
       issues.\"description_json\", issues.\"description_html\", \
       issues.\"description_stripped\", issues.\"description_binary\", \
       issues.\"priority\", issues.\"start_date\", issues.\"target_date\", \
       issues.\"sequence_id\", issues.\"sort_order\", issues.\"completed_at\", \
       issues.\"archived_at\", issues.\"is_draft\", \
       issues.\"external_source\", issues.\"external_id\", issues.\"type_id\" \
     FROM issues \
     WHERE issues.\"deleted_at\" IS NULL AND issues.\"id\" = $1 \
       AND issues.\"workspace_id\" = $2 AND issues.\"project_id\" = $3 LIMIT 1",
    [
        ("created_at", Timestamptz),
        ("updated_at", Timestamptz),
        ("created_by_id", Uuid),
        ("updated_by_id", Uuid),
        ("deleted_at", Timestamptz),
        ("id", Uuid),
        ("project_id", Uuid),
        ("workspace_id", Uuid),
        ("parent_id", Uuid),
        ("state_id", Uuid),
        ("point", Int4),
        ("estimate_point_id", Uuid),
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
        ("type_id", Uuid),
    ]
}

// ---------------------------------------------------------------------------
// Assets (routes 255-258)
// ---------------------------------------------------------------------------

plane_query! {
    /// `FileAsset.objects.filter(id=…, workspace=…, project_id=…,
    /// created_by=request.user).first()` — the public PATCH/DELETE/bulk lookup
    /// (`plane/space/views/asset.py:185-190`, `:212-217`, `:280-285`).
    ///
    /// `FileAsset.objects` is the soft-deletion manager, so a soft-deleted
    /// asset is a miss (404 `The requested asset could not be found.`).
    ///
    /// Bind order: `$1` id, `$2` workspace id, `$3` project id,
    /// `$4` created_by id.
    PUBLIC_ASSET_FOR_USER = "public_asset_for_user",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
       fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", \
       fa.\"attributes\", fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", \
       fa.\"draft_issue_id\", fa.\"project_id\", fa.\"issue_id\", \
       fa.\"comment_id\", fa.\"page_id\", fa.\"entity_type\", \
       fa.\"entity_identifier\", fa.\"is_deleted\", fa.\"is_archived\", \
       fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
       fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = $1 \
       AND fa.\"workspace_id\" = $2 AND fa.\"project_id\" = $3 \
       AND fa.\"created_by_id\" = $4 LIMIT 1",
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
        // `entity_identifier` is a `CharField` on `FileAsset`
        // (`plane/db/models/asset.py:38`), unlike the `UUIDField`s around it.
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
    /// The restore lookup, through `FileAsset.all_objects`
    /// (`plane/space/views/asset.py:239-244`): a soft-deleted row resolves.
    ///
    /// Bind order: `$1` id, `$2` workspace id, `$3` project id,
    /// `$4` created_by id.
    PUBLIC_ASSET_FOR_USER_ANY_STATE = "public_asset_for_user_any_state",
    "SELECT fa.\"created_at\", fa.\"updated_at\", fa.\"created_by_id\", \
       fa.\"updated_by_id\", fa.\"deleted_at\", fa.\"id\", \
       fa.\"attributes\", fa.\"asset\", fa.\"user_id\", fa.\"workspace_id\", \
       fa.\"draft_issue_id\", fa.\"project_id\", fa.\"issue_id\", \
       fa.\"comment_id\", fa.\"page_id\", fa.\"entity_type\", \
       fa.\"entity_identifier\", fa.\"is_deleted\", fa.\"is_archived\", \
       fa.\"external_id\", fa.\"external_source\", fa.\"size\", \
       fa.\"is_uploaded\", fa.\"storage_metadata\" \
     FROM file_assets fa \
     WHERE fa.\"id\" = $1 AND fa.\"workspace_id\" = $2 \
       AND fa.\"project_id\" = $3 AND fa.\"created_by_id\" = $4 LIMIT 1",
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
        // `entity_identifier` is a `CharField` on `FileAsset`
        // (`plane/db/models/asset.py:38`), unlike the `UUIDField`s around it.
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
    /// `asset.save(update_fields=["attributes", "is_uploaded"])`
    /// (`plane/space/views/asset.py:202`) — a two-column update: no
    /// `updated_at`, no audit.
    ///
    /// Bind order: `$1` attributes, `$2` is_uploaded, `$3` id.
    PUBLIC_ASSET_UPDATE_UPLOAD = "public_asset_update_upload",
    "UPDATE file_assets SET attributes = $1, is_uploaded = $2 WHERE id = $3",
    []
}

plane_query! {
    /// `asset.is_deleted = True; asset.deleted_at = timezone.now();
    /// asset.save(update_fields=["is_deleted", "deleted_at"])`
    /// (`plane/space/views/asset.py:221-224`).
    ///
    /// Bind order: `$1` deleted_at, `$2` id.
    PUBLIC_ASSET_DELETE = "public_asset_delete",
    "UPDATE file_assets SET is_deleted = true, deleted_at = $1 WHERE id = $2",
    []
}

plane_query! {
    /// `asset.is_deleted = False; asset.deleted_at = None;
    /// asset.save(update_fields=["is_deleted", "deleted_at"])`
    /// (`plane/space/views/asset.py:247-249`).
    ///
    /// Bind order: `$1` id.
    PUBLIC_ASSET_RESTORE = "public_asset_restore",
    "UPDATE file_assets SET is_deleted = false, deleted_at = NULL WHERE id = $1",
    []
}

plane_query! {
    /// `FileAsset.objects.filter(id__in=asset_ids, workspace=…,
    /// project_id=…, created_by=…).first()` — the bulk route's existence check
    /// (`plane/space/views/asset.py:280-294`).
    ///
    /// Bind order: `$1` ids, `$2` workspace id, `$3` project id,
    /// `$4` created_by id.
    PUBLIC_ASSET_PICK_FOR_USER = "public_asset_pick_for_user",
    "SELECT fa.\"id\" FROM file_assets fa \
     WHERE fa.\"deleted_at\" IS NULL AND fa.\"id\" = ANY($1::uuid[]) \
       AND fa.\"workspace_id\" = $2 AND fa.\"project_id\" = $3 \
       AND fa.\"created_by_id\" = $4 \
     ORDER BY fa.\"created_at\" DESC LIMIT 1",
    [("id", Uuid)]
}

plane_query! {
    /// `assets.filter(entity_type=COMMENT_DESCRIPTION,
    /// created_by=request.user).update(comment_id=entity_id)`
    /// (`plane/space/views/asset.py:297-300`) — one UPDATE over the caller's
    /// comment-description assets among the requested ids.
    ///
    /// Bind order: `$1` comment id, `$2` ids, `$3` workspace id,
    /// `$4` project id, `$5` created_by id.
    PUBLIC_ASSET_REBIND = "public_asset_rebind",
    "UPDATE file_assets SET comment_id = $1 \
     WHERE file_assets.\"deleted_at\" IS NULL \
       AND file_assets.\"id\" = ANY($2::uuid[]) \
       AND file_assets.\"workspace_id\" = $3 \
       AND file_assets.\"project_id\" = $4 \
       AND file_assets.\"created_by_id\" = $5 \
       AND file_assets.\"entity_type\" = 'COMMENT_DESCRIPTION'",
    []
}

// ---------------------------------------------------------------------------
// Comments (routes 239/240's POST/PATCH/DELETE)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueComment.objects.get(pk=…, issue_id=…, project_id=…,
    /// workspace_id=…, access="EXTERNAL", actor=request.user)` — the public
    /// PATCH/DELETE lookup (`plane/space/views/issue.py:343-350`, `:377-384`).
    ///
    /// The projection is the app comment view's
    /// (`issues::write_comment_queries::COMMENT_ROW_BY_ID`), which is what
    /// `CommentRow::from_row` decodes: the 20 comment columns, the `IssueFlat`
    /// block, the `ProjectLite` block (+ its cover asset), the `WorkspaceLite`
    /// block (+ its logo asset) and the `UserLite` block (+ its avatar asset).
    ///
    /// Bind order: `$1` pk, `$2` workspace id, `$3` project id, `$4` issue id,
    /// `$5` actor id.
    PUBLIC_COMMENT_ROW_BY_ID = "public_comment_row_by_id",
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
     WHERE c.deleted_at IS NULL AND c.id = $1 AND c.workspace_id = $2 \
       AND c.project_id = $3 AND c.issue_id = $4 AND c.access = 'EXTERNAL' \
       AND c.actor_id = $5 LIMIT 1",
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
// Reactions (routes 241-244's POST/DELETE)
// ---------------------------------------------------------------------------

plane_query! {
    /// `IssueReaction.objects.get(workspace_id=…, project_id=…, issue_id=…,
    /// reaction=…, actor=…)` — the public destroy lookup
    /// (`plane/space/views/issue.py:474-480`). The app route joins
    /// `workspaces` by slug; this URL has no slug, so the board's id is bound
    /// directly.
    ///
    /// Bind order: `$1` workspace id, `$2` project id, `$3` issue id,
    /// `$4` reaction, `$5` actor id.
    PUBLIC_ISSUE_REACTION_TARGET = "public_issue_reaction_target",
    "SELECT r.\"id\", r.\"reaction\" FROM issue_reactions r \
     WHERE r.\"deleted_at\" IS NULL AND r.\"workspace_id\" = $1 \
       AND r.\"project_id\" = $2 AND r.\"issue_id\" = $3 \
       AND r.\"reaction\" = $4 AND r.\"actor_id\" = $5 LIMIT 1",
    [("id", Uuid), ("reaction", Text)]
}

plane_query! {
    /// `CommentReaction.objects.get(project_id=…, workspace_id=…,
    /// comment_id=…, comment__access="EXTERNAL", reaction=…, actor=…)` — the
    /// public destroy lookup (`plane/space/views/issue.py:576-583`). The
    /// `comment__access` filter is a plain INNER JOIN: Django does not apply
    /// `IssueComment`'s soft-deletion manager to a related lookup, so a
    /// soft-deleted comment still satisfies it.
    ///
    /// Bind order: `$1` project id, `$2` workspace id, `$3` comment id,
    /// `$4` reaction, `$5` actor id.
    PUBLIC_COMMENT_REACTION_TARGET = "public_comment_reaction_target",
    "SELECT r.\"id\", r.\"reaction\" FROM comment_reactions r \
     INNER JOIN issue_comments c ON c.\"id\" = r.\"comment_id\" \
     WHERE r.\"deleted_at\" IS NULL AND r.\"project_id\" = $1 \
       AND r.\"workspace_id\" = $2 AND r.\"comment_id\" = $3 \
       AND c.\"access\" = 'EXTERNAL' AND r.\"reaction\" = $4 \
       AND r.\"actor_id\" = $5 LIMIT 1",
    [("id", Uuid), ("reaction", Text)]
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
    &PUBLIC_ACTIVE_PROJECT_MEMBER,
    &PUBLIC_PUBLIC_MEMBER_LOOKUP,
    &PUBLIC_PUBLIC_MEMBER_INSERT,
    &PUBLIC_VOTE_FOR_ACTOR,
    &PUBLIC_VOTE_INSERT,
    &PUBLIC_VOTE_UPDATE,
    &PUBLIC_VOTE_BY_ID,
    &PUBLIC_VOTE_TARGET,
    &PUBLIC_ISSUE_REACTION_TARGET,
    &PUBLIC_COMMENT_REACTION_TARGET,
    &PUBLIC_COMMENT_ROW_BY_ID,
    &PUBLIC_TRIAGE_STATE,
    &PUBLIC_INTAKE_LINK_BY_PK,
    &PUBLIC_ISSUE_ROW_BY_PK,
    &PUBLIC_ASSET_FOR_USER,
    &PUBLIC_ASSET_FOR_USER_ANY_STATE,
    &PUBLIC_ASSET_UPDATE_UPLOAD,
    &PUBLIC_ASSET_DELETE,
    &PUBLIC_ASSET_RESTORE,
    &PUBLIC_ASSET_PICK_FOR_USER,
    &PUBLIC_ASSET_REBIND,
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
