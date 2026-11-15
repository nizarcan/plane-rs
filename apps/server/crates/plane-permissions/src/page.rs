// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ProjectPagePermission` (`plane/app/permissions/page.py:18-143`; the utils
//! copy is byte-identical).
//!
//! It is the only permission class with a query chain rather than one
//! membership test, and it has three behaviours worth naming:
//!
//! 1. **The owner branch ignores the method.** A page owner passes every verb,
//!    including DELETE on a public page where the role table would deny. That is
//!    the Python as written (`page.py:57-59` returns before
//!    `_has_public_page_action_access`), so it is ported as is.
//! 2. **No project link, no access.** The page must be linked to the URL
//!    project by a *non-deleted* `ProjectPage` row, and the page row itself must
//!    not be soft-deleted (`Page.objects`).
//! 3. **Private pages are owner-only.** The private-page hook returns False for
//!    everyone; there is no subclass override anywhere in the tree.
//!
//! It reads `view.kwargs.get("project_id")` directly, not the
//! `view.project_id` property (`page.py:34`), so the `pk` fallback does not
//! apply here.

use crate::membership::project_member_role;
use crate::{Error, PermCtx, Role, parse_uuid, queries as catalog};

/// `ProjectPagePermission.has_permission`.
pub(crate) async fn project_page_permission(ctx: &PermCtx<'_>) -> Result<bool, Error> {
    if !ctx.user.is_authenticated() {
        return Ok(false);
    }

    let user_id = ctx.user_id()?;
    let slug = ctx.workspace_slug();
    let page_id = ctx.kwargs.get("page_id");
    let project_id = ctx.raw_project_id();

    // `_check_access_and_get_role`: an active project membership at any role is
    // required before anything else. `if not role` treats a stored 0 as absent.
    let role = project_member_role(ctx, slug, project_id).await?;
    let Some(role) = role.filter(|role| *role != 0) else {
        return Ok(false);
    };

    if let Some(page_id) = page_id.filter(|page_id| !page_id.is_empty()) {
        let Some(page) = page_for_permission(ctx, page_id, slug, project_id).await? else {
            return Ok(false);
        };

        // Owner: allowed for every method, before any access/role check.
        if page.owned_by_id == user_id {
            return Ok(true);
        }

        // Guests may only touch pages they own unless the project opts in.
        if role == Role::Guest.value() && !project_guest_view_all(ctx, project_id).await? {
            return Ok(false);
        }

        // Private page: `_has_private_page_action_access` is `False` in the
        // base class and overridden nowhere.
        if page.access == 1 {
            return Ok(false);
        }
    }

    Ok(project_action_access(ctx.method, role))
}

/// One row of [`catalog::PAGE_FOR_PERMISSION`].
struct PageRow {
    owned_by_id: uuid::Uuid,
    access: i16,
}

async fn page_for_permission(
    ctx: &PermCtx<'_>,
    page_id: &str,
    slug: Option<&str>,
    project_id: Option<&str>,
) -> Result<Option<PageRow>, Error> {
    let page_id = parse_uuid("page_id", page_id)?;
    // `project_pages.project_id IS NULL` / `workspaces.slug IS NULL` match
    // nothing: both columns are NOT NULL.
    let (Some(slug), Some(project_id)) = (slug, project_id) else {
        return Ok(None);
    };
    let project_id = parse_uuid("project_id", project_id)?;
    let statement = catalog::PAGE_FOR_PERMISSION.prepare(ctx.client).await?;
    let row = ctx
        .client
        .query_opt(&statement, &[&page_id, &project_id, &slug])
        .await?;
    Ok(row.map(|row| PageRow {
        owned_by_id: row.get(0),
        access: row.get(1),
    }))
}

/// `Project.objects.filter(pk=project_id, guest_view_all_features=True).exists()`.
async fn project_guest_view_all(
    ctx: &PermCtx<'_>,
    project_id: Option<&str>,
) -> Result<bool, Error> {
    let Some(project_id) = project_id.filter(|value| !value.is_empty()) else {
        return Ok(false);
    };
    let project_id = parse_uuid("project_id", project_id)?;
    let statement = catalog::PROJECT_GUEST_VIEW_ALL.prepare(ctx.client).await?;
    let row = ctx.client.query_one(&statement, &[&project_id]).await?;
    Ok(row.get(0))
}

/// `_check_project_action_access` (`page.py:105-133`).
///
/// POST needs ADMIN/MEMBER; safe methods allow GUEST; PUT/PATCH need
/// ADMIN/MEMBER; DELETE needs ADMIN; anything else denies.
fn project_action_access(method: &http::Method, role: i16) -> bool {
    let admin = Role::Admin.value();
    let member = Role::Member.value();
    let guest = Role::Guest.value();
    match *method {
        http::Method::POST => role == admin || role == member,
        http::Method::PUT | http::Method::PATCH => role == admin || role == member,
        http::Method::DELETE => role == admin,
        _ if crate::is_safe_method(method) => role == admin || role == member || role == guest,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Method;

    #[test]
    fn role_table_matches_the_python() {
        assert!(project_action_access(&Method::POST, 20));
        assert!(project_action_access(&Method::POST, 15));
        assert!(!project_action_access(&Method::POST, 5));
        for method in [Method::GET, Method::HEAD, Method::OPTIONS] {
            assert!(project_action_access(&method, 20));
            assert!(project_action_access(&method, 15));
            assert!(project_action_access(&method, 5));
        }
        assert!(project_action_access(&Method::PUT, 20));
        assert!(project_action_access(&Method::PATCH, 15));
        assert!(!project_action_access(&Method::PUT, 5));
        assert!(project_action_access(&Method::DELETE, 20));
        assert!(!project_action_access(&Method::DELETE, 15));
        assert!(!project_action_access(&Method::DELETE, 5));
        assert!(!project_action_access(&Method::TRACE, 20));
    }
}
