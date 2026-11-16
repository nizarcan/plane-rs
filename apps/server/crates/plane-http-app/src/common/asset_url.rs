// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The model properties that render a `FileAsset` as a local API path:
//!
//! * `FileAsset.asset_url` (`plane/db/models/asset.py:83-104`) — [`file_asset_url`]
//!   and, for one row of the five joined columns, [`AssetRef::asset_url`];
//! * `User.avatar_url` / `User.cover_image_url`
//!   (`plane/db/models/user.py:143-167`) — [`user_avatar_url`],
//!   [`user_cover_image_url`];
//! * `Workspace.logo_url` (`plane/db/models/workspace.py:145-154`) —
//!   [`workspace_logo_url`];
//! * `Project.cover_image_url` (`plane/db/models/project.py:129-139`) —
//!   [`project_cover_image_url`].
//!
//! The four `<entity>_url` properties have the same two-branch shape: the
//! related `FileAsset` wins when the foreign key points at a row, and the
//! legacy text column is the fallback. They share [`AssetRef`], the five
//! columns every branch reads.
//!
//! ## Why a `Result`
//!
//! The Python reads `self.workspace.slug` for the non-static branches
//! (`asset.py:94,102`). A `FileAsset` row always has a workspace, but a
//! `select_related` join can produce `None` in Rust where Python would raise
//! `AttributeError: 'NoneType' object has no attribute 'workspace'`, which
//! Plane's family `handle_exception` turns into a 500. Keeping the `Result`
//! means that case is the same 500 here instead of a silently different URL.
//!
//! The `None` ids inside an f-string are rendered `"None"`, which is what
//! Python does (`f"{None}"`); the tests pin it because a fixture row can reach
//! it (an `ISSUE_ATTACHMENT` with no project, say).

use uuid::Uuid;

use plane_drf::error::ViewError;

/// The columns of a related `FileAsset` a model property needs to compute
/// `asset_url` (`plane/db/models/asset.py:83-104`).
///
/// The related object is read through Django's `_base_manager`, which does
/// **not** apply the soft-delete predicate: a soft-deleted avatar still
/// renders. The SQL that fills this struct therefore joins `file_assets`
/// without a `deleted_at` filter, exactly as `select_related` does.
#[derive(Debug, Clone, Default)]
pub struct AssetRef {
    pub id: Option<Uuid>,
    pub entity_type: Option<String>,
    /// The asset's own workspace slug (`self.workspace.slug` in `asset_url`);
    /// the source row can be NULL.
    pub workspace_slug: Option<String>,
    pub project_id: Option<Uuid>,
    pub issue_id: Option<Uuid>,
}

/// Number of columns [`AssetRef::from_row`] reads (the asset select lists in
/// each domain's `queries.rs`, in order).
pub const ASSET_COLUMNS: usize = 5;

impl AssetRef {
    /// Read the five columns written by the asset joins in `queries.rs`.
    pub fn from_row(
        row: &tokio_postgres::Row,
        start: usize,
    ) -> Result<Self, tokio_postgres::Error> {
        Ok(Self {
            id: row.try_get(start)?,
            entity_type: row.try_get(start + 1)?,
            workspace_slug: row.try_get(start + 2)?,
            project_id: row.try_get(start + 3)?,
            issue_id: row.try_get(start + 4)?,
        })
    }

    /// Whether the foreign key pointed at a row.
    pub fn is_some(&self) -> bool {
        self.id.is_some()
    }

    /// `FileAsset.asset_url` for this row; `Ok(None)` when the join produced no
    /// row (`id` is NULL), which the properties read as "no asset".
    pub fn asset_url(&self) -> Result<Option<String>, ViewError> {
        let Some(id) = self.id else {
            return Ok(None);
        };
        file_asset_url(
            id,
            self.entity_type.as_deref(),
            self.workspace_slug.as_deref(),
            self.project_id,
            self.issue_id,
        )
    }
}

/// `FileAsset.asset_url` (`plane/db/models/asset.py:83-104`).
///
/// `workspace_slug` is `self.workspace.slug`; `None` raises Python's
/// `AttributeError` for the branches that read it and is ignored by the static
/// branch (`asset.py:90-91`).
///
/// # Errors
///
/// [`ViewError::Unknown`] with the Python `AttributeError` for a non-static
/// entity type whose asset has no workspace.
pub fn file_asset_url(
    id: Uuid,
    entity_type: Option<&str>,
    workspace_slug: Option<&str>,
    project_id: Option<Uuid>,
    issue_id: Option<Uuid>,
) -> Result<Option<String>, ViewError> {
    let slug = |attribute: &str| -> Result<&str, ViewError> {
        // `AttributeError: 'NoneType' object has no attribute 'workspace'`
        workspace_slug.map_or_else(|| Err(attribute_error(attribute)), Ok)
    };
    Ok(match entity_type {
        Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
            Some(format!("/api/assets/v2/static/{id}/"))
        }
        Some("ISSUE_ATTACHMENT") => Some(format!(
            "/api/assets/v2/workspaces/{}/projects/{}/issues/{}/attachments/{id}/",
            slug("workspace")?,
            python_uuid(project_id),
            python_uuid(issue_id),
        )),
        Some(
            "ISSUE_DESCRIPTION"
            | "COMMENT_DESCRIPTION"
            | "PAGE_DESCRIPTION"
            | "DRAFT_ISSUE_DESCRIPTION",
        ) => Some(format!(
            "/api/assets/v2/workspaces/{}/projects/{}/{id}/",
            slug("workspace")?,
            python_uuid(project_id),
        )),
        _ => None,
    })
}

/// `User.avatar_url` (`plane/db/models/user.py:143-151`).
pub fn user_avatar_url(avatar: &str, asset: &AssetRef) -> Result<Option<String>, ViewError> {
    if asset.is_some() {
        return asset.asset_url();
    }
    Ok(if avatar.is_empty() {
        None
    } else {
        Some(avatar.to_string())
    })
}

/// `User.cover_image_url` (`plane/db/models/user.py:153-161`).
pub fn user_cover_image_url(
    cover_image: Option<&str>,
    asset: &AssetRef,
) -> Result<Option<String>, ViewError> {
    if asset.is_some() {
        return asset.asset_url();
    }
    Ok(cover_image
        .filter(|value| !value.is_empty())
        .map(str::to_string))
}

/// `Workspace.logo_url` (`plane/db/models/workspace.py:145-154`).
pub fn workspace_logo_url(
    logo: Option<&str>,
    asset: &AssetRef,
) -> Result<Option<String>, ViewError> {
    if asset.is_some() {
        return asset.asset_url();
    }
    Ok(logo.filter(|logo| !logo.is_empty()).map(str::to_string))
}

/// `Project.cover_image_url` (`plane/db/models/project.py:129-139`).
pub fn project_cover_image_url(
    cover_image: Option<&str>,
    asset: &AssetRef,
) -> Result<Option<String>, ViewError> {
    if asset.is_some() {
        return asset.asset_url();
    }
    Ok(cover_image
        .filter(|value| !value.is_empty())
        .map(str::to_string))
}

/// Python's `str(None)` inside an f-string: `"None"`.
fn python_uuid(value: Option<Uuid>) -> String {
    match value {
        Some(value) => value.to_string(),
        None => "None".to_string(),
    }
}

fn attribute_error(attribute: &str) -> ViewError {
    // `AttributeError: 'NoneType' object has no attribute '<attribute>'`, which
    // Plane's family handler turns into its 500 body.
    ViewError::Unknown(format!(
        "AttributeError: 'NoneType' object has no attribute '{attribute}'"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: Uuid = Uuid::nil();

    #[test]
    fn static_entity_types_ignore_the_workspace() {
        for entity_type in [
            "WORKSPACE_LOGO",
            "USER_AVATAR",
            "USER_COVER",
            "PROJECT_COVER",
        ] {
            assert_eq!(
                file_asset_url(ID, Some(entity_type), None, None, None).expect("url"),
                Some("/api/assets/v2/static/00000000-0000-0000-0000-000000000000/".to_string())
            );
        }
    }

    #[test]
    fn attachment_and_description_urls_match_the_property() {
        let project = Uuid::parse_str("22222222-2222-4222-8222-222222222222").expect("uuid");
        let issue = Uuid::parse_str("33333333-3333-4333-8333-333333333333").expect("uuid");
        assert_eq!(
            file_asset_url(ID, Some("ISSUE_ATTACHMENT"), Some("ws"), Some(project), Some(issue))
                .expect("url"),
            Some(
                "/api/assets/v2/workspaces/ws/projects/22222222-2222-4222-8222-222222222222/issues/33333333-3333-4333-8333-333333333333/attachments/00000000-0000-0000-0000-000000000000/"
                    .to_string()
            )
        );
        // The description branch ends in `/`, as the Python f-string does.
        for entity_type in [
            "ISSUE_DESCRIPTION",
            "COMMENT_DESCRIPTION",
            "PAGE_DESCRIPTION",
            "DRAFT_ISSUE_DESCRIPTION",
        ] {
            assert_eq!(
                file_asset_url(ID, Some(entity_type), Some("ws"), Some(project), None)
                    .expect("url"),
                Some(
                    "/api/assets/v2/workspaces/ws/projects/22222222-2222-4222-8222-222222222222/00000000-0000-0000-0000-000000000000/"
                        .to_string()
                )
            );
        }
    }

    #[test]
    fn missing_ids_render_like_python_f_strings() {
        assert_eq!(
            file_asset_url(ID, Some("ISSUE_ATTACHMENT"), Some("ws"), None, None).expect("url"),
            Some(
                "/api/assets/v2/workspaces/ws/projects/None/issues/None/attachments/00000000-0000-0000-0000-000000000000/"
                    .to_string()
            )
        );
    }

    #[test]
    fn a_workspace_less_non_static_asset_is_the_python_attribute_error() {
        for entity_type in ["ISSUE_ATTACHMENT", "ISSUE_DESCRIPTION"] {
            let error = file_asset_url(ID, Some(entity_type), None, None, None)
                .expect_err("AttributeError");
            assert!(
                matches!(error, ViewError::Unknown(ref message) if message.contains("AttributeError")),
                "{error:?}"
            );
        }
    }

    #[test]
    fn unknown_entity_types_have_no_url() {
        assert_eq!(
            file_asset_url(ID, Some("SOMETHING_ELSE"), Some("ws"), None, None).expect("url"),
            None
        );
        assert_eq!(
            file_asset_url(ID, None, Some("ws"), None, None).expect("url"),
            None
        );
        // `DRAFT_ISSUE_ATTACHMENT` is in the model's choices but not in the
        // property's branches.
        assert_eq!(
            file_asset_url(ID, Some("DRAFT_ISSUE_ATTACHMENT"), Some("ws"), None, None)
                .expect("url"),
            None
        );
    }

    #[test]
    fn asset_ref_swallows_a_missing_row() {
        assert_eq!(AssetRef::default().asset_url().expect("url"), None);
        let asset = AssetRef {
            id: Some(ID),
            entity_type: Some("USER_AVATAR".to_string()),
            ..AssetRef::default()
        };
        assert_eq!(
            asset.asset_url().expect("url"),
            Some("/api/assets/v2/static/00000000-0000-0000-0000-000000000000/".to_string())
        );
    }

    #[test]
    fn the_column_is_only_the_fallback() {
        let none = AssetRef::default();
        assert_eq!(
            user_avatar_url("https://x.test/a.png", &none).expect("url"),
            Some("https://x.test/a.png".to_string())
        );
        assert_eq!(user_avatar_url("", &none).expect("url"), None);
        assert_eq!(
            user_cover_image_url(Some("https://x.test/c.png"), &none).expect("url"),
            Some("https://x.test/c.png".to_string())
        );
        assert_eq!(user_cover_image_url(Some(""), &none).expect("url"), None);
        assert_eq!(user_cover_image_url(None, &none).expect("url"), None);
        assert_eq!(
            workspace_logo_url(Some("https://x.test/l.png"), &none).expect("url"),
            Some("https://x.test/l.png".to_string())
        );
        assert_eq!(workspace_logo_url(Some(""), &none).expect("url"), None);
        assert_eq!(
            project_cover_image_url(Some("https://x.test/p.png"), &none).expect("url"),
            Some("https://x.test/p.png".to_string())
        );
        assert_eq!(project_cover_image_url(Some(""), &none).expect("url"), None);

        // An asset row wins over a non-empty column.
        let asset = AssetRef {
            id: Some(ID),
            entity_type: Some("USER_AVATAR".to_string()),
            ..AssetRef::default()
        };
        assert_eq!(
            user_avatar_url("https://x.test/a.png", &asset).expect("url"),
            Some("/api/assets/v2/static/00000000-0000-0000-0000-000000000000/".to_string())
        );
    }
}
