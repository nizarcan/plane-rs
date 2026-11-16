// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Shared helpers for the issue-core write routes (`plans/WORK_ITEMS.md` set
//! A: orders 64, 67, 71, 72, 92 and 96).
//!
//! The module holds the pieces every handler in [`super::write_issue`] needs
//! and that nothing outside this set does:
//!
//! * the request body as the Python `request.data` object (`dict`/`QueryDict`)
//!   that the activity and webhook tasks serialise, with `json.dumps(...,
//!   cls=DjangoJSONEncoder)` semantics;
//! * `writable_data(serializer, request.data)` (`plane/utils/serializers.py:6-12`),
//!   keeping request order and unknown keys, dropping the read-only fields
//!   (and `is_draft`, read-only on PATCH);
//! * `IssueSerializer(issue).data` for the archive `current_instance` strings,
//!   through the shared [`plane_query::json::Json`] document and the Django
//!   dialect (the serializer's datetimes are already DRF strings);
//! * the `base_host(request, is_app=True)` origin;
//! * the serializer's extra `validate()` queries (assignee/label filtering and
//!   the project checks).
//!
//! ## `IssueSerializer(issue).data` in the archive views
//!
//! The archive and bulk-archive views fetch issues with `Issue.objects` (or
//! `issue_objects`) and **no annotations**, so the seven annotation-backed
//! fields (`cycle_id`, `module_ids`, `label_ids`, `assignee_ids`,
//! `sub_issues_count`, `attachment_count`, `link_count`) are declared
//! `required=False` and DRF raises `SkipField` for a missing attribute. They
//! therefore do not appear in the dict. [`issue_serializer_json`] reproduces
//! exactly that 18-key projection.

use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;
use plane_db::models::issue::Issue;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::ViewCtx;
use plane_query::json::Json as Doc;
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::generated::app_issue::IssueSerializer;
use plane_serializers::{FieldValue, RenderCtx};
use uuid::Uuid;

use crate::write::Serializer;

/// Field names `serializer.fields[key].read_only` is true for on create
/// (`write/generated/issue_create_serializer.rs`'s `READ_ONLY_FIELDS`).
const READ_ONLY_ON_CREATE: [&str; 16] = [
    "id",
    "project_id",
    "workspace_id",
    "created_at",
    "updated_at",
    "deleted_at",
    "description_binary",
    "sequence_id",
    "completed_at",
    "archived_at",
    "created_by",
    "updated_by",
    "project",
    "workspace",
    "assignees",
    "labels",
];

/// `IssueCreateSerializer.get_fields` makes `is_draft` read-only when
/// `self.instance is not None` (`plane/app/serializers/issue.py:126-131`).
pub(crate) fn mark_serializer_patch_fields(serializer: &mut Serializer<'_>) {
    serializer.skip_field("is_draft");
}

/// `request.data` as a Python `dict` for the tasks that serialise it inline or
/// with `json.dumps`.
///
/// A JSON body is returned as parsed. A form body is a `QueryDict`, a `dict`
/// subclass whose iteration yields one (last) value per key, so it becomes a
/// string-valued object in first-key order.
///
/// # Errors
///
/// [`ViewError::Unknown`] when a JSON body is not an object (where Python's
/// `json.dumps(request.data)` would still work for a list, but the call sites
/// index it as a mapping first).
pub(crate) fn request_object(ctx: &ViewCtx<'_>) -> Result<Json, ViewError> {
    match ctx.data()? {
        Data::Json(value) => Ok(value.clone()),
        Data::Form(query) => {
            let mut dict = PyDict::with_capacity(query.len());
            let mut seen = std::collections::BTreeSet::new();
            for key in query.keys() {
                if !seen.insert(key) {
                    continue;
                }
                if let Some(value) = query.get(key) {
                    dict.insert(PyStr::new(key), Json::Str(PyStr::new(value)));
                }
            }
            Ok(Json::Dict(dict))
        }
    }
}

/// `request.data.get(key, default)` with the body shapes the bulk endpoints
/// accept.
///
/// A JSON body must be an object (a list/scalar would raise `AttributeError:
/// 'list' object has no attribute 'get'` → 500); a form body uses
/// `QueryDict.get`. `None` means "absent or null", exactly like Python's
/// `get` without a default returning `None`; the caller distinguishes the two
/// only where Python does.
///
/// # Errors
///
/// [`ViewError::Unknown`] for a non-object JSON body.
pub(crate) fn body_get(ctx: &ViewCtx<'_>, key: &str) -> Result<Option<Json>, ViewError> {
    match ctx.data()? {
        Data::Json(Json::Dict(object)) => Ok(object.get(key).cloned()),
        Data::Json(_) => Err(ViewError::Unknown(
            "AttributeError: request.data is not a mapping".to_owned(),
        )),
        Data::Form(query) => Ok(query.get(key).map(|value| Json::Str(PyStr::new(value)))),
    }
}

/// Python truthiness of a parsed JSON value.
#[must_use]
pub(crate) fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(value) => *value,
        Json::Int(value) => value.as_i64().is_some_and(|value| value != 0),
        Json::Float(value) => *value != 0.0,
        Json::Str(value) => value.as_str().is_some_and(|value| !value.is_empty()),
        Json::List(items) => !items.is_empty(),
        Json::Dict(object) => !object.is_empty(),
    }
}

/// Python `str(value)` for the scalar JSON values the bulk endpoints use as
/// dict keys (`issue_id = update["id"]`).
#[must_use]
pub(crate) fn python_str(value: &Json) -> String {
    match value {
        Json::Null => "None".to_owned(),
        Json::Bool(value) => {
            if *value {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Json::Int(value) => value.to_string(),
        Json::Float(value) => plane_query::pyfloat::repr(*value),
        Json::Str(value) => value.as_str().map(str::to_owned).unwrap_or_default(),
        Json::List(_) | Json::Dict(_) => String::new(),
    }
}

/// `json.dumps(value, cls=DjangoJSONEncoder)`.
///
/// # Errors
///
/// [`ViewError::Unknown`] if the value cannot be encoded (only a malformed
/// internal value; parsed request JSON always encodes).
pub(crate) fn dumps(value: &Json) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `writable_data(serializer, request.data)` — `request.data` without the keys
/// that are read-only fields on the bound serializer, in request order.
///
/// Unknown keys are kept (`serializer.fields.get(key)` is `None`, whose
/// `getattr(..., "read_only", False)` is `False`). On PATCH, `is_draft` is
/// read-only too.
///
/// # Errors
///
/// [`ViewError::Unknown`] when a JSON body is not an object (the PATCH view has
/// already reproduced the earlier `.pop` failure in that case).
pub(crate) fn accepted_data(ctx: &ViewCtx<'_>, patch: bool) -> Result<Json, ViewError> {
    // `skip_activity = request.data.pop("skip_activity", False)`
    // (`plane/app/views/issue/base.py:630`) removes it before
    // `writable_data(serializer, request.data)` reads the body.
    let read_only = |key: &str| {
        key == "skip_activity" || READ_ONLY_ON_CREATE.contains(&key) || (patch && key == "is_draft")
    };
    match ctx.data()? {
        Data::Json(Json::Dict(object)) => {
            let mut out = PyDict::with_capacity(object.len());
            for (key, value) in object.iter() {
                let Some(key_text) = key.as_str() else {
                    out.insert(key.clone(), value.clone());
                    continue;
                };
                if !read_only(key_text) {
                    out.insert(key.clone(), value.clone());
                }
            }
            Ok(Json::Dict(out))
        }
        Data::Json(_) => Err(ViewError::Unknown(
            "AttributeError: request.data is not a mapping".to_owned(),
        )),
        Data::Form(query) => {
            let mut out = PyDict::with_capacity(query.len());
            let mut seen = std::collections::BTreeSet::new();
            for key in query.keys() {
                if !seen.insert(key) || read_only(key) {
                    continue;
                }
                if let Some(value) = query.get(key) {
                    out.insert(PyStr::new(key), Json::Str(PyStr::new(value)));
                }
            }
            Ok(Json::Dict(out))
        }
    }
}

/// `base_host(request=request, is_app=True)` (`plane/utils/host.py:17-57`).
///
/// # Errors
///
/// An unset `APP_BASE_URL`/`WEB_URL` (Django's `ImproperlyConfigured`, a 500).
pub(crate) fn base_host(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    plane_auth::base_host(
        ctx.settings,
        plane_auth::LoginFlags {
            is_app: true,
            ..Default::default()
        },
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `timezone.now().date()`: the UTC date, not the active timezone's.
#[must_use]
pub(crate) fn today() -> NaiveDate {
    Utc::now().date_naive()
}

/// The 18 fields of `IssueSerializer(issue).data` the archive views see.
///
/// The seven annotation-backed fields are absent because the querysets have no
/// annotations: DRF raises `SkipField` for a missing attribute on a
/// `required=False` field.
#[derive(Debug, Clone)]
pub(crate) struct IssueSerializerData {
    id: Uuid,
    name: String,
    state_id: Option<Uuid>,
    sort_order: f64,
    completed_at: Option<DateTime<Utc>>,
    estimate_point: Option<Uuid>,
    priority: String,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    sequence_id: i32,
    project_id: Uuid,
    parent_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    is_draft: bool,
    archived_at: Option<NaiveDate>,
}

impl IssueSerializerData {
    /// The issue's id (for the activity `issue_id`).
    #[must_use]
    pub(crate) fn id(&self) -> Uuid {
        self.id
    }

    /// The issue's state id (NULL means `issue.state` raises on access).
    #[must_use]
    pub(crate) fn state_id(&self) -> Option<Uuid> {
        self.state_id
    }

    #[must_use]
    pub(crate) fn from_issue(issue: &Issue) -> Self {
        Self {
            id: issue.id,
            name: issue.name.clone(),
            state_id: issue.state_id,
            sort_order: issue.sort_order,
            completed_at: issue.completed_at,
            estimate_point: issue.estimate_point_id,
            priority: issue.priority.clone(),
            start_date: issue.start_date,
            target_date: issue.target_date,
            sequence_id: issue.sequence_id,
            project_id: issue.project_id,
            parent_id: issue.parent_id,
            created_at: issue.created_at,
            updated_at: issue.updated_at,
            created_by: issue.created_by_id,
            updated_by: issue.updated_by_id,
            is_draft: issue.is_draft,
            archived_at: issue.archived_at,
        }
    }

    /// The serializer's dict, in `_readable_fields` order.
    ///
    /// The datetime strings are DRF's: converted to the request timezone and
    /// rendered with six fractional digits and `Z` for UTC, matching
    /// `DateTimeField.to_representation`. `IssueDetail::dje_json` uses the same
    /// representation.
    #[must_use]
    pub(crate) fn to_json(&self, timezone: Tz) -> Doc {
        let zoned = |value: DateTime<Utc>| {
            Doc::DateTimeZoned(value.with_timezone(&timezone).fixed_offset())
        };
        let zoned_opt = |value: Option<DateTime<Utc>>| value.map_or(Doc::Null, zoned);
        Doc::Object(vec![
            ("id".to_owned(), Doc::uuid(self.id)),
            ("name".to_owned(), Doc::string(self.name.as_str())),
            ("state_id".to_owned(), Doc::opt_uuid(self.state_id)),
            ("sort_order".to_owned(), Doc::Float(self.sort_order)),
            ("completed_at".to_owned(), zoned_opt(self.completed_at)),
            (
                "estimate_point".to_owned(),
                Doc::opt_uuid(self.estimate_point),
            ),
            ("priority".to_owned(), Doc::string(self.priority.as_str())),
            ("start_date".to_owned(), Doc::opt_date(self.start_date)),
            ("target_date".to_owned(), Doc::opt_date(self.target_date)),
            (
                "sequence_id".to_owned(),
                Doc::Int(i64::from(self.sequence_id)),
            ),
            ("project_id".to_owned(), Doc::uuid(self.project_id)),
            ("parent_id".to_owned(), Doc::opt_uuid(self.parent_id)),
            ("created_at".to_owned(), zoned(self.created_at)),
            ("updated_at".to_owned(), zoned(self.updated_at)),
            ("created_by".to_owned(), Doc::opt_uuid(self.created_by)),
            ("updated_by".to_owned(), Doc::opt_uuid(self.updated_by)),
            ("is_draft".to_owned(), Doc::Bool(self.is_draft)),
            ("archived_at".to_owned(), Doc::opt_date(self.archived_at)),
        ])
    }
}

/// `IssueSerializer(issue).data` for a full issue.
#[must_use]
pub(crate) fn issue_serializer_json(issue: &Issue, timezone: Tz) -> Doc {
    IssueSerializerData::from_issue(issue).to_json(timezone)
}

/// `json.dumps(IssueSerializer(issue).data, cls=DjangoJSONEncoder)`.
///
/// # Errors
///
/// A rendering failure (unreachable for the values of the projection).
pub(crate) fn issue_serializer_current_instance(
    issue: &Issue,
    timezone: Tz,
) -> Result<String, ViewError> {
    let document = issue_serializer_json(issue, timezone);
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, &document)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// [`super::write_issue_queries::ISSUES_FOR_BULK_ARCHIVE`]'s row: the
/// serializer projection plus `state.group`.
#[derive(Debug, Clone)]
pub(crate) struct IssueSummary {
    pub data: IssueSerializerData,
    pub state_group: Option<String>,
}

impl IssueSummary {
    /// Decode the catalogued row.
    ///
    /// # Errors
    ///
    /// A column type mismatch (impossible for the catalogued statement).
    pub fn from_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            data: IssueSerializerData {
                id: row.try_get(0).map_err(ViewError::from)?,
                name: row.try_get(1).map_err(ViewError::from)?,
                state_id: row.try_get(2).map_err(ViewError::from)?,
                sort_order: row.try_get(3).map_err(ViewError::from)?,
                completed_at: row.try_get(4).map_err(ViewError::from)?,
                estimate_point: row.try_get(5).map_err(ViewError::from)?,
                priority: row.try_get(6).map_err(ViewError::from)?,
                start_date: row.try_get(7).map_err(ViewError::from)?,
                target_date: row.try_get(8).map_err(ViewError::from)?,
                sequence_id: row.try_get(9).map_err(ViewError::from)?,
                project_id: row.try_get(10).map_err(ViewError::from)?,
                parent_id: row.try_get(11).map_err(ViewError::from)?,
                created_at: row.try_get(12).map_err(ViewError::from)?,
                updated_at: row.try_get(13).map_err(ViewError::from)?,
                created_by: row.try_get(14).map_err(ViewError::from)?,
                updated_by: row.try_get(15).map_err(ViewError::from)?,
                is_draft: row.try_get(16).map_err(ViewError::from)?,
                archived_at: row.try_get(17).map_err(ViewError::from)?,
            },
            state_group: row.try_get(18).map_err(ViewError::from)?,
        })
    }
}

/// Render an ordered document with the Django dialect (the bytes
/// `json.dumps(..., cls=DjangoJSONEncoder)` writes for the same values).
///
/// # Errors
///
/// A rendering failure.
pub(crate) fn dje_bytes(document: &Doc) -> Result<Vec<u8>, ViewError> {
    plane_serde::to_vec(plane_serde::Dialect::Django, document)
        .map_err(|error| ViewError::Unknown(error.to_string()))
}

/// Render an ordered document with the DRF dialect.
#[cfg(test)]
#[must_use]
pub(crate) fn drf_bytes(document: &Doc) -> Vec<u8> {
    document.to_drf()
}

/// The `RequestRender` timezone helper: the caller's timezone or UTC.
#[must_use]
pub(crate) fn timezone(ctx: &ViewCtx<'_>) -> Tz {
    ctx.timezone.copied().unwrap_or(chrono_tz::UTC)
}

/// Build the `IssueSerializer` struct from a full issue, only where the
/// generated renderer needs to see it (tests use this).
#[allow(dead_code)]
pub(crate) fn generated_issue_serializer(issue: &Issue) -> IssueSerializer<'_> {
    IssueSerializer {
        id: issue.id,
        name: issue.name.as_str(),
        state_id: issue.state_id,
        sort_order: issue.sort_order,
        completed_at: issue.completed_at,
        estimate_point: issue.estimate_point_id,
        priority: issue.priority.as_str(),
        start_date: issue.start_date,
        target_date: issue.target_date,
        sequence_id: i64::from(issue.sequence_id),
        project_id: issue.project_id,
        parent_id: issue.parent_id,
        cycle_id: FieldValue::absent(),
        module_ids: FieldValue::absent(),
        label_ids: FieldValue::absent(),
        assignee_ids: FieldValue::absent(),
        sub_issues_count: FieldValue::absent(),
        created_at: issue.created_at,
        updated_at: issue.updated_at,
        created_by: issue.created_by_id,
        updated_by: issue.updated_by_id,
        attachment_count: FieldValue::absent(),
        link_count: FieldValue::absent(),
        is_draft: issue.is_draft,
        archived_at: issue.archived_at,
    }
}

/// A `RenderCtx` without a field mask, for the test above.
#[allow(dead_code)]
pub(crate) fn full_render_ctx(timezone: &Tz) -> RenderCtx<'_> {
    RenderCtx::new(timezone, None, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue() -> Issue {
        Issue {
            created_at: DateTime::from_timestamp(1_700_000_000, 123_456_000).expect("timestamp"),
            updated_at: DateTime::from_timestamp(1_700_000_100, 0).expect("timestamp"),
            created_by_id: Some(Uuid::nil()),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::nil(),
            project_id: Uuid::nil(),
            workspace_id: Uuid::nil(),
            parent_id: None,
            state_id: None,
            point: None,
            estimate_point_id: None,
            name: "Fix".to_owned(),
            description_json: plane_db::hooks::issue::empty_json(),
            description_html: "<p></p>".to_owned(),
            description_stripped: None,
            description_binary: None,
            priority: "none".to_owned(),
            start_date: None,
            target_date: None,
            sequence_id: 3,
            sort_order: 65535.0,
            completed_at: None,
            archived_at: None,
            is_draft: false,
            external_source: None,
            external_id: None,
            type_id: None,
        }
    }

    #[test]
    fn archive_current_instance_shape_is_the_eighteen_key_serializer() {
        let document = issue_serializer_json(&issue(), chrono_tz::UTC);
        let bytes = dje_bytes(&document).expect("render");
        let text = String::from_utf8(bytes).expect("utf8");
        assert_eq!(
            text,
            "{\"id\": \"00000000-0000-0000-0000-000000000000\", \"name\": \"Fix\", \
             \"state_id\": null, \"sort_order\": 65535.0, \"completed_at\": null, \
             \"estimate_point\": null, \"priority\": \"none\", \"start_date\": null, \
             \"target_date\": null, \"sequence_id\": 3, \
             \"project_id\": \"00000000-0000-0000-0000-000000000000\", \"parent_id\": null, \
             \"created_at\": \"2023-11-14T22:13:20.123456Z\", \
             \"updated_at\": \"2023-11-14T22:15:00Z\", \
             \"created_by\": \"00000000-0000-0000-0000-000000000000\", \"updated_by\": null, \
             \"is_draft\": false, \"archived_at\": null}"
        );
    }

    #[test]
    fn writable_data_drops_read_only_and_keeps_unknown_keys() {
        let read_only = |key: &str| READ_ONLY_ON_CREATE.contains(&key);
        assert!(read_only("project_id"));
        assert!(!read_only("parent_id"));
        assert!(!read_only("skip_activity")); // excluded by `accepted_data` separately
    }

    #[test]
    fn the_generated_projection_matches_the_hand_built_one() {
        let issue = issue();
        let timezone = chrono_tz::Asia::Kolkata;
        let render_ctx = full_render_ctx(&timezone);
        let generated = plane_serializers::render(&generated_issue_serializer(&issue), &render_ctx)
            .expect("generated");
        let hand_built = drf_bytes(&issue_serializer_json(&issue, timezone));
        assert_eq!(generated, hand_built);
    }
}
