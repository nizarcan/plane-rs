// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `issues` (`deploy/route-groups.yaml`): Work items (issues), drafts, comments, links, attachments and reactions.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/issues/`
//! * `/api/workspaces/*/work-items/**`
//! * `/api/workspaces/*/draft-issues/**`
//! * `/api/workspaces/*/draft-to-issue/**`
//! * `/api/workspaces/*/export-issues/`
//! * `/api/workspaces/*/projects/*/issues/`
//! * `/api/workspaces/*/projects/*/issues/**`
//! * `/api/workspaces/*/projects/*/issues-detail/`
//! * `/api/workspaces/*/projects/*/v2/issues/**`
//! * `/api/workspaces/*/projects/*/work-items/**`
//! * `/api/workspaces/*/projects/*/archived-issues/`
//! * `/api/workspaces/*/projects/*/deleted-issues/`
//! * `/api/workspaces/*/projects/*/issue-dates/`
//! * `/api/workspaces/*/projects/*/bulk-delete-issues/`
//! * `/api/workspaces/*/projects/*/bulk-archive-issues/`
//! * `/api/workspaces/*/projects/*/comments/*/reactions/**`
//! * `/api/workspaces/*/projects/*/user-properties/`
//!
//! Nothing is registered yet: every route of this group still answers Django,
//! so `register` is empty. It stops being empty one route at a time, each one
//! only after its `scenarios/read/` (and, for a write, its write) scenario is
//! clean against Django — `apps/server/HANDLERS.md` is the procedure. Once a
//! group is fully ported, the orchestrator flips its `Owner` column in
//! `ROUTES.md`; a domain agent never edits that file.

use plane_drf::registry::{Registry, RegistryError};

pub mod lists;
pub mod queries;
pub mod row;

pub(crate) mod sub_resources;
/// The issue-core write routes (`plans/WORK_ITEMS.md` set A).
///
/// `pub(crate)` since the intake-issue create/PATCH sets reuse the shared
/// pieces the plan lists (`plans/INTAKE.md` §2d): `validate_issue` with
/// `allow_triage`, `apply_patch`, `ensure_state_for_save`, `field_order`,
/// `fk_value`, `strip_html` and the two task publishers.
pub(crate) mod write_issue;
pub mod write_issue_queries;
pub(crate) mod write_support;

// Set B: comments and reactions.
pub mod write_comment;
pub mod write_comment_queries;
// Set C: links, relations, subscribers and the sub-issue re-parent
// (`plans/WORK_ITEMS.md` §3).
mod write_link;
mod write_relation;
mod write_sub_issue;
pub mod write_sub_queries;
mod write_subscriber;
// Set D: attachments and drafts.
pub mod write_attachment;
pub mod write_draft;
pub mod write_draft_queries;

// Set B of `plans/ANALYTICS_EXPORTS.md`: `POST export-issues/` (233). The
// handler is registered next to the route's `GET` in `list_export`.
pub mod write_export;
pub mod write_export_queries;

/// Register this group's handlers, in the order they were ported.
///
/// `apps/server/HANDLERS.md` has the step list. Return the first
/// [`RegistryError`]: a route that cannot be registered must fail startup
/// rather than fall back to a 501 nobody notices.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    sub_resources::register_sub_resources(registry)?;
    lists::register_lists(registry)?;
    // Work-item sub-resource writes (plans/WORK_ITEMS.md §3, set C):
    // `POST sub-issues/` (73), the issue-link writes (74/75), the
    // issue-subscriber writes (83/84/85) and the relation writes (93/94).
    write_link::register(registry)?;
    write_relation::register(registry)?;
    write_subscriber::register(registry)?;
    write_sub_issue::register(registry)?;
    // `…/projects/{project_id}/user-properties/` matches this group's
    // `projects/*/user-properties/` selector, so the route is registered here
    // even though the handler lives with the other project settings writes.
    registry
        .api_view("plane.app.views.issue.base.ProjectUserDisplayPropertyEndpoint")?
        .method(
            http::Method::PATCH,
            crate::projects::settings_write::user_properties,
        )?;
    // Set B: comments and reactions (routes 81, 82, 86, 87, 88, 89).
    registry
        .viewset("plane.app.views.issue.comment.IssueCommentViewSet")?
        .action("create", write_comment::comment_create)?
        // `PUT …/comments/{pk}/`: the viewset implements only `partial_update`
        // and declares no permission class but `BaseViewSet`'s
        // `IsAuthenticated`, so the `BaseViewSet` guard answers 405 (`routes.json`
        // 82's `guard_actions.update`). A non-member gets the same 405: the
        // permission class passes before the guard.
        .action("update", write_issue::guard_only)?
        .action("partial_update", write_comment::comment_partial_update)?
        .action("destroy", write_comment::comment_destroy)?;
    registry
        .viewset("plane.app.views.issue.reaction.IssueReactionViewSet")?
        .action("create", write_comment::issue_reaction_create)?
        .action("destroy", write_comment::issue_reaction_destroy)?;
    registry
        .viewset("plane.app.views.issue.comment.CommentReactionViewSet")?
        .action("create", write_comment::comment_reaction_create)?
        .action("destroy", write_comment::comment_reaction_destroy)?;

    // Set D: drafts (attachments register below).
    registry
        .viewset("plane.app.views.workspace.draft.WorkspaceDraftIssueViewSet")?
        .action("create", write_draft::create)?
        .action("partial_update", write_draft::partial_update)?
        .action("destroy", write_draft::destroy)?
        .action("create_draft_to_issue", write_draft::create_draft_to_issue)?;
    write_attachment::register(registry)?;

    // `plans/WORK_ITEMS.md` set A — issue core writes (orders 64, 67, 71, 72,
    // 92 and 96), registered in one block so parallel sets only add their own.
    registry
        .viewset("plane.app.views.issue.base.IssueViewSet")?
        .action("create", write_issue::create)?
        .action("update", write_issue::guard_only)?
        .action("partial_update", write_issue::patch)?
        .action("destroy", write_issue::destroy)?;
    registry
        .viewset("plane.app.views.issue.archive.IssueArchiveViewSet")?
        .action("archive", write_issue::archive)?
        .action("unarchive", write_issue::unarchive)?;
    registry
        .api_view("plane.app.views.issue.base.BulkDeleteIssuesEndpoint")?
        .method(http::Method::DELETE, write_issue::bulk_delete)?;
    registry
        .api_view("plane.app.views.issue.archive.BulkArchiveIssuesEndpoint")?
        .method(http::Method::POST, write_issue::bulk_archive)?;
    registry
        .api_view("plane.app.views.issue.base.IssueBulkUpdateDateEndpoint")?
        .method(http::Method::POST, write_issue::issue_dates)?;
    // Modules set B: `POST …/projects/{project_id}/issues/{issue_id}/modules/`
    // (105) belongs to this group (`/projects/*/issues/**` is the most
    // specific selector); the handler is `ModuleIssueViewSet`'s.
    registry
        .viewset("plane.app.views.module.issue.ModuleIssueViewSet")?
        .action(
            "create_issue_modules",
            crate::modules::write_issue_module::create_issue_modules,
        )?;
    Ok(())
}
