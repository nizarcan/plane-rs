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

/// Number of columns [`AssetRef::from_row`] reads.
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
/// views read (`per_page`, `cursor`, `month`).
///
/// Surrounding whitespace, an optional sign and single underscores between
/// digits are accepted, as CPython's `int()` does. Non-ASCII digits are not
/// reproduced (they would need Unicode decimal tables); they are an error here
/// where Python would parse them. This is recorded as a known edge in the
/// group's report.
pub fn python_int(text: &str) -> Option<i64> {
    let trimmed = text.trim();
    let (sign, digits) = match trimmed.as_bytes().first() {
        Some(b'-') => (-1i64, &trimmed[1..]),
        Some(b'+') => (1, &trimmed[1..]),
        _ => (1, trimmed),
    };
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') {
        return None;
    }
    let mut value: i64 = 0;
    let mut seen_digit = false;
    let mut previous_underscore = false;
    for ch in digits.chars() {
        if ch == '_' {
            if previous_underscore || !seen_digit {
                return None;
            }
            previous_underscore = true;
            continue;
        }
        if !ch.is_ascii_digit() {
            return None;
        }
        let digit = ch.to_digit(10)?;
        value = value.checked_mul(10)?.checked_add(i64::from(digit))?;
        previous_underscore = false;
        seen_digit = true;
    }
    if previous_underscore {
        return None;
    }
    value.checked_mul(sign)
}

/// `BasePaginator.get_per_page` (`plane/utils/paginator.py:642-657`).
///
/// The Python reads `request.GET.get("per_page", default)` and calls `int()` on
/// whatever it gets: an empty value is a `ValueError`, not the default.
pub fn per_page(
    query: &plane_drf::QueryDict,
    default_per_page: u32,
    max_per_page: u32,
) -> Result<u32, ViewError> {
    let parsed = match query.get("per_page") {
        Some(raw) => python_int(raw).ok_or_else(|| {
            ViewError::Api(ApiError::ParseError(
                "Invalid per_page parameter.".to_string(),
            ))
        })?,
        None => i64::from(default_per_page),
    };
    let effective_max = max_per_page.max(default_per_page);
    if parsed <= 0 {
        return Err(ViewError::Api(ApiError::ParseError(
            "Invalid per_page value. Must be greater than zero.".to_string(),
        )));
    }
    if parsed > i64::from(effective_max) {
        return Err(ViewError::Api(ApiError::ParseError(format!(
            "Invalid per_page value. Cannot exceed {effective_max}."
        ))));
    }
    Ok(parsed as u32)
}

/// `BasePaginator.paginate`'s cursor step (`plane/utils/paginator.py:672-681`).
pub fn page_query(
    query: &plane_drf::QueryDict,
    default_per_page: u32,
    max_per_page: u32,
) -> Result<plane_query::PageQuery, ViewError> {
    let per_page = per_page(query, default_per_page, max_per_page)?;
    let raw_cursor = query.get("cursor").map(str::to_owned);
    let default = format!("{per_page}:0:0");
    let cursor = plane_query::Cursor::from_string(raw_cursor.as_deref().unwrap_or(&default))
        .map_err(|_| {
            ViewError::Api(ApiError::ParseError(
                "Invalid cursor parameter.".to_string(),
            ))
        })?;
    // `0 <= input_cursor.value <= effective_max_per_page` and a non-negative
    // offset (`paginator.py:679-681`).
    let effective_max = f64::from(max_per_page.max(default_per_page));
    if !(0.0..=effective_max).contains(&cursor.value) || cursor.offset < 0 {
        return Err(ViewError::Api(ApiError::ParseError(
            "Invalid cursor parameter.".to_string(),
        )));
    }
    Ok(plane_query::PageQuery {
        per_page,
        cursor,
        raw_cursor,
    })
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
        assert_eq!(python_int(""), None);
        assert_eq!(python_int("1.0"), None);
        assert_eq!(python_int("_1"), None);
        assert_eq!(python_int("1_"), None);
        assert_eq!(python_int("1__0"), None);
        assert_eq!(python_int("abc"), None);
        assert_eq!(python_int("0x10"), None);
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
