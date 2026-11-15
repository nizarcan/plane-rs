// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Every `BasePermission.has_permission` Plane runs.
//!
//! Sources, all read line by line:
//!
//! * `plane/app/permissions/workspace.py:19-137`
//! * `plane/app/permissions/project.py:13-205`
//! * `plane/app/permissions/page.py:18-143`
//! * `plane/utils/permissions/workspace.py` and `project.py` — identical to the
//!   app files for the classes they contain (see the crate docs);
//! * `plane/license/api/permissions/instance.py:12-17`.
//!
//! Two behaviours surprise every reader and are kept:
//!
//! * the classes take the workspace from `view.workspace_slug`
//!   (`kwargs.get("slug")`) and the project from `view.project_id`, which on a
//!   url named `project` is `kwargs["pk"]`, not `kwargs["project_id"]`;
//! * `WorkspaceOwnerPermission` does not look at `Workspace.owner`; it tests an
//!   active workspace membership with role ADMIN.

use crate::instance::instance_admin_permission;
use crate::membership::{
    is_admin, is_admin_or_member, project_member_exists_by_identifier, project_member_role,
    workspace_member_role, workspace_member_role_by_id,
};
use crate::page::project_page_permission;
use crate::{Error, PermCtx, is_safe_method};

/// One permission class, addressed by the dotted path `routes.json` uses.
///
/// The app and utils copies are separate variants because they are separate
/// dotted paths in the route table; their implementations are shared, which the
/// differential matrix verifies against both real Python classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PermissionClass {
    // `rest_framework.permissions`
    AllowAny,
    IsAuthenticated,
    // `plane.app.permissions.workspace`
    AppWorkSpaceBasePermission,
    AppWorkspaceOwnerPermission,
    AppWorkSpaceAdminPermission,
    AppWorkspaceEntityPermission,
    AppWorkspaceViewerPermission,
    AppWorkspaceUserPermission,
    AppWorkspaceMemberPermission,
    // `plane.app.permissions.project`
    AppProjectBasePermission,
    AppProjectArchiveUnarchivePermission,
    AppProjectMemberPermission,
    AppProjectEntityPermission,
    AppProjectAdminPermission,
    AppProjectLitePermission,
    // `plane.app.permissions.page`
    AppProjectPagePermission,
    // `plane.utils.permissions.workspace` (no `WorkspaceMemberPermission`)
    UtilsWorkSpaceBasePermission,
    UtilsWorkspaceOwnerPermission,
    UtilsWorkSpaceAdminPermission,
    UtilsWorkspaceEntityPermission,
    UtilsWorkspaceViewerPermission,
    UtilsWorkspaceUserPermission,
    // `plane.utils.permissions.project` (no `ProjectArchiveUnarchivePermission`)
    UtilsProjectBasePermission,
    UtilsProjectMemberPermission,
    UtilsProjectEntityPermission,
    UtilsProjectAdminPermission,
    UtilsProjectLitePermission,
    // `plane.utils.permissions.page`
    UtilsProjectPagePermission,
    // `plane.license.api.permissions.instance`
    InstanceAdminPermission,
}

impl PermissionClass {
    /// Resolve a `permission_classes` entry from `routes.json`.
    ///
    /// Only the defining module path is accepted (what Python's
    /// `security_classes` records); a package re-export such as
    /// `plane.app.permissions.WorkSpaceAdminPermission` is *not* a class's
    /// `__module__`, so it never appears in `routes.json` and is rejected here
    /// on purpose — a typo in the policy generator must fail loudly.
    pub fn from_dotted(path: &str) -> Option<Self> {
        match path {
            "rest_framework.permissions.AllowAny" => Some(Self::AllowAny),
            "rest_framework.permissions.IsAuthenticated" => Some(Self::IsAuthenticated),

            "plane.app.permissions.workspace.WorkSpaceBasePermission" => {
                Some(Self::AppWorkSpaceBasePermission)
            }
            "plane.app.permissions.workspace.WorkspaceOwnerPermission" => {
                Some(Self::AppWorkspaceOwnerPermission)
            }
            "plane.app.permissions.workspace.WorkSpaceAdminPermission" => {
                Some(Self::AppWorkSpaceAdminPermission)
            }
            "plane.app.permissions.workspace.WorkspaceEntityPermission" => {
                Some(Self::AppWorkspaceEntityPermission)
            }
            "plane.app.permissions.workspace.WorkspaceViewerPermission" => {
                Some(Self::AppWorkspaceViewerPermission)
            }
            "plane.app.permissions.workspace.WorkspaceUserPermission" => {
                Some(Self::AppWorkspaceUserPermission)
            }
            "plane.app.permissions.workspace.WorkspaceMemberPermission" => {
                Some(Self::AppWorkspaceMemberPermission)
            }

            "plane.app.permissions.project.ProjectBasePermission" => {
                Some(Self::AppProjectBasePermission)
            }
            "plane.app.permissions.project.ProjectArchiveUnarchivePermission" => {
                Some(Self::AppProjectArchiveUnarchivePermission)
            }
            "plane.app.permissions.project.ProjectMemberPermission" => {
                Some(Self::AppProjectMemberPermission)
            }
            "plane.app.permissions.project.ProjectEntityPermission" => {
                Some(Self::AppProjectEntityPermission)
            }
            "plane.app.permissions.project.ProjectAdminPermission" => {
                Some(Self::AppProjectAdminPermission)
            }
            "plane.app.permissions.project.ProjectLitePermission" => {
                Some(Self::AppProjectLitePermission)
            }

            "plane.app.permissions.page.ProjectPagePermission" => {
                Some(Self::AppProjectPagePermission)
            }

            "plane.utils.permissions.workspace.WorkSpaceBasePermission" => {
                Some(Self::UtilsWorkSpaceBasePermission)
            }
            "plane.utils.permissions.workspace.WorkspaceOwnerPermission" => {
                Some(Self::UtilsWorkspaceOwnerPermission)
            }
            "plane.utils.permissions.workspace.WorkSpaceAdminPermission" => {
                Some(Self::UtilsWorkSpaceAdminPermission)
            }
            "plane.utils.permissions.workspace.WorkspaceEntityPermission" => {
                Some(Self::UtilsWorkspaceEntityPermission)
            }
            "plane.utils.permissions.workspace.WorkspaceViewerPermission" => {
                Some(Self::UtilsWorkspaceViewerPermission)
            }
            "plane.utils.permissions.workspace.WorkspaceUserPermission" => {
                Some(Self::UtilsWorkspaceUserPermission)
            }

            "plane.utils.permissions.project.ProjectBasePermission" => {
                Some(Self::UtilsProjectBasePermission)
            }
            "plane.utils.permissions.project.ProjectMemberPermission" => {
                Some(Self::UtilsProjectMemberPermission)
            }
            "plane.utils.permissions.project.ProjectEntityPermission" => {
                Some(Self::UtilsProjectEntityPermission)
            }
            "plane.utils.permissions.project.ProjectAdminPermission" => {
                Some(Self::UtilsProjectAdminPermission)
            }
            "plane.utils.permissions.project.ProjectLitePermission" => {
                Some(Self::UtilsProjectLitePermission)
            }

            "plane.utils.permissions.page.ProjectPagePermission" => {
                Some(Self::UtilsProjectPagePermission)
            }

            "plane.license.api.permissions.instance.InstanceAdminPermission" => {
                Some(Self::InstanceAdminPermission)
            }

            _ => None,
        }
    }

    /// The dotted path `routes.json` records for this class.
    pub const fn dotted(self) -> &'static str {
        match self {
            Self::AllowAny => "rest_framework.permissions.AllowAny",
            Self::IsAuthenticated => "rest_framework.permissions.IsAuthenticated",
            Self::AppWorkSpaceBasePermission => {
                "plane.app.permissions.workspace.WorkSpaceBasePermission"
            }
            Self::AppWorkspaceOwnerPermission => {
                "plane.app.permissions.workspace.WorkspaceOwnerPermission"
            }
            Self::AppWorkSpaceAdminPermission => {
                "plane.app.permissions.workspace.WorkSpaceAdminPermission"
            }
            Self::AppWorkspaceEntityPermission => {
                "plane.app.permissions.workspace.WorkspaceEntityPermission"
            }
            Self::AppWorkspaceViewerPermission => {
                "plane.app.permissions.workspace.WorkspaceViewerPermission"
            }
            Self::AppWorkspaceUserPermission => {
                "plane.app.permissions.workspace.WorkspaceUserPermission"
            }
            Self::AppWorkspaceMemberPermission => {
                "plane.app.permissions.workspace.WorkspaceMemberPermission"
            }
            Self::AppProjectBasePermission => "plane.app.permissions.project.ProjectBasePermission",
            Self::AppProjectArchiveUnarchivePermission => {
                "plane.app.permissions.project.ProjectArchiveUnarchivePermission"
            }
            Self::AppProjectMemberPermission => {
                "plane.app.permissions.project.ProjectMemberPermission"
            }
            Self::AppProjectEntityPermission => {
                "plane.app.permissions.project.ProjectEntityPermission"
            }
            Self::AppProjectAdminPermission => {
                "plane.app.permissions.project.ProjectAdminPermission"
            }
            Self::AppProjectLitePermission => "plane.app.permissions.project.ProjectLitePermission",
            Self::AppProjectPagePermission => "plane.app.permissions.page.ProjectPagePermission",
            Self::UtilsWorkSpaceBasePermission => {
                "plane.utils.permissions.workspace.WorkSpaceBasePermission"
            }
            Self::UtilsWorkspaceOwnerPermission => {
                "plane.utils.permissions.workspace.WorkspaceOwnerPermission"
            }
            Self::UtilsWorkSpaceAdminPermission => {
                "plane.utils.permissions.workspace.WorkSpaceAdminPermission"
            }
            Self::UtilsWorkspaceEntityPermission => {
                "plane.utils.permissions.workspace.WorkspaceEntityPermission"
            }
            Self::UtilsWorkspaceViewerPermission => {
                "plane.utils.permissions.workspace.WorkspaceViewerPermission"
            }
            Self::UtilsWorkspaceUserPermission => {
                "plane.utils.permissions.workspace.WorkspaceUserPermission"
            }
            Self::UtilsProjectBasePermission => {
                "plane.utils.permissions.project.ProjectBasePermission"
            }
            Self::UtilsProjectMemberPermission => {
                "plane.utils.permissions.project.ProjectMemberPermission"
            }
            Self::UtilsProjectEntityPermission => {
                "plane.utils.permissions.project.ProjectEntityPermission"
            }
            Self::UtilsProjectAdminPermission => {
                "plane.utils.permissions.project.ProjectAdminPermission"
            }
            Self::UtilsProjectLitePermission => {
                "plane.utils.permissions.project.ProjectLitePermission"
            }
            Self::UtilsProjectPagePermission => {
                "plane.utils.permissions.page.ProjectPagePermission"
            }
            Self::InstanceAdminPermission => {
                "plane.license.api.permissions.instance.InstanceAdminPermission"
            }
        }
    }

    /// `BasePermission.has_permission(request, view)`.
    ///
    /// Only the `(GET, HEAD, OPTIONS)` split, the anonymous guard and the role
    /// predicates matter; `view` is reduced to the fields [`PermCtx`] carries.
    pub async fn check(self, ctx: &PermCtx<'_>) -> Result<bool, Error> {
        match self {
            // `AllowAny.has_permission` returns True unconditionally.
            Self::AllowAny => Ok(true),
            // `bool(request.user and request.user.is_authenticated)`; DRF's
            // `request.user` is never None, so this is `is_authenticated()`.
            Self::IsAuthenticated => Ok(ctx.user.is_authenticated()),

            Self::AppWorkSpaceBasePermission | Self::UtilsWorkSpaceBasePermission => {
                workspace_base(ctx).await
            }
            Self::AppWorkspaceOwnerPermission | Self::UtilsWorkspaceOwnerPermission => {
                workspace_owner(ctx).await
            }
            Self::AppWorkSpaceAdminPermission | Self::UtilsWorkSpaceAdminPermission => {
                workspace_admin(ctx).await
            }
            Self::AppWorkspaceEntityPermission | Self::UtilsWorkspaceEntityPermission => {
                workspace_entity(ctx).await
            }
            Self::AppWorkspaceViewerPermission | Self::UtilsWorkspaceViewerPermission => {
                workspace_viewer(ctx).await
            }
            Self::AppWorkspaceUserPermission | Self::UtilsWorkspaceUserPermission => {
                workspace_viewer(ctx).await
            }
            Self::AppWorkspaceMemberPermission => workspace_member_permission(ctx).await,

            Self::AppProjectBasePermission | Self::UtilsProjectBasePermission => {
                project_base(ctx).await
            }
            Self::AppProjectArchiveUnarchivePermission => project_archive_unarchive(ctx).await,
            Self::AppProjectMemberPermission | Self::UtilsProjectMemberPermission => {
                project_member(ctx).await
            }
            Self::AppProjectEntityPermission | Self::UtilsProjectEntityPermission => {
                project_entity(ctx).await
            }
            Self::AppProjectAdminPermission | Self::UtilsProjectAdminPermission => {
                project_admin(ctx).await
            }
            Self::AppProjectLitePermission | Self::UtilsProjectLitePermission => {
                project_lite(ctx).await
            }

            Self::AppProjectPagePermission | Self::UtilsProjectPagePermission => {
                project_page_permission(ctx).await
            }

            Self::InstanceAdminPermission => instance_admin_permission(ctx).await,
        }
    }
}

/// `has_permission(class, ctx)`: the free-function form the DRF runtime calls.
///
/// Same as [`PermissionClass::check`], kept as a function so a policy table can
/// be applied with `has_permission` without knowing the enum's method.
pub async fn has_permission(class: PermissionClass, ctx: &PermCtx<'_>) -> Result<bool, Error> {
    class.check(ctx).await
}

/// `WorkSpaceBasePermission.has_permission` (`plane/app/permissions/workspace.py:19-48`).
///
/// POST is allowed for any authenticated caller (workspace creation), safe
/// methods are allowed and filtered later by the queryset, PUT/PATCH/DELETE
/// need an ADMIN workspace membership. Any other method (TRACE) falls off the
/// end and returns `None`, which DRF treats as a deny.
async fn workspace_base(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    if *ctx.method == http::Method::POST || is_safe_method(ctx.method) {
        return Ok(true);
    }
    if matches!(
        *ctx.method,
        http::Method::PUT | http::Method::PATCH | http::Method::DELETE
    ) {
        let role = workspace_member_role(ctx, ctx.workspace_slug()).await?;
        return Ok(is_admin(role));
    }
    Ok(false)
}

/// `WorkspaceOwnerPermission.has_permission` (`workspace.py:51-58`).
async fn workspace_owner(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    let role = workspace_member_role(ctx, ctx.workspace_slug()).await?;
    Ok(is_admin(role))
}

/// `WorkSpaceAdminPermission.has_permission` (`workspace.py:61-71`):
/// `role in [ADMIN, MEMBER]`.
async fn workspace_admin(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    let role = workspace_member_role(ctx, ctx.workspace_slug()).await?;
    Ok(is_admin_or_member(role))
}

/// `WorkspaceEntityPermission.has_permission` (`workspace.py:74-90`):
/// any active member may read, ADMIN/MEMBER may write.
async fn workspace_entity(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    let role = workspace_member_role(ctx, ctx.workspace_slug()).await?;
    if is_safe_method(ctx.method) {
        return Ok(role.is_some());
    }
    Ok(is_admin_or_member(role))
}

/// `WorkspaceViewerPermission` / `WorkspaceUserPermission`
/// (`workspace.py:93-110`): an active membership at any role.
async fn workspace_viewer(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    let role = workspace_member_role(ctx, ctx.workspace_slug()).await?;
    Ok(role.is_some())
}

/// `WorkspaceMemberPermission.has_permission` (`workspace.py:113-137`).
///
/// The workspace is resolved from `workspace_id` when the route provides it,
/// otherwise from `slug`; a route with neither always denies.
async fn workspace_member_permission(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    if let Some(workspace_id) = ctx
        .kwargs
        .get("workspace_id")
        .filter(|value| !value.is_empty())
    {
        let role = workspace_member_role_by_id(ctx, workspace_id).await?;
        return Ok(role.is_some());
    }
    if let Some(slug) = ctx.kwargs.get("slug").filter(|value| !value.is_empty()) {
        let role = workspace_member_role(ctx, Some(slug)).await?;
        return Ok(role.is_some());
    }
    Ok(false)
}

/// `ProjectBasePermission.has_permission` (`project.py:13-53`).
async fn project_base(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    if is_safe_method(ctx.method) {
        let role = workspace_member_role(ctx, ctx.workspace_slug()).await?;
        return Ok(role.is_some());
    }
    if *ctx.method == http::Method::POST {
        let role = workspace_member_role(ctx, ctx.workspace_slug()).await?;
        return Ok(is_admin_or_member(role));
    }
    project_admin_fallback(ctx).await
}

/// `ProjectArchiveUnarchivePermission.has_permission` (`project.py:56-98`).
///
/// Unlike `ProjectBasePermission` it demands an active workspace membership
/// first for every method, then the same project-admin-or-workspace-admin rule
/// as the other project checks; its POST branch is not project creation.
async fn project_archive_unarchive(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    let workspace = workspace_member_role(ctx, ctx.workspace_slug()).await?;
    if workspace.is_none() {
        return Ok(false);
    }
    let project = project_member_role(ctx, ctx.workspace_slug(), ctx.project_id()).await?;
    if is_admin_or_member(project) {
        return Ok(true);
    }
    Ok(project.is_some() && is_admin(workspace))
}

/// `ProjectMemberPermission.has_permission` (`project.py:101-132`).
///
/// Safe methods need any active project membership; POST and the remaining
/// unsafe methods need ADMIN or MEMBER.
async fn project_member(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    let project = project_member_role(ctx, ctx.workspace_slug(), ctx.project_id()).await?;
    if is_safe_method(ctx.method) {
        return Ok(project.is_some());
    }
    Ok(is_admin_or_member(project))
}

/// `ProjectEntityPermission.has_permission` (`project.py:135-166`).
///
/// The `project_identifier` branch is only reached when the view exposes the
/// property and it is truthy, and only safe methods take it; an unsafe method
/// on such a route falls through to the `project_id` rules.
async fn project_entity(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    if let Some(identifier) = ctx
        .view_project_identifier
        .filter(|identifier| !identifier.is_empty())
        && is_safe_method(ctx.method)
    {
        return project_member_exists_by_identifier(ctx, ctx.workspace_slug(), identifier).await;
    }
    let project = project_member_role(ctx, ctx.workspace_slug(), ctx.project_id()).await?;
    if is_safe_method(ctx.method) {
        return Ok(project.is_some());
    }
    Ok(is_admin_or_member(project))
}

/// `ProjectAdminPermission.has_permission` (`project.py:169-192`).
async fn project_admin(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    project_admin_fallback(ctx).await
}

/// `ProjectLitePermission.has_permission` (`project.py:195-205`).
async fn project_lite(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }
    let project = project_member_role(ctx, ctx.workspace_slug(), ctx.project_id()).await?;
    Ok(project.is_some())
}

/// The shared tail of `ProjectBasePermission` and `ProjectAdminPermission`:
/// a project ADMIN, or any project member who is also a workspace ADMIN.
async fn project_admin_fallback(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    let project = project_member_role(ctx, ctx.workspace_slug(), ctx.project_id()).await?;
    if is_admin(project) {
        return Ok(true);
    }
    if project.is_none() {
        return Ok(false);
    }
    let workspace = workspace_member_role(ctx, ctx.workspace_slug()).await?;
    Ok(is_admin(workspace))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_paths_round_trip() {
        let all = [
            PermissionClass::AllowAny,
            PermissionClass::IsAuthenticated,
            PermissionClass::AppWorkSpaceBasePermission,
            PermissionClass::AppWorkspaceOwnerPermission,
            PermissionClass::AppWorkSpaceAdminPermission,
            PermissionClass::AppWorkspaceEntityPermission,
            PermissionClass::AppWorkspaceViewerPermission,
            PermissionClass::AppWorkspaceUserPermission,
            PermissionClass::AppWorkspaceMemberPermission,
            PermissionClass::AppProjectBasePermission,
            PermissionClass::AppProjectArchiveUnarchivePermission,
            PermissionClass::AppProjectMemberPermission,
            PermissionClass::AppProjectEntityPermission,
            PermissionClass::AppProjectAdminPermission,
            PermissionClass::AppProjectLitePermission,
            PermissionClass::AppProjectPagePermission,
            PermissionClass::UtilsWorkSpaceBasePermission,
            PermissionClass::UtilsWorkspaceOwnerPermission,
            PermissionClass::UtilsWorkSpaceAdminPermission,
            PermissionClass::UtilsWorkspaceEntityPermission,
            PermissionClass::UtilsWorkspaceViewerPermission,
            PermissionClass::UtilsWorkspaceUserPermission,
            PermissionClass::UtilsProjectBasePermission,
            PermissionClass::UtilsProjectMemberPermission,
            PermissionClass::UtilsProjectEntityPermission,
            PermissionClass::UtilsProjectAdminPermission,
            PermissionClass::UtilsProjectLitePermission,
            PermissionClass::UtilsProjectPagePermission,
            PermissionClass::InstanceAdminPermission,
        ];
        for class in all {
            assert_eq!(PermissionClass::from_dotted(class.dotted()), Some(class));
        }
        assert_eq!(
            PermissionClass::from_dotted("plane.app.permissions.ROLE"),
            None
        );
        assert_eq!(
            PermissionClass::from_dotted("rest_framework.permissions.IsAdminUser"),
            None
        );
        // These two only exist in `plane/app`.
        assert_eq!(
            PermissionClass::from_dotted(
                "plane.utils.permissions.workspace.WorkspaceMemberPermission"
            ),
            None
        );
        assert_eq!(
            PermissionClass::from_dotted(
                "plane.utils.permissions.project.ProjectArchiveUnarchivePermission"
            ),
            None
        );
    }
}
