// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Reading the lite project/workspace/user columns a joined statement returns.
//!
//! Three statements of this group already join the rows the nested serializers
//! render (`PROJECT_INVITATIONS`, `PROJECT_MEMBER_ONE`, `PROJECT_MEMBER_SELF`,
//! the deploy-board pair). Selecting their columns instead of letting Django
//! load each relation lazily is what keeps these routes at Django's round-trip
//! count or below; the `*_url` properties are resolved by one batched asset
//! query per response (`super::lite`).

use plane_db::JsonText;
use plane_drf::error::ViewError;
use tokio_postgres::Row;
use uuid::Uuid;

use crate::projects::lite::{LiteProject, LiteUser, LiteWorkspace};

/// The project columns, starting at `start`:
/// `p.id, p.identifier, p.name, p.cover_image, p.cover_image_asset_id,
/// p.logo_props, p.description`.
pub fn project_at(row: &Row, start: usize) -> Result<LiteProject, ViewError> {
    let id: Option<Uuid> = row.get(start);
    let Some(id) = id else {
        // The `LEFT OUTER JOIN` found no row, which Django's lazy accessor
        // raises `RelatedObjectDoesNotExist` for (a 500 from the family handler).
        return Err(ViewError::Unknown(
            "a nested project row referenced by this response does not exist".to_string(),
        ));
    };
    Ok(LiteProject {
        id,
        identifier: row.get(start + 1),
        name: row.get(start + 2),
        cover_image: row.get(start + 3),
        cover_image_asset_id: row.get(start + 4),
        cover_image_url: None,
        logo_props: row.get::<_, JsonText>(start + 5),
        description: row.get(start + 6),
    })
}

/// The workspace columns, starting at `start`:
/// `w.name, w.slug, w.id, w.logo, w.logo_asset_id`.
pub fn workspace_at(row: &Row, start: usize) -> Result<LiteWorkspace, ViewError> {
    let id: Option<Uuid> = row.get(start + 2);
    let Some(id) = id else {
        return Err(ViewError::Unknown(
            "a nested workspace row referenced by this response does not exist".to_string(),
        ));
    };
    Ok(LiteWorkspace {
        name: row.get(start),
        slug: row.get(start + 1),
        id,
        logo: row.get(start + 3),
        logo_asset_id: row.get(start + 4),
        logo_url: None,
    })
}

/// The user columns, starting at `start`:
/// `u.id, u.first_name, u.last_name, u.avatar, u.avatar_asset_id, u.is_bot,
/// u.display_name, u.email, u.last_login_medium`.
pub fn user_at(row: &Row, start: usize) -> Result<LiteUser, ViewError> {
    let id: Option<Uuid> = row.get(start);
    let Some(id) = id else {
        return Err(ViewError::Unknown(
            "a nested user row referenced by this response does not exist".to_string(),
        ));
    };
    Ok(LiteUser {
        id,
        first_name: row.get(start + 1),
        last_name: row.get(start + 2),
        avatar: row.get(start + 3),
        avatar_asset_id: row.get(start + 4),
        avatar_url: None,
        is_bot: row.get(start + 5),
        display_name: row.get(start + 6),
        email: row.get(start + 7),
        last_login_medium: row.get(start + 8),
    })
}
