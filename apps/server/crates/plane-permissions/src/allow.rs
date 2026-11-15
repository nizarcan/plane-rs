// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `allow_permission` (`plane/app/permissions/base.py:19-103`, byte-identical
//! in `plane/utils/permissions/base.py`).
//!
//! The decorator wraps a DRF view method. It never raises a DRF exception for a
//! deny; it returns `Response({"error": "You don't have the required
//! permissions."}, status=403)`, which the view hands back unchanged. That is
//! [`Decision::Deny`], not a [`crate::Denial`].
//!
//! Every route in `routes.json` that carries a decorator uses the app copy: no
//! view imports `allow_permission` from `plane.utils.permissions`, and both
//! modules define the function with the same body, so one implementation is
//! exact.

use crate::membership::{
    created_by_user, is_admin, project_member_role, role_allows, workspace_member_role,
};
use crate::{Error, Level, PermCtx, Role};

/// The `model` argument of `allow_permission(creator=True, model=…)`.
///
/// Eight models appear with `creator=True` in `routes.json`; each is one
/// `EXISTS` statement from [`crate::queries`]. `from_dotted` accepts exactly
/// the paths `routes.json` records, so a policy generator cannot silently pass
/// a model this crate does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreatorModel {
    Issue,
    IssueComment,
    FileAsset,
    Sticky,
    Cycle,
    Module,
    IssueView,
    DraftIssue,
}

impl CreatorModel {
    /// The dotted path `routes.json` records.
    pub const fn dotted(self) -> &'static str {
        match self {
            Self::Issue => "plane.db.models.issue.Issue",
            Self::IssueComment => "plane.db.models.issue.IssueComment",
            Self::FileAsset => "plane.db.models.asset.FileAsset",
            Self::Sticky => "plane.db.models.sticky.Sticky",
            Self::Cycle => "plane.db.models.cycle.Cycle",
            Self::Module => "plane.db.models.module.Module",
            Self::IssueView => "plane.db.models.view.IssueView",
            Self::DraftIssue => "plane.db.models.draft.DraftIssue",
        }
    }

    pub fn from_dotted(path: &str) -> Option<Self> {
        match path {
            "plane.db.models.issue.Issue" => Some(Self::Issue),
            "plane.db.models.issue.IssueComment" => Some(Self::IssueComment),
            "plane.db.models.asset.FileAsset" => Some(Self::FileAsset),
            "plane.db.models.sticky.Sticky" => Some(Self::Sticky),
            "plane.db.models.cycle.Cycle" => Some(Self::Cycle),
            "plane.db.models.module.Module" => Some(Self::Module),
            "plane.db.models.view.IssueView" => Some(Self::IssueView),
            "plane.db.models.draft.DraftIssue" => Some(Self::DraftIssue),
            _ => None,
        }
    }

    pub(crate) const fn query(self) -> &'static plane_db::Query {
        match self {
            Self::Issue => &crate::queries::CREATOR_ISSUE,
            Self::IssueComment => &crate::queries::CREATOR_ISSUE_COMMENT,
            Self::FileAsset => &crate::queries::CREATOR_FILE_ASSET,
            Self::Sticky => &crate::queries::CREATOR_STICKY,
            Self::Cycle => &crate::queries::CREATOR_CYCLE,
            Self::Module => &crate::queries::CREATOR_MODULE,
            Self::IssueView => &crate::queries::CREATOR_ISSUE_VIEW,
            Self::DraftIssue => &crate::queries::CREATOR_DRAFT_ISSUE,
        }
    }
}

/// The arguments of one `@allow_permission(...)` decorator.
///
/// Mirrors the Python signature and defaults: `level="PROJECT"`,
/// `creator=False`, `model=None`. `roles` is the `allowed_roles` list in its
/// original order; empty means "creator only" (the Python builds
/// `role__in=[]`, which matches nothing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AllowPermission {
    pub roles: &'static [Role],
    pub level: Level,
    pub creator: bool,
    pub model: Option<CreatorModel>,
}

impl AllowPermission {
    /// A const constructor so a generated policy table can be a `static`.
    pub const fn new(
        roles: &'static [Role],
        level: Level,
        creator: bool,
        model: Option<CreatorModel>,
    ) -> Self {
        Self {
            roles,
            level,
            creator,
            model,
        }
    }
}

/// The body of the 403 `allow_permission` returns on a deny.
///
/// `DRF` renders `Response({"error": …}, status=403)` with its JSON renderer;
/// the runtime writes `{"error": <error>}` with status [`Self::STATUS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DenyResponse {
    pub status: u16,
    pub error: &'static str,
}

impl DenyResponse {
    pub const STATUS: u16 = 403;
    /// The only wording the decorator uses
    /// (`plane/app/permissions/base.py:32,47,97`).
    pub const NOT_ALLOWED: &'static str = "You don't have the required permissions.";

    pub const fn not_allowed() -> Self {
        Self {
            status: Self::STATUS,
            error: Self::NOT_ALLOWED,
        }
    }
}

/// The decorator's answer: run the view, or return the 403 body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(DenyResponse),
}

impl Decision {
    pub fn is_allowed(self) -> bool {
        matches!(self, Self::Allow)
    }
}

/// Port of the decorator body for one request.
///
/// Evaluation order (and therefore which error a bad request produces) follows
/// the Python exactly:
///
/// 1. `kwargs["slug"]` — a missing key raises `KeyError` before any query.
/// 2. the active workspace membership (403 if absent);
/// 3. for `level != WORKSPACE`, `kwargs["project_id"]` and the active project
///    membership (403 if absent);
/// 4. `kwargs["pk"]` and `model.objects.filter(id=pk, created_by=user)`;
/// 5. if neither branch returned: the role check, with the same membership rows
///    reused (`role__in=allowed`, then project membership + workspace admin).
pub async fn allow_permission(
    spec: &AllowPermission,
    ctx: &PermCtx<'_>,
) -> Result<Decision, Error> {
    // Step 1: `kwargs["slug"]` is evaluated while the queryset is built, so a
    // missing key wins over everything, including an anonymous user.
    let slug = ctx.kwargs.require("slug")?;

    let mut workspace_role: Option<Option<i16>> = None;
    let mut project_role: Option<Option<i16>> = None;

    if spec.creator
        && let Some(model) = spec.model
    {
        // `WorkspaceMember.objects.filter(member=…, workspace__slug=…,
        // is_active=True).exists()`.
        let role = workspace_member_role(ctx, Some(slug)).await?;
        workspace_role = Some(role);
        if role.is_none() {
            return Ok(Decision::Deny(DenyResponse::not_allowed()));
        }

        // `ProjectMember.objects.filter(member=…, workspace__slug=…,
        // project_id=…, is_active=True).exists()` for project-level specs.
        if spec.level != Level::Workspace {
            let project_id = ctx.kwargs.require("project_id")?;
            let role = project_member_role(ctx, Some(slug), Some(project_id)).await?;
            project_role = Some(role);
            if role.is_none() {
                return Ok(Decision::Deny(DenyResponse::not_allowed()));
            }
        }

        // `model.objects.filter(id=kwargs["pk"], created_by=request.user)`.
        let pk = ctx.kwargs.require("pk")?;
        if created_by_user(ctx, model, pk).await? {
            return Ok(Decision::Allow);
        }
    }

    // The role checks. `workspace_role`/`project_role` are reused when the
    // creator branch already fetched them: the filters select the same single
    // row, so the answer cannot change inside one check.
    match spec.level {
        Level::Workspace => {
            let role = match workspace_role {
                Some(role) => role,
                None => workspace_member_role(ctx, Some(slug)).await?,
            };
            if role_allows(role, spec.roles) {
                return Ok(Decision::Allow);
            }
        }
        Level::Project => {
            let role = match project_role {
                Some(role) => role,
                None => {
                    let project_id = ctx.kwargs.require("project_id")?;
                    let role = project_member_role(ctx, Some(slug), Some(project_id)).await?;
                    project_role = Some(role);
                    role
                }
            };

            if role_allows(role, spec.roles) {
                return Ok(Decision::Allow);
            }

            // `elif ProjectMember…exists() and WorkspaceMember…filter(
            // role=ROLE.ADMIN.value).exists()`. The project row exists when the
            // fetched role is `Some`; the workspace row is fetched only in that
            // case (Python's `and` short-circuits).
            if project_role.flatten().is_some() {
                let workspace = match workspace_role {
                    Some(role) => role,
                    None => workspace_member_role(ctx, Some(slug)).await?,
                };
                if is_admin(workspace) {
                    return Ok(Decision::Allow);
                }
            }
        }
    }

    Ok(Decision::Deny(DenyResponse::not_allowed()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creator_model_dotted_paths_round_trip() {
        for model in [
            CreatorModel::Issue,
            CreatorModel::IssueComment,
            CreatorModel::FileAsset,
            CreatorModel::Sticky,
            CreatorModel::Cycle,
            CreatorModel::Module,
            CreatorModel::IssueView,
            CreatorModel::DraftIssue,
        ] {
            assert_eq!(CreatorModel::from_dotted(model.dotted()), Some(model));
        }
        assert_eq!(CreatorModel::from_dotted("plane.db.models.user.User"), None);
    }

    #[test]
    fn deny_body_is_the_python_literal() {
        let deny = DenyResponse::not_allowed();
        assert_eq!(deny.status, 403);
        assert_eq!(deny.error, "You don't have the required permissions.");
    }

    #[test]
    fn defaults_match_the_python_signature() {
        let spec = AllowPermission::default();
        assert_eq!(spec.level, Level::Project);
        assert!(!spec.creator);
        assert_eq!(spec.model, None);
        assert!(spec.roles.is_empty());
    }
}
