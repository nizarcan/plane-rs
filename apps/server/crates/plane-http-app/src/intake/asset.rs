// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `FileAsset.asset_url` and the `*_url` properties this group's serializers
//! render.
//!
//! Python (`plane/db/models/asset.py:83-104`):
//!
//! ```python
//! @property
//! def asset_url(self):
//!     if self.entity_type in (WORKSPACE_LOGO, USER_AVATAR, USER_COVER, PROJECT_COVER):
//!         return f"/api/assets/v2/static/{self.id}/"
//!     if self.entity_type == ISSUE_ATTACHMENT:
//!         return f"/api/assets/v2/workspaces/{self.workspace.slug}/projects/{self.project_id}/issues/{self.issue_id}/attachments/{self.id}/"
//!     if self.entity_type in (ISSUE_DESCRIPTION, COMMENT_DESCRIPTION, PAGE_DESCRIPTION, DRAFT_ISSUE_DESCRIPTION):
//!         return f"/api/assets/v2/workspaces/{self.workspace.slug}/projects/{self.project_id}/{self.id}/"
//!     return None
//! ```
//!
//! A `None` id renders as the string `"None"` because Python's f-string does
//! that; it cannot happen for a row that passed the FK, but the port keeps it.
//!
//! Loaded through [`crate::intake::queries::ASSETS_BY_IDS`], which does **not**
//! apply the soft-delete predicate: `ForeignKey.__get__` uses the base manager
//! (`django/db/models/fields/related_descriptors.py:445-470`), so a
//! soft-deleted avatar or cover image still has a URL. `issues` and
//! `notifications` both use this module.

use std::collections::{HashMap, HashSet};

use deadpool_postgres::Client;
use plane_drf::error::ViewError;
use uuid::Uuid;

use crate::intake::queries;

/// `FileAsset.EntityTypeContext` values `asset_url` treats as static.
const STATIC_TYPES: &[&str] = &[
    "WORKSPACE_LOGO",
    "USER_AVATAR",
    "USER_COVER",
    "PROJECT_COVER",
];

/// Entity types whose URL embeds the workspace slug and project id.
const PROJECT_SCOPED_TYPES: &[&str] = &[
    "ISSUE_DESCRIPTION",
    "COMMENT_DESCRIPTION",
    "PAGE_DESCRIPTION",
    "DRAFT_ISSUE_DESCRIPTION",
];

/// Asset id → its `asset_url` (`None` when the entity type has no URL).
pub type AssetUrls = HashMap<Uuid, Option<String>>;

/// `FileAsset.asset_url` for the columns [`queries::ASSETS_BY_IDS`] selects.
pub fn asset_url(
    id: Uuid,
    entity_type: Option<&str>,
    project_id: Option<Uuid>,
    issue_id: Option<Uuid>,
    workspace_slug: Option<&str>,
) -> Option<String> {
    let entity_type = entity_type.unwrap_or_default();
    let uuid_text =
        |value: Option<Uuid>| value.map_or_else(|| "None".to_string(), |id| id.to_string());
    let slug = workspace_slug.unwrap_or("None");
    if STATIC_TYPES.contains(&entity_type) {
        return Some(format!("/api/assets/v2/static/{id}/"));
    }
    if entity_type == "ISSUE_ATTACHMENT" {
        return Some(format!(
            "/api/assets/v2/workspaces/{slug}/projects/{}/issues/{}/attachments/{id}/",
            uuid_text(project_id),
            uuid_text(issue_id),
        ));
    }
    if PROJECT_SCOPED_TYPES.contains(&entity_type) {
        return Some(format!(
            "/api/assets/v2/workspaces/{slug}/projects/{}/{id}/",
            uuid_text(project_id),
        ));
    }
    None
}

/// [`asset_url`] for a set of ids, in one statement. Ids with no row are absent
/// from the map; the caller turns that into Django's `RelatedObjectDoesNotExist`
/// (`AttributeError`, i.e. a 500 through the family handler).
pub async fn load_asset_urls(client: &Client, ids: &HashSet<Uuid>) -> Result<AssetUrls, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let ids: Vec<Uuid> = ids.iter().copied().collect();
    let statement = client
        .prepare_cached(queries::ASSETS_BY_IDS.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&ids])
        .await
        .map_err(ViewError::from)?;
    let mut urls = HashMap::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.get(0);
        let slug: Option<String> = row.get(4);
        urls.insert(
            id,
            asset_url(id, row.get(1), row.get(2), row.get(3), slug.as_deref()),
        );
    }
    Ok(urls)
}

/// The three-branch property (`if asset: …; if text: …; return None`) with the
/// asset branch already resolved.
pub fn asset_or_text(
    asset_id: Option<Uuid>,
    text: String,
    urls: &AssetUrls,
) -> Result<Option<String>, ViewError> {
    match asset_id {
        Some(id) => match urls.get(&id) {
            Some(url) => Ok(url.clone()),
            // The FK points at a row the base manager cannot see: Django raises
            // `RelatedObjectDoesNotExist` out of the property, which the family
            // handler renders as a 500.
            None => Err(ViewError::Unknown(format!(
                "the file asset {id} referenced by this row does not exist"
            ))),
        },
        None if !text.is_empty() => Ok(Some(text)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_assets_lose_every_other_field() {
        let id = Uuid::nil();
        assert_eq!(
            asset_url(id, Some("USER_AVATAR"), Some(id), Some(id), Some("ws")),
            Some(format!("/api/assets/v2/static/{id}/"))
        );
    }

    #[test]
    fn the_unknown_type_has_no_url() {
        assert_eq!(
            asset_url(Uuid::nil(), Some("OTHER"), None, None, None),
            None
        );
        assert_eq!(asset_url(Uuid::nil(), None, None, None, None), None);
    }

    #[test]
    fn a_missing_slug_or_id_renders_python_none() {
        let id = Uuid::nil();
        assert_eq!(
            asset_url(id, Some("ISSUE_ATTACHMENT"), None, None, None),
            Some(format!(
                "/api/assets/v2/workspaces/None/projects/None/issues/None/attachments/{id}/"
            ))
        );
    }

    #[test]
    fn a_text_cover_wins_only_without_an_asset() {
        let urls = AssetUrls::new();
        assert_eq!(
            asset_or_text(None, String::new(), &urls).expect("none"),
            None
        );
        assert_eq!(
            asset_or_text(None, "https://example.test/a.png".to_string(), &urls).expect("text"),
            Some("https://example.test/a.png".to_string())
        );
    }

    #[test]
    fn an_unresolved_asset_is_the_python_attribute_error() {
        let err = asset_or_text(Some(Uuid::nil()), String::new(), &AssetUrls::new())
            .expect_err("missing asset");
        assert!(matches!(err, ViewError::Unknown(_)));
    }
}
