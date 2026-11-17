// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/utils/redirection_path.py` — where an app login goes
//! when the session has no `next_path`.
//!
//! ```python
//! profile, _ = Profile.objects.get_or_create(user=user)
//! if not profile.is_onboarded: return "onboarding"
//! if profile.last_workspace_id and Workspace.objects.filter(...).exists():
//!     workspace = Workspace.objects.filter(...).first()
//!     return f"{workspace.slug}"
//! fallback_workspace = Workspace.objects.filter(...).order_by("created_at").first()
//! if fallback_workspace: return f"{fallback_workspace.slug}"
//! if WorkspaceMemberInvite.objects.filter(email=user.email).count(): return "invitations"
//! return "create-workspace"
//! ```
//!
//! Every answer is a **bare slug** (`"onboarding"`, `"fix-workspace"`, …), and
//! the callers pass it to `validate_next_path`, which keeps only values that
//! start with `/` (`utils/path_validator.py:132-133`). The redirection is
//! therefore always the app root; the two-query workspace lookup is ported as
//! written anyway (`AUTH_FLOWS.md` §2a).

use deadpool_postgres::Client;
use plane_auth::auth_flow::adapter::AdapterError;
use plane_auth::auth_flow::adapter::create_profile;
use plane_db::models::user::User;

/// `get_redirection_path(user)` (`redirection_path.py:8-47`).
///
/// # Errors
///
/// A database failure, or `Profile.objects.get`'s `MultipleObjectsReturned`
/// (unreachable: `profiles.user_id` is unique).
pub async fn get_redirection_path(client: &Client, user: &User) -> Result<String, AdapterError> {
    // `Profile.objects.get_or_create(user=user)`.
    let statement = super::queries::PROFILE_BY_USER.prepare(client).await?;
    let rows = client.query(&statement, &[&user.id]).await?;
    let (is_onboarded, last_workspace_id) = match rows.as_slice() {
        [] => {
            create_profile(client, user).await?;
            (false, None)
        }
        [row] => (row.get::<_, bool>(0), row.get::<_, Option<uuid::Uuid>>(1)),
        _ => {
            return Err(AdapterError::Internal(
                "MultipleObjectsReturned: Profile.objects.get(user=…)".to_string(),
            ));
        }
    };
    if !is_onboarded {
        return Ok("onboarding".to_string());
    }

    if let Some(workspace_id) = last_workspace_id {
        let statement = super::queries::WORKSPACE_SLUG_FOR_LAST
            .prepare(client)
            .await?;
        if let Some(row) = client
            .query_opt(&statement, &[&workspace_id, &user.id])
            .await?
        {
            return Ok(row.get(0));
        }
    }

    let statement = super::queries::FALLBACK_WORKSPACE_SLUG
        .prepare(client)
        .await?;
    if let Some(row) = client.query_opt(&statement, &[&user.id]).await? {
        return Ok(row.get(0));
    }

    let statement = super::queries::INVITE_EXISTS_FOR_EMAIL
        .prepare(client)
        .await?;
    let row = client.query_one(&statement, &[&user.email]).await?;
    if row.get::<_, bool>(0) {
        return Ok("invitations".to_string());
    }

    Ok("create-workspace".to_string())
}
