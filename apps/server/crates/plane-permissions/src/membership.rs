// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Membership lookups shared by the permission checks.
//!
//! Each function is one Django queryset reduced to the single fact the Python
//! asks of it (a role, or existence), with Django's parameter-validation order:
//! the user id is resolved before a missing/`None` project or workspace is
//! short-circuited, because Django still compiles the `member_id` predicate —
//! and therefore raises `ValidationError` for `AnonymousUser` — even when the
//! project predicate is `IS NULL`.

use uuid::Uuid;

use crate::allow::CreatorModel;
use crate::queries as catalog;
use crate::{Error, PermCtx, Role};

/// `WorkspaceMember.objects.filter(member=…, workspace__slug=…, is_active=True)`
/// → the row's role, or `None` when no row matches.
pub(crate) async fn workspace_member_role(
    ctx: &PermCtx<'_>,
    slug: Option<&str>,
) -> Result<Option<i16>, Error> {
    let user = ctx.user_id()?;
    let Some(slug) = slug else {
        // `workspaces.slug IS NULL` matches nothing: the column is NOT NULL.
        return Ok(None);
    };
    let statement = catalog::WORKSPACE_MEMBER_ROLE.prepare(ctx.client).await?;
    let row = ctx.client.query_opt(&statement, &[&user, &slug]).await?;
    Ok(row.map(|row| row.get(0)))
}

/// `WorkspaceMember.objects.filter(workspace_id=…, member=…, is_active=True)`.
pub(crate) async fn workspace_member_role_by_id(
    ctx: &PermCtx<'_>,
    workspace_id: &str,
) -> Result<Option<i16>, Error> {
    let user = ctx.user_id()?;
    let workspace_id: Uuid = crate::parse_uuid("workspace_id", workspace_id)?;
    let statement = catalog::WORKSPACE_MEMBER_ROLE_BY_ID
        .prepare(ctx.client)
        .await?;
    let row = ctx
        .client
        .query_opt(&statement, &[&workspace_id, &user])
        .await?;
    Ok(row.map(|row| row.get(0)))
}

/// `ProjectMember.objects.filter(member=…, workspace__slug=…, project_id=…,
/// is_active=True)` → the row's role, or `None`.
pub(crate) async fn project_member_role(
    ctx: &PermCtx<'_>,
    slug: Option<&str>,
    project_id: Option<&str>,
) -> Result<Option<i16>, Error> {
    let user = ctx.user_id()?;
    let (Some(slug), Some(project_id)) = (slug, project_id) else {
        // Either column is NOT NULL, so `IS NULL` matches nothing.
        return Ok(None);
    };
    let project_id: Uuid = crate::parse_uuid("project_id", project_id)?;
    let statement = catalog::PROJECT_MEMBER_ROLE.prepare(ctx.client).await?;
    let row = ctx
        .client
        .query_opt(&statement, &[&user, &project_id, &slug])
        .await?;
    Ok(row.map(|row| row.get(0)))
}

/// `ProjectMember.objects.filter(workspace__slug=…, member=…,
/// project__identifier=…, is_active=True).exists()`.
pub(crate) async fn project_member_exists_by_identifier(
    ctx: &PermCtx<'_>,
    slug: Option<&str>,
    identifier: &str,
) -> Result<bool, Error> {
    let user = ctx.user_id()?;
    let Some(slug) = slug else {
        return Ok(false);
    };
    let statement = catalog::PROJECT_MEMBER_EXISTS_BY_IDENTIFIER
        .prepare(ctx.client)
        .await?;
    let row = ctx
        .client
        .query_one(&statement, &[&user, &identifier, &slug])
        .await?;
    Ok(row.get(0))
}

/// `model.objects.filter(id=kwargs["pk"], created_by=request.user).exists()`.
pub(crate) async fn created_by_user(
    ctx: &PermCtx<'_>,
    model: CreatorModel,
    pk: &str,
) -> Result<bool, Error> {
    // Django preps `id` before `created_by`, so a malformed id wins over an
    // anonymous user. Both are unreachable after the membership guards, but
    // the order is kept.
    let pk: Uuid = crate::parse_uuid("pk", pk)?;
    let user = ctx.user_id()?;
    let query = model.query();
    let statement = query.prepare(ctx.client).await?;
    let row = ctx.client.query_one(&statement, &[&pk, &user]).await?;
    Ok(row.get(0))
}

/// `role__in=[...]` for one already-fetched row.
///
/// `None` (no row) never matches, and an empty role list never matches —
/// Django compiles `role__in=[]` to an `EmptyResultSet`.
pub(crate) fn role_allows(role: Option<i16>, roles: &[Role]) -> bool {
    match role {
        Some(value) => roles.iter().any(|role| role.value() == value),
        None => false,
    }
}

/// `role == ROLE.ADMIN.value`.
pub(crate) fn is_admin(role: Option<i16>) -> bool {
    role == Some(Role::Admin.value())
}

/// `role in [ROLE.ADMIN.value, ROLE.MEMBER.value]`.
pub(crate) fn is_admin_or_member(role: Option<i16>) -> bool {
    matches!(role, Some(value) if value == Role::Admin.value() || value == Role::Member.value())
}
