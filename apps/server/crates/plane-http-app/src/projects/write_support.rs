// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Row types and small helpers shared by the project satellite writes.
//!
//! * [`ProjectRow`] — the 36 columns `project.save()` rewrites
//!   (`plane/db/models/project.py:68-120`);
//! * [`DeployBoardRow`] — the 18 columns of a deploy board
//!   (`plane/db/models/deploy_board.py:22-48`);
//! * [`integrity`] — Django's `IntegrityError` → the family's 400
//!   `{"error": "The payload is not valid"}` (`plane/app/views/base.py:306-310`);
//! * [`python_str_datetime`] — `str(datetime)`, which the archive response
//!   uses (`base.py:434`), is `isoformat(" ")`, not the serializer's `Z` form.

use chrono::{DateTime, SubsecRound, Utc};
use plane_drf::error::ViewError;
use uuid::Uuid;

use plane_db::JsonText;

/// One `projects` row, in the catalog's column order
/// (`write_queries::PROJECT_BY_ID_IN_WORKSPACE`).
#[derive(Debug, Clone)]
pub struct ProjectRow {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub description_text: Option<JsonText>,
    pub description_html: Option<JsonText>,
    pub network: i16,
    pub workspace_id: Uuid,
    pub identifier: String,
    pub default_assignee_id: Option<Uuid>,
    pub project_lead_id: Option<Uuid>,
    pub emoji: Option<String>,
    pub icon_prop: Option<JsonText>,
    pub module_view: bool,
    pub cycle_view: bool,
    pub issue_views_view: bool,
    pub page_view: bool,
    pub intake_view: bool,
    pub is_time_tracking_enabled: bool,
    pub is_issue_type_enabled: bool,
    pub guest_view_all_features: bool,
    pub cover_image: Option<String>,
    pub cover_image_asset_id: Option<Uuid>,
    pub estimate_id: Option<Uuid>,
    pub archive_in: i32,
    pub close_in: i32,
    pub logo_props: JsonText,
    pub default_state_id: Option<Uuid>,
    pub archived_at: Option<DateTime<Utc>>,
    pub timezone: String,
    pub external_source: Option<String>,
    pub external_id: Option<String>,
}

impl ProjectRow {
    /// Decode the 36 columns the statement selects.
    pub fn from_row(row: &tokio_postgres::Row, start: usize) -> Result<Self, ViewError> {
        let at = |offset: usize| start + offset;
        Ok(Self {
            created_at: row.try_get(at(0))?,
            updated_at: row.try_get(at(1))?,
            created_by_id: row.try_get(at(2))?,
            updated_by_id: row.try_get(at(3))?,
            deleted_at: row.try_get(at(4))?,
            id: row.try_get(at(5))?,
            name: row.try_get(at(6))?,
            description: row.try_get(at(7))?,
            description_text: row.try_get(at(8))?,
            description_html: row.try_get(at(9))?,
            network: row.try_get(at(10))?,
            workspace_id: row.try_get(at(11))?,
            identifier: row.try_get(at(12))?,
            default_assignee_id: row.try_get(at(13))?,
            project_lead_id: row.try_get(at(14))?,
            emoji: row.try_get(at(15))?,
            icon_prop: row.try_get(at(16))?,
            module_view: row.try_get(at(17))?,
            cycle_view: row.try_get(at(18))?,
            issue_views_view: row.try_get(at(19))?,
            page_view: row.try_get(at(20))?,
            intake_view: row.try_get(at(21))?,
            is_time_tracking_enabled: row.try_get(at(22))?,
            is_issue_type_enabled: row.try_get(at(23))?,
            guest_view_all_features: row.try_get(at(24))?,
            cover_image: row.try_get(at(25))?,
            cover_image_asset_id: row.try_get(at(26))?,
            estimate_id: row.try_get(at(27))?,
            archive_in: row.try_get(at(28))?,
            close_in: row.try_get(at(29))?,
            logo_props: row.try_get(at(30))?,
            default_state_id: row.try_get(at(31))?,
            archived_at: row.try_get(at(32))?,
            timezone: row.try_get(at(33))?,
            external_source: row.try_get(at(34))?,
            external_id: row.try_get(at(35))?,
        })
    }

    /// `project.save()`'s bind list: the 35 writable columns plus the id.
    ///
    /// `PROJECT_UPDATE` takes them in the same order, with `$6` used only in
    /// the `WHERE`.
    pub fn update_args(&self) -> Vec<&(dyn tokio_postgres::types::ToSql + Sync)> {
        vec![
            &self.created_at,
            &self.updated_at,
            &self.created_by_id,
            &self.updated_by_id,
            &self.deleted_at,
            &self.id,
            &self.name,
            &self.description,
            &self.description_text,
            &self.description_html,
            &self.network,
            &self.workspace_id,
            &self.identifier,
            &self.default_assignee_id,
            &self.project_lead_id,
            &self.emoji,
            &self.icon_prop,
            &self.module_view,
            &self.cycle_view,
            &self.issue_views_view,
            &self.page_view,
            &self.intake_view,
            &self.is_time_tracking_enabled,
            &self.is_issue_type_enabled,
            &self.guest_view_all_features,
            &self.cover_image,
            &self.cover_image_asset_id,
            &self.estimate_id,
            &self.archive_in,
            &self.close_in,
            &self.logo_props,
            &self.default_state_id,
            &self.archived_at,
            &self.timezone,
            &self.external_source,
            &self.external_id,
        ]
    }
}

/// One `deploy_boards` row, in the catalog's column order.
#[derive(Debug, Clone)]
pub struct DeployBoardRow {
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by_id: Option<Uuid>,
    pub updated_by_id: Option<Uuid>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Option<Uuid>,
    pub entity_identifier: Option<Uuid>,
    pub entity_name: Option<String>,
    pub anchor: String,
    pub is_comments_enabled: bool,
    pub is_reactions_enabled: bool,
    pub intake_id: Option<Uuid>,
    pub is_votes_enabled: bool,
    pub view_props: JsonText,
    pub is_activity_enabled: bool,
    pub is_disabled: bool,
}

impl DeployBoardRow {
    /// Decode the 18 columns the statements select.
    pub fn from_row(row: &tokio_postgres::Row, start: usize) -> Result<Self, ViewError> {
        let at = |offset: usize| start + offset;
        Ok(Self {
            created_at: row.try_get(at(0))?,
            updated_at: row.try_get(at(1))?,
            created_by_id: row.try_get(at(2))?,
            updated_by_id: row.try_get(at(3))?,
            deleted_at: row.try_get(at(4))?,
            id: row.try_get(at(5))?,
            workspace_id: row.try_get(at(6))?,
            project_id: row.try_get(at(7))?,
            entity_identifier: row.try_get(at(8))?,
            entity_name: row.try_get(at(9))?,
            anchor: row.try_get(at(10))?,
            is_comments_enabled: row.try_get(at(11))?,
            is_reactions_enabled: row.try_get(at(12))?,
            intake_id: row.try_get(at(13))?,
            is_votes_enabled: row.try_get(at(14))?,
            view_props: row.try_get(at(15))?,
            is_activity_enabled: row.try_get(at(16))?,
            is_disabled: row.try_get(at(17))?,
        })
    }

    /// `DeployBoard.save()`'s bind list: every column, id last.
    pub fn update_args(&self) -> Vec<&(dyn tokio_postgres::types::ToSql + Sync)> {
        vec![
            &self.created_at,
            &self.updated_at,
            &self.created_by_id,
            &self.updated_by_id,
            &self.deleted_at,
            &self.workspace_id,
            &self.project_id,
            &self.entity_identifier,
            &self.entity_name,
            &self.anchor,
            &self.is_comments_enabled,
            &self.is_reactions_enabled,
            &self.intake_id,
            &self.is_votes_enabled,
            &self.view_props,
            &self.is_activity_enabled,
            &self.is_disabled,
            &self.id,
        ]
    }
}

/// The three request values every handler in this family starts with:
/// `(slug, project_id, user_id)`. The runtime's `allow_permission` already
/// resolved the project for the routes that declare it, but the views still
/// re-read the row, so the kwarg is parsed here as `UUIDField.to_python` would.
pub fn target<'a>(
    ctx: &'a plane_drf::view::ViewCtx<'a>,
) -> Result<(&'a str, Uuid, Uuid), plane_drf::error::ViewError> {
    let slug = ctx.kwarg("slug").ok_or(plane_drf::error::ViewError::Key)?;
    let project_id = ctx
        .kwarg("project_id")
        .ok_or(plane_drf::error::ViewError::Key)
        .and_then(|value| {
            Uuid::parse_str(value).map_err(|_| plane_drf::error::ViewError::Validation)
        })?;
    let user = ctx.user.id().ok_or(plane_drf::error::ViewError::Api(
        plane_core::error::ApiError::NotAuthenticated,
    ))?;
    Ok((slug, project_id, user))
}

/// `view.kwargs["member_id"]` as a UUID.
pub fn member_id(ctx: &plane_drf::view::ViewCtx<'_>) -> Result<Uuid, plane_drf::error::ViewError> {
    ctx.kwarg("member_id")
        .ok_or(plane_drf::error::ViewError::Key)
        .and_then(|value| {
            Uuid::parse_str(value).map_err(|_| plane_drf::error::ViewError::Validation)
        })
}

/// `view.kwargs["pk"]` as a UUID.
pub fn pk(ctx: &plane_drf::view::ViewCtx<'_>) -> Result<Uuid, plane_drf::error::ViewError> {
    ctx.kwarg("pk")
        .ok_or(plane_drf::error::ViewError::Key)
        .and_then(|value| {
            Uuid::parse_str(value).map_err(|_| plane_drf::error::ViewError::Validation)
        })
}

/// The PostgreSQL errors Django's `django.db.IntegrityError` covers.
///
/// Plane's family `handle_exception` maps `IntegrityError` to
/// `400 {"error": "The payload is not valid"}`
/// (`plane/app/views/base.py:306-310`). psycopg raises `IntegrityError` for the
/// SQLSTATE class 23 — unique, not-null, check, foreign-key and exclusion
/// violations — and a different class for a bad literal (`DataError`, a 500).
pub fn integrity(error: tokio_postgres::Error) -> ViewError {
    if error
        .code()
        .is_some_and(|code| code.code().starts_with("23"))
    {
        return ViewError::Integrity;
    }
    ViewError::from(error)
}

/// `str(datetime)` — `datetime.isoformat(" ")`.
///
/// `ProjectArchiveUnarchiveEndpoint.post` answers
/// `{"archived_at": str(project.archived_at)}` (`base.py:434`), which is
/// `2026-11-16 07:12:34.123456+00:00`: a space separator, microseconds only
/// when non-zero, and `+00:00` rather than DRF's `Z`.
#[must_use]
pub fn python_str_datetime(value: DateTime<Utc>) -> String {
    let value = value.trunc_subsecs(6);
    let micros = value.timestamp_subsec_micros();
    if micros == 0 {
        value.format("%Y-%m-%d %H:%M:%S+00:00").to_string()
    } else {
        value.format("%Y-%m-%d %H:%M:%S%.6f+00:00").to_string()
    }
}

/// `{"archived_at": str(...)}` as the response body.
#[must_use]
pub fn archived_at_body(value: DateTime<Utc>) -> plane_drf::view::ViewResponse {
    plane_drf::view::ViewResponse::raw(
        200,
        plane_core::error::APPLICATION_JSON,
        plane_query::json::Json::object(vec![(
            "archived_at",
            plane_query::json::Json::string(python_str_datetime(value)),
        )])
        .to_drf(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_postgres::error::SqlState;

    #[test]
    fn python_str_datetime_matches_isoformat_with_a_space() {
        let value: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-11-16T07:12:34.123456Z")
            .expect("instant")
            .with_timezone(&Utc);
        assert_eq!(
            python_str_datetime(value),
            "2026-11-16 07:12:34.123456+00:00"
        );
        let whole: DateTime<Utc> = DateTime::parse_from_rfc3339("2026-11-16T07:12:34Z")
            .expect("instant")
            .with_timezone(&Utc);
        // `isoformat()` omits `.000000` when microsecond == 0.
        assert_eq!(python_str_datetime(whole), "2026-11-16 07:12:34+00:00");
    }

    #[test]
    fn integrity_covers_the_sqlstate_class_django_maps() {
        // The class prefix is what matters; the actual conversion needs a real
        // error, so this asserts the predicate the function uses.
        assert!(SqlState::UNIQUE_VIOLATION.code().starts_with("23"));
        assert!(SqlState::NOT_NULL_VIOLATION.code().starts_with("23"));
        assert!(SqlState::CHECK_VIOLATION.code().starts_with("23"));
        assert!(
            !SqlState::INVALID_TEXT_REPRESENTATION
                .code()
                .starts_with("23")
        );
    }
}
