// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Small helpers the `users-me` handlers share.
//!
//! They all reproduce a Python rule that is not a serializer: model
//! properties (`FileAsset.asset_url`, `Workspace.logo_url`, …), CPython's
//! `int()` in the paginator, and the paginator's cursor parsing.
//!
//! Nothing here is domain-neutral enough for `crate::common`: a second group
//! that needs the same rule should move it there (`HANDLERS.md` §7), not copy
//! it.

use chrono::NaiveDate;
use plane_core::ApiError;
use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;
use uuid::Uuid;

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
    /// The asset's own workspace slug (`self.workspace.slug` in
    /// `asset_url`); the source row can be NULL.
    pub workspace_slug: Option<String>,
    pub project_id: Option<Uuid>,
    pub issue_id: Option<Uuid>,
}

/// Number of columns [`AssetRef::from_row`] reads (the `ASSET_BY_ID` select
/// list, in order).
pub const ASSET_COLUMNS: usize = 5;

impl AssetRef {
    /// Read the five columns written by the joins in `queries.rs`.
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

    /// `FileAsset.asset_url` (`plane/db/models/asset.py:83-104`).
    ///
    /// Returns `Ok(None)` for an entity type the property does not know.
    /// A non-static entity type whose `workspace` is NULL raises
    /// `AttributeError` in Python (`self.workspace.slug`); that becomes the
    /// same 500.
    pub fn asset_url(&self) -> Result<Option<String>, ViewError> {
        let Some(id) = self.id else {
            return Ok(None);
        };
        match self.entity_type.as_deref() {
            Some("WORKSPACE_LOGO" | "USER_AVATAR" | "USER_COVER" | "PROJECT_COVER") => {
                Ok(Some(format!("/api/assets/v2/static/{id}/")))
            }
            Some("ISSUE_ATTACHMENT") => {
                let Some(slug) = self.workspace_slug.as_deref() else {
                    return Err(attribute_error("workspace"));
                };
                Ok(Some(format!(
                    "/api/assets/v2/workspaces/{slug}/projects/{}/issues/{}/attachments/{id}/",
                    python_uuid(self.project_id),
                    python_uuid(self.issue_id),
                )))
            }
            Some(
                "ISSUE_DESCRIPTION"
                | "COMMENT_DESCRIPTION"
                | "PAGE_DESCRIPTION"
                | "DRAFT_ISSUE_DESCRIPTION",
            ) => {
                let Some(slug) = self.workspace_slug.as_deref() else {
                    return Err(attribute_error("workspace"));
                };
                Ok(Some(format!(
                    "/api/assets/v2/workspaces/{slug}/projects/{}/{id}/",
                    python_uuid(self.project_id),
                )))
            }
            _ => Ok(None),
        }
    }
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

/// `User.avatar_url` (`plane/db/models/user.py:149-157`).
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

/// `User.cover_image_url` (`plane/db/models/user.py:159-167`).
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

/// Python's `int(text)` for the handful of integer query parameters these
/// views read (`month`).
///
/// Delegates to `plane_core::py::int`, CPython's `int()`: surrounding
/// whitespace, an optional sign, single underscores between digits and Unicode
/// decimal digits are accepted.
pub fn python_int(text: &str) -> Option<i64> {
    plane_core::py::int(text).ok()
}

/// `BasePaginator.paginate`'s cursor step (`plane/utils/paginator.py:672-681`),
/// including `get_per_page` (`paginator.py:642-657`).
///
/// The parsing itself lives in `plane_query::PageQuery`, which is the same code
/// the issue lists run. In particular a present-but-empty `?per_page=` is the
/// `ValueError` Django's `int()` raises, not the default.
pub fn page_query(
    query: &plane_drf::QueryDict,
    default_per_page: u32,
    max_per_page: u32,
) -> Result<plane_query::PageQuery, ViewError> {
    plane_query::PageQuery::parse_with(query, default_per_page, max_per_page, None, None).map_err(
        |error| match error {
            plane_query::QueryError::Api(error) => {
                ViewError::Api(ApiError::ParseError(error.message))
            }
            other => ViewError::Unknown(other.to_string()),
        },
    )
}

/// `date.today()` — the process's local date, not UTC.
pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

/// `date.today() + relativedelta(months=-months)`.
///
/// `relativedelta` clamps the day to the last day of the target month, which
/// `chrono`'s `checked_sub_months` also does.
pub fn months_ago(months: u32) -> NaiveDate {
    let today = today();
    today
        .checked_sub_months(chrono::Months::new(months))
        .unwrap_or(today)
}

/// The authenticated user's id. Every route here is behind `IsAuthenticated`,
/// so a missing user is a bug in the runtime rather than a request outcome.
pub fn user_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user.id().ok_or(ViewError::Key)
}

/// The email `request.user.email` the settings/invitation views filter on.
pub fn user_email(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    ctx.user
        .user()
        .map(|user| user.email.clone())
        .ok_or(ViewError::Key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_int_matches_the_common_cases() {
        assert_eq!(python_int("42"), Some(42));
        assert_eq!(python_int(" 42 "), Some(42));
        assert_eq!(python_int("+42"), Some(42));
        assert_eq!(python_int("-7"), Some(-7));
        assert_eq!(python_int("1_000"), Some(1000));
        // CPython's `int()` accepts Unicode decimal digits (`int("٠") == 0`).
        assert_eq!(python_int("١٢"), Some(12));
        assert_eq!(python_int(""), None);
        assert_eq!(python_int("1.0"), None);
        assert_eq!(python_int("_1"), None);
        assert_eq!(python_int("1_"), None);
        assert_eq!(python_int("1__0"), None);
        assert_eq!(python_int("abc"), None);
        assert_eq!(python_int("0x10"), None);
    }

    #[test]
    fn page_query_rejects_a_present_but_empty_per_page() {
        // `int(request.GET.get("per_page", 1000))` (`paginator.py:645`): absent
        // is the default, `?per_page=` is a `ValueError` → `ParseError`.
        let absent = plane_drf::QueryDict::parse("", None).expect("query");
        let page = page_query(&absent, 1000, 1000).expect("page");
        assert_eq!(page.per_page, 1000);

        let empty = plane_drf::QueryDict::parse("per_page=", None).expect("query");
        assert_eq!(
            page_query(&empty, 1000, 1000).expect_err("error"),
            ViewError::Api(ApiError::ParseError(
                "Invalid per_page parameter.".to_string()
            ))
        );
    }

    #[test]
    fn asset_url_matches_the_model_property() {
        let static_asset = AssetRef {
            id: Some(Uuid::nil()),
            entity_type: Some("USER_AVATAR".to_string()),
            ..AssetRef::default()
        };
        assert_eq!(
            static_asset.asset_url().expect("url"),
            Some("/api/assets/v2/static/00000000-0000-0000-0000-000000000000/".to_string())
        );

        let unknown = AssetRef {
            id: Some(Uuid::nil()),
            entity_type: Some("SOMETHING_ELSE".to_string()),
            ..AssetRef::default()
        };
        assert_eq!(unknown.asset_url().expect("url"), None);

        let attachment = AssetRef {
            id: Some(Uuid::nil()),
            entity_type: Some("ISSUE_ATTACHMENT".to_string()),
            workspace_slug: Some("ws".to_string()),
            project_id: None,
            issue_id: None,
        };
        assert_eq!(
            attachment.asset_url().expect("url"),
            Some(
                "/api/assets/v2/workspaces/ws/projects/None/issues/None/attachments/\
                 00000000-0000-0000-0000-000000000000/"
                    .to_string()
            )
        );
    }

    #[test]
    fn avatar_falls_back_to_the_column_only_without_an_asset() {
        let none = AssetRef::default();
        assert_eq!(
            user_avatar_url("https://x.test/a.png", &none).expect("url"),
            Some("https://x.test/a.png".to_string())
        );
        assert_eq!(user_avatar_url("", &none).expect("url"), None);
        let asset = AssetRef {
            id: Some(Uuid::nil()),
            entity_type: Some("USER_AVATAR".to_string()),
            ..AssetRef::default()
        };
        assert_eq!(
            user_avatar_url("https://x.test/a.png", &asset).expect("url"),
            Some("/api/assets/v2/static/00000000-0000-0000-0000-000000000000/".to_string())
        );
    }
}
