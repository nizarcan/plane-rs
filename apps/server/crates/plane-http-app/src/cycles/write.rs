// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The cycle-core writes (`plane/app/views/cycle/base.py` and the
//! archive/unarchive half of `cycle/archive.py`).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/cycles/` | [`create`] | `base.py:272-336` |
//! | `PATCH …/cycles/{pk}/` | [`partial_update`] | `base.py:337-477` |
//! | `DELETE …/cycles/{pk}/` | [`destroy`] | `base.py:479-519` |
//! | `POST …/cycles/date-check/` | [`date_check`] | `base.py:521-559` |
//! | `POST …/user-favorite-cycles/` | [`favorite_create`] | `base.py:573-586` |
//! | `DELETE …/user-favorite-cycles/{cycle_id}/` | [`favorite_destroy`] | `base.py:587-598` |
//! | `PATCH …/cycles/{cycle_id}/user-properties/` | [`user_properties_patch`] | `base.py:630-649` |
//! | `POST/DELETE …/cycles/{cycle_id}/archive/` | [`archive_post`]/[`archive_delete`] | `archive.py:587-611` |
//! | `POST/DELETE …/archived-cycles/` and `…/{pk}/` | the same two handlers' 500 | 41/42 |
//!
//! The writes are a sequence of autocommitting statements (no request-level
//! transaction upstream): create runs H22 then the INSERT then re-reads the
//! annotated `.values()` row; PATCH reads the row, publishes nothing yet, saves
//! and re-reads; destroy publishes the activity, soft-deletes (H03 + cascade
//! task), removes the caller's favourite (H04) and hard-deletes recent visits.
//!
//! Upstream quirks ported as-is: 41/42 pass no `cycle_id` to a handler that
//! requires one (family 500); a PATCH on a missing/foreign cycle dereferences
//! `None`; the completed-cycle guard builds `request_data` but the serializer
//! still receives `request.data`; archive POST deletes **every** user's
//! favourite; destroy hard-deletes **every** user's recent visit; a favourite
//! destroy hard-deletes the whole `user_favorites.parent` tree; one-sided
//! PATCH dates skip `convert_to_utc`.

use std::str::FromStr;
use std::sync::LazyLock;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, Offset, TimeZone, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks;
use plane_db::models::cycle::Cycle as CycleModel;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall, ModelActivityCall};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::{
    DrfDateTime, DrfError, DrfSerialize, DrfUuid, DrfWriter, PgJson, RenderCtx,
};
use uuid::Uuid;

use crate::common::client;
use crate::cycles::base::{self, CycleRow, params};
use crate::cycles::write_queries as queries;
use crate::write::generated::cycle_write_serializer;
use crate::write::{self, CharOptions, Detail, IntegerOptions, Serializer};

/// `Cycle.timezone`'s `choices` (`TIMEZONE_CHOICES = pytz.common_timezones`),
/// taken from the generated write table.
static TIMEZONES: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    cycle_write_serializer::FIELDS
        .iter()
        .find(|field| field.name == "timezone")
        .map(|field| field.choices.iter().map(|(_, value)| *value).collect())
        .unwrap_or_default()
});

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn origin(ctx: &ViewCtx<'_>) -> Result<String, ViewError> {
    plane_auth::base_host(
        ctx.settings,
        plane_auth::LoginFlags {
            is_app: true,
            ..Default::default()
        },
    )
    .map_err(|error| ViewError::Unknown(error.to_string()))
}

fn epoch() -> i64 {
    Utc::now().timestamp()
}

fn integrity(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

fn empty_object() -> JsonText {
    JsonText::new("{}")
}

fn timezone_of(name: &str) -> Result<Tz, ViewError> {
    Tz::from_str(name).map_err(|_| ViewError::Unknown(format!("UnknownTimeZoneError: {name}")))
}

struct ProjectRow {
    workspace_id: Uuid,
    timezone: String,
}

async fn load_project(client: &Client, project_id: Uuid) -> Result<ProjectRow, ViewError> {
    let statement = client
        .prepare_cached(queries::CYCLE_PROJECT.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await
        .map_err(ViewError::from)?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(ProjectRow {
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
        timezone: row.try_get(2).map_err(ViewError::from)?,
    })
}

/// `request.data` as a mapping. `request.data.get` on a JSON list/scalar is an
/// `AttributeError` (family 500) in the create view, before the pre-check.
fn ensure_mapping(data: &Data) -> Result<(), ViewError> {
    match data {
        Data::Json(Json::Dict(_)) | Data::Form(_) => Ok(()),
        Data::Json(Json::Null) => Err(ViewError::Unknown(
            "AttributeError: 'NoneType' object has no attribute 'get'".to_owned(),
        )),
        Data::Json(_) => Err(ViewError::Unknown(
            "AttributeError: 'list' object has no attribute 'get'".to_owned(),
        )),
    }
}

/// `request.data.get(key, None) is not None`.
fn has_value(data: &Data, key: &str) -> bool {
    match data {
        Data::Json(Json::Dict(map)) => map.get(key).is_some_and(|value| !value.is_null()),
        Data::Form(query) => query.get(key).is_some(),
        Data::Json(_) => false,
    }
}

/// `key in request.data`.
fn has_key(data: &Data, key: &str) -> bool {
    match data {
        Data::Json(Json::Dict(map)) => map.contains_key(key),
        Data::Form(query) => query.contains_key(key),
        Data::Json(_) => false,
    }
}

/// `datetime.combine(day, time.min)` in `timezone`, using pytz's
/// `localize(..., is_dst=False)`: an ambiguous wall time picks standard time
/// (the later occurrence) and a nonexistent one keeps the pre-transition
/// offset, which is what pytz's non-raising default does.
fn localize(naive: NaiveDateTime, timezone: Tz) -> DateTime<Utc> {
    match timezone.from_local_datetime(&naive) {
        chrono::LocalResult::Single(value) => value.with_timezone(&Utc),
        chrono::LocalResult::Ambiguous(_, latest) => latest.with_timezone(&Utc),
        chrono::LocalResult::None => {
            let offset = timezone
                .offset_from_utc_datetime(&(naive - Duration::hours(1)))
                .fix();
            DateTime::from_naive_utc_and_offset(naive - offset, Utc)
        }
    }
}

/// `plane/utils/timezone_converter.py:43-95`.
pub fn convert_to_utc(
    day: NaiveDate,
    timezone: Tz,
    is_start: bool,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    let midnight = day.and_hms_opt(0, 0, 0).expect("midnight is valid");
    if is_start {
        let start = localize(midnight, timezone) + Duration::seconds(1);
        // `localized_datetime.date()` keeps the wall date (pytz does not shift
        // a nonexistent wall time), which is `day`.
        if day == now.with_timezone(&timezone).date_naive() {
            return now;
        }
        return start;
    }
    localize(
        midnight + Duration::hours(23) + Duration::minutes(59),
        timezone,
    )
}

/// Python `str(datetime)` — `isoformat(' ')`, microseconds only when non-zero,
/// `+00:00` for UTC (the archive POST response).
fn python_str_datetime(value: DateTime<Utc>) -> String {
    let base = value.format("%Y-%m-%d %H:%M:%S").to_string();
    let micros = value.timestamp_subsec_micros();
    if micros == 0 {
        format!("{base}+00:00")
    } else {
        format!("{base}.{micros:06}+00:00")
    }
}

/// `DjangoJSONEncoder` for an aware UTC datetime: `isoformat()` truncated to
/// milliseconds, `+00:00` replaced by `Z` (`django/core/serializers/json.py:
/// 93-99`).
fn django_datetime(value: DateTime<Utc>) -> String {
    let base = value.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = value.timestamp_subsec_micros();
    if micros == 0 {
        format!("{base}Z")
    } else {
        format!("{base}.{:03}Z", micros / 1000)
    }
}

fn dumps(value: &Json) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

fn option_datetime(value: Option<DateTime<Utc>>) -> Json {
    value.map_or(Json::Null, |value| Json::from(django_datetime(value)))
}

fn json_column(value: Option<&JsonText>) -> Result<Json, ViewError> {
    match value {
        Some(text) => Ok(
            plane_serde::pyjson::loads(text.as_str().as_bytes(), "utf-8")
                .map_err(|error| ViewError::Unknown(error.to_string()))?,
        ),
        None => Ok(Json::Null),
    }
}

// ---------------------------------------------------------------------------
// The annotated write row (`CycleViewSet.get_queryset().values(...)`)
// ---------------------------------------------------------------------------

/// Decode one `CYCLE_WRITE_ROW`.
fn decode_write_row(row: &tokio_postgres::Row, timezone: Tz) -> Result<CycleRow, ViewError> {
    Ok(CycleRow {
        id: row.try_get(0).map_err(ViewError::from)?,
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
        project_id: row.try_get(2).map_err(ViewError::from)?,
        name: row.try_get(3).map_err(ViewError::from)?,
        description: row.try_get(4).map_err(ViewError::from)?,
        start_date: row.try_get(5).map_err(ViewError::from)?,
        end_date: row.try_get(6).map_err(ViewError::from)?,
        owned_by_id: row.try_get(7).map_err(ViewError::from)?,
        view_props: row.try_get(8).map_err(ViewError::from)?,
        sort_order: row.try_get(9).map_err(ViewError::from)?,
        external_source: row.try_get(10).map_err(ViewError::from)?,
        external_id: row.try_get(11).map_err(ViewError::from)?,
        progress_snapshot: row.try_get(12).map_err(ViewError::from)?,
        logo_props: row.try_get(13).map_err(ViewError::from)?,
        version: Some(row.try_get(14).map_err(ViewError::from)?),
        archived_at: row.try_get(15).map_err(ViewError::from)?,
        is_favorite: row.try_get(16).map_err(ViewError::from)?,
        total_issues: row.try_get(17).map_err(ViewError::from)?,
        completed_issues: row.try_get(18).map_err(ViewError::from)?,
        cancelled_issues: Some(row.try_get(19).map_err(ViewError::from)?),
        assignee_ids: row
            .try_get::<_, Option<Vec<Uuid>>>(20)
            .map_err(ViewError::from)?
            .unwrap_or_default(),
        status: row.try_get(21).map_err(ViewError::from)?,
        created_by: row.try_get(22).map_err(ViewError::from)?,
        sub_issues: None,
        completed_estimate_points: None,
        total_estimate_points: None,
        started_issues: None,
        unstarted_issues: None,
        backlog_issues: None,
        timezone,
    })
}

async fn load_write_row(
    client: &Client,
    params: &base::Params,
    cycle_id: Uuid,
    now: DateTime<Utc>,
    timezone: Tz,
) -> Result<Option<CycleRow>, ViewError> {
    let statement = client
        .prepare_cached(queries::CYCLE_WRITE_ROW.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(
            &statement,
            &[
                &params.slug,
                &params.project_id,
                &params.user_id,
                &now,
                &cycle_id,
            ],
        )
        .await
        .map_err(ViewError::from)?;
    match row {
        Some(row) => Ok(Some(decode_write_row(&row, timezone)?)),
        None => Ok(None),
    }
}

/// The `.values()` set the create/update response renders, in the Python's
/// order (`base.py:290-312`, `:446-468`). Unlike the list, it omits the
/// state-group counts and adds `logo_props` before `version`.
#[derive(Debug)]
struct CycleWriteValue<'a>(&'a CycleRow);

impl DrfSerialize for CycleWriteValue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let row = self.0;
        let mut obj = w.begin_object();
        obj.field_dyn("id", ctx, &DrfUuid(row.id))?;
        obj.field_dyn("workspace_id", ctx, &DrfUuid(row.workspace_id))?;
        obj.field_dyn("project_id", ctx, &DrfUuid(row.project_id))?;
        obj.field("name", &row.name)?;
        obj.field("description", &row.description)?;
        obj.field(
            "start_date",
            &row.start_date
                .map(|value| DrfDateTime(value.with_timezone(&row.timezone))),
        )?;
        obj.field(
            "end_date",
            &row.end_date
                .map(|value| DrfDateTime(value.with_timezone(&row.timezone))),
        )?;
        obj.field_dyn("owned_by_id", ctx, &DrfUuid(row.owned_by_id))?;
        obj.field_json("view_props", row.view_props.as_ref().map(PgJson::from))?;
        obj.field("sort_order", &row.sort_order)?;
        obj.field("external_source", &row.external_source)?;
        obj.field("external_id", &row.external_id)?;
        obj.field_json(
            "progress_snapshot",
            row.progress_snapshot.as_ref().map(PgJson::from),
        )?;
        obj.field_json("logo_props", row.logo_props.as_ref().map(PgJson::from))?;
        obj.field("version", &row.version.unwrap_or(1))?;
        obj.field("is_favorite", &row.is_favorite)?;
        obj.field("total_issues", &row.total_issues)?;
        obj.field("completed_issues", &row.completed_issues)?;
        let assignees: Vec<DrfUuid> = row.assignee_ids.iter().copied().map(DrfUuid).collect();
        obj.field("assignee_ids", &assignees)?;
        obj.field("status", &row.status)?;
        obj.field("created_by", &row.created_by)?;
        obj.end();
        Ok(())
    }
}

fn render_write_row(row: &CycleRow, timezone: Tz, status: u16) -> Result<ViewResponse, ViewError> {
    let bytes = plane_serializers::render(&CycleWriteValue(row), &RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, bytes))
}

/// `json.dumps(CycleSerializer(cycle).data, cls=DjangoJSONEncoder)` — the
/// PATCH activity's `current_instance` (`base.py:346`). `CycleSerializer` has
/// no `version`/`created_by`, and its `started/unstarted/backlog_issues`
/// sources are not annotated by `get_queryset()`, so DRF skips them.
fn current_instance_json(row: &CycleRow) -> Result<String, ViewError> {
    let mut obj = PyDict::with_capacity(19);
    obj.insert(PyStr::new("id"), Json::from(row.id.to_string()));
    obj.insert(
        PyStr::new("workspace_id"),
        Json::from(row.workspace_id.to_string()),
    );
    obj.insert(
        PyStr::new("project_id"),
        Json::from(row.project_id.to_string()),
    );
    obj.insert(PyStr::new("name"), Json::from(row.name.as_str()));
    obj.insert(
        PyStr::new("description"),
        Json::from(row.description.as_str()),
    );
    obj.insert(PyStr::new("start_date"), option_datetime(row.start_date));
    obj.insert(PyStr::new("end_date"), option_datetime(row.end_date));
    obj.insert(
        PyStr::new("owned_by_id"),
        Json::from(row.owned_by_id.to_string()),
    );
    obj.insert(
        PyStr::new("view_props"),
        json_column(row.view_props.as_ref())?,
    );
    obj.insert(PyStr::new("sort_order"), Json::from(row.sort_order));
    obj.insert(
        PyStr::new("external_source"),
        row.external_source
            .as_deref()
            .map_or(Json::Null, Json::from),
    );
    obj.insert(
        PyStr::new("external_id"),
        row.external_id.as_deref().map_or(Json::Null, Json::from),
    );
    obj.insert(
        PyStr::new("progress_snapshot"),
        json_column(row.progress_snapshot.as_ref())?,
    );
    obj.insert(
        PyStr::new("logo_props"),
        json_column(row.logo_props.as_ref())?,
    );
    obj.insert(PyStr::new("is_favorite"), Json::from(row.is_favorite));
    obj.insert(PyStr::new("total_issues"), Json::from(row.total_issues));
    obj.insert(
        PyStr::new("cancelled_issues"),
        Json::from(row.cancelled_issues.unwrap_or(0)),
    );
    obj.insert(
        PyStr::new("completed_issues"),
        Json::from(row.completed_issues),
    );
    obj.insert(PyStr::new("status"), Json::from(row.status.as_str()));
    dumps(&Json::Dict(obj))
}

// ---------------------------------------------------------------------------
// `CycleWriteSerializer` validation
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CycleBody {
    name: Option<String>,
    description: Option<String>,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
    view_props: Option<JsonText>,
    sort_order: Option<f64>,
    external_source: Option<String>,
    external_id: Option<String>,
    progress_snapshot: Option<JsonText>,
    logo_props: Option<JsonText>,
    timezone: Option<String>,
    version: Option<i64>,
    deleted_at: Option<DateTime<Utc>>,
}

/// Which nullable fields the body carried (`null` included), so a PATCH can
/// distinguish "absent" from "clear it".
struct CyclePresence {
    start_date: bool,
    end_date: bool,
    deleted_at: bool,
    external_source: bool,
    external_id: bool,
}

struct ValidatedCycle {
    body: CycleBody,
    presence: CyclePresence,
}

fn version_i32(value: Option<i64>) -> Result<i32, ViewError> {
    match value {
        None => Ok(1),
        Some(value) => {
            i32::try_from(value).map_err(|_| ViewError::Unknown("integer out of range".to_owned()))
        }
    }
}

/// `CycleWriteSerializer(...).is_valid()` plus its custom `validate()`.
///
/// `url_project_id` is the serializer context's `project_id`;
/// `instance_project_id` is `self.instance.project_id` for a PATCH.
async fn validate_cycle(
    ctx: &ViewCtx<'_>,
    client: &Client,
    partial: bool,
    url_project_id: Uuid,
    instance_project_id: Option<Uuid>,
) -> Result<Result<ValidatedCycle, ViewResponse>, ViewError> {
    let mut serializer = Serializer::new(ctx, partial)?;

    // `_meta.fields` order — the generated table's order.
    let deleted_at = serializer.datetime_field("deleted_at", true);
    let name = serializer.char_field(
        "name",
        CharOptions {
            allow_null: false,
            allow_blank: false,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    if name.is_none() && serializer.missing_required("name") {
        serializer.push_error("name", Detail::required());
    }
    let description = serializer.char_field(
        "description",
        CharOptions {
            allow_null: false,
            allow_blank: true,
            trim_whitespace: true,
            max_length: None,
        },
    );
    let start_date = serializer.datetime_field("start_date", true);
    let end_date = serializer.datetime_field("end_date", true);
    let view_props = serializer.json_field("view_props", false);
    let sort_order = serializer.float_field("sort_order", false);
    let external_source = serializer.char_field(
        "external_source",
        CharOptions {
            allow_null: true,
            allow_blank: true,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    let external_id = serializer.char_field(
        "external_id",
        CharOptions {
            allow_null: true,
            allow_blank: true,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    let progress_snapshot = serializer.json_field("progress_snapshot", false);
    let logo_props = serializer.json_field("logo_props", false);
    let timezone = serializer.choice_field("timezone", false, false, TIMEZONES.as_slice());
    let version = serializer.integer_field(
        "version",
        false,
        IntegerOptions {
            min_value: None,
            max_value: None,
        },
    );

    let values = match serializer.finish() {
        Ok(values) => values,
        Err(errors) => return Ok(Err(errors.into_response())),
    };
    let presence = CyclePresence {
        start_date: values.contains("start_date"),
        end_date: values.contains("end_date"),
        deleted_at: values.contains("deleted_at"),
        external_source: values.contains("external_source"),
        external_id: values.contains("external_id"),
    };

    let mut body = CycleBody {
        name,
        description,
        start_date,
        end_date,
        view_props,
        sort_order,
        external_source,
        external_id,
        progress_snapshot,
        logo_props,
        timezone,
        version,
        deleted_at,
    };

    if let (Some(start), Some(end)) = (body.start_date, body.end_date) {
        if start > end {
            return Ok(Err(write::field_string_error(
                "non_field_errors",
                "Start date cannot exceed end date",
            )));
        }
        let project_id = initial_project_id(ctx)?
            .or(instance_project_id)
            .unwrap_or(url_project_id);
        let project = load_project(client, project_id).await?;
        let timezone = timezone_of(&project.timezone)?;
        let now = write::now();
        let active_timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let start_day = start.with_timezone(&active_timezone).date_naive();
        let end_day = end.with_timezone(&active_timezone).date_naive();
        body.start_date = Some(convert_to_utc(start_day, timezone, true, now));
        body.end_date = Some(convert_to_utc(end_day, timezone, false, now));
    }

    Ok(Ok(ValidatedCycle { body, presence }))
}

/// `self.initial_data.get("project_id")` — the serializer's `validate()`
/// prefers it over the instance/context.
fn initial_project_id(ctx: &ViewCtx<'_>) -> Result<Option<Uuid>, ViewError> {
    let data = ctx.data()?;
    let Some(raw) = data.get("project_id") else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    Uuid::parse_str(raw)
        .map(Some)
        .map_err(|_| ViewError::Validation)
}

// ---------------------------------------------------------------------------
// Activity
// ---------------------------------------------------------------------------

async fn publish_model_activity(
    ctx: &ViewCtx<'_>,
    slug: &str,
    cycle_id: Uuid,
    current_instance: Option<String>,
    user: Uuid,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::MODEL_ACTIVITY;
    let message = tasks::model_activity(ModelActivityCall {
        model_name: "cycle".to_owned(),
        model_id: cycle_id.to_string(),
        requested_data: write::request_data_arg(ctx)?,
        current_instance,
        actor_id: user,
        slug: slug.to_owned(),
        origin: origin(ctx)?,
    });
    crate::common::jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task))
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Route 31 — create
// ---------------------------------------------------------------------------

/// `CycleViewSet.create` (`base.py:272-336`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        ensure_mapping(ctx.data()?)?;
        if has_value(ctx.data()?, "start_date") != has_value(ctx.data()?, "end_date") {
            return Ok(base::error_body(
                400,
                "Both start date and end date are either required or are to be null",
            ));
        }

        let client = client(ctx)?;
        let params = params(ctx)?;
        let validated = match validate_cycle(ctx, client, false, params.project_id, None).await? {
            Ok(validated) => validated,
            Err(response) => return Ok(response),
        };
        let project = load_project(client, params.project_id).await?;
        let timezone = timezone_of(&project.timezone)?;
        let body = validated.body;
        let name = body
            .name
            .clone()
            .ok_or_else(|| ViewError::Unknown("validated name vanished".to_owned()))?;
        let sort_order = hooks::cycle::before_insert(
            client,
            params.project_id,
            body.sort_order.unwrap_or(65535.0),
        )
        .await
        .map_err(ViewError::from)?;

        let now = write::now();
        let audit = hooks::audit::Audit::on_create(Some(params.user_id));
        let cycle = CycleModel {
            created_at: now,
            updated_at: now,
            created_by_id: audit.created_by_id,
            updated_by_id: audit.updated_by_id,
            deleted_at: body.deleted_at,
            id: Uuid::new_v4(),
            project_id: params.project_id,
            workspace_id: project.workspace_id,
            name,
            description: body.description.clone().unwrap_or_default(),
            start_date: body.start_date,
            end_date: body.end_date,
            owned_by_id: params.user_id,
            view_props: body.view_props.clone().unwrap_or_else(empty_object),
            sort_order,
            external_source: body.external_source.clone(),
            external_id: body.external_id.clone(),
            progress_snapshot: body.progress_snapshot.clone().unwrap_or_else(empty_object),
            archived_at: None,
            logo_props: body.logo_props.clone().unwrap_or_else(empty_object),
            timezone: body.timezone.clone().unwrap_or_else(|| "UTC".to_owned()),
            version: version_i32(body.version)?,
        };
        client
            .execute(
                queries::CYCLE_INSERT.sql,
                &[
                    &cycle.created_at,
                    &cycle.updated_at,
                    &cycle.created_by_id,
                    &cycle.updated_by_id,
                    &cycle.deleted_at,
                    &cycle.id,
                    &cycle.project_id,
                    &cycle.workspace_id,
                    &cycle.name,
                    &cycle.description,
                    &cycle.start_date,
                    &cycle.end_date,
                    &cycle.owned_by_id,
                    &cycle.view_props,
                    &cycle.sort_order,
                    &cycle.external_source,
                    &cycle.external_id,
                    &cycle.progress_snapshot,
                    &cycle.logo_props,
                    &cycle.timezone,
                    &cycle.version,
                ],
            )
            .await
            .map_err(integrity)?;

        let now = write::now();
        let Some(row) = load_write_row(client, &params, cycle.id, now, timezone).await? else {
            // `cycle["id"]` on the `None` the default manager returns for a
            // body that wrote `deleted_at`.
            return Err(ViewError::Unknown(
                "TypeError: 'NoneType' object is not subscriptable".to_owned(),
            ));
        };
        publish_model_activity(ctx, &params.slug, cycle.id, None, params.user_id).await?;
        render_write_row(&row, timezone, 201)
    })
}

// ---------------------------------------------------------------------------
// Route 32 — partial_update, destroy
// ---------------------------------------------------------------------------

/// `CycleViewSet.partial_update` (`base.py:337-477`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let pk = base::uuid_kwarg(ctx, "pk")?;
        // `get_queryset()` reads the project for its timezone before it
        // evaluates the row (`cycle/base.py:78-86`).
        let project = load_project(client, params.project_id).await?;
        let timezone = timezone_of(&project.timezone)?;

        let now = write::now();
        let Some(row) = load_write_row(client, &params, pk, now, timezone).await? else {
            // `cycle.archived_at` on the `None` `.first()` returned.
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'archived_at'".to_owned(),
            ));
        };
        if row.archived_at.is_some() {
            return Ok(base::error_body(400, "Archived cycle cannot be updated"));
        }
        let current_instance = current_instance_json(&row)?;

        if row.end_date.is_some_and(|end| end < now) && !has_key(ctx.data()?, "sort_order") {
            return Ok(base::error_body(
                400,
                "The Cycle has already been completed so it cannot be edited",
            ));
        }

        let validated =
            match validate_cycle(ctx, client, true, params.project_id, Some(row.project_id)).await?
            {
                Ok(validated) => validated,
                Err(response) => return Ok(response),
            };
        let body = validated.body;
        let presence = validated.presence;

        let statement = client
            .prepare_cached(queries::CYCLE_MODEL_ROW.sql)
            .await
            .map_err(ViewError::from)?;
        let stored = client
            .query_opt(&statement, &[&pk])
            .await
            .map_err(ViewError::from)?
            .ok_or_else(|| {
                ViewError::Unknown(
                    "AttributeError: 'NoneType' object has no attribute 'save'".to_owned(),
                )
            })?;
        let mut cycle = CycleModel::from_row(&stored, 0).map_err(ViewError::from)?;

        if let Some(name) = body.name {
            cycle.name = name;
        }
        if let Some(description) = body.description {
            cycle.description = description;
        }
        if presence.start_date {
            cycle.start_date = body.start_date;
        }
        if presence.end_date {
            cycle.end_date = body.end_date;
        }
        if let Some(view_props) = body.view_props {
            cycle.view_props = view_props;
        }
        if let Some(sort_order) = body.sort_order {
            cycle.sort_order = sort_order;
        }
        if presence.external_source {
            cycle.external_source = body.external_source;
        }
        if presence.external_id {
            cycle.external_id = body.external_id;
        }
        if let Some(progress_snapshot) = body.progress_snapshot {
            cycle.progress_snapshot = progress_snapshot;
        }
        if let Some(logo_props) = body.logo_props {
            cycle.logo_props = logo_props;
        }
        if let Some(timezone) = body.timezone {
            cycle.timezone = timezone;
        }
        if let Some(version) = body.version {
            cycle.version = version_i32(Some(version))?;
        }
        if presence.deleted_at {
            cycle.deleted_at = body.deleted_at;
        }
        // H01: only `updated_by`/`updated_at` change on an update.
        cycle.updated_at = write::now();
        cycle.updated_by_id = Some(params.user_id);

        client
            .execute(
                queries::CYCLE_UPDATE.sql,
                &[
                    &cycle.created_at,
                    &cycle.updated_at,
                    &cycle.created_by_id,
                    &cycle.updated_by_id,
                    &cycle.deleted_at,
                    &cycle.id,
                    &cycle.project_id,
                    &cycle.workspace_id,
                    &cycle.name,
                    &cycle.description,
                    &cycle.start_date,
                    &cycle.end_date,
                    &cycle.owned_by_id,
                    &cycle.view_props,
                    &cycle.sort_order,
                    &cycle.external_source,
                    &cycle.external_id,
                    &cycle.progress_snapshot,
                    &cycle.archived_at,
                    &cycle.logo_props,
                    &cycle.timezone,
                    &cycle.version,
                    &cycle.id,
                ],
            )
            .await
            .map_err(integrity)?;

        let now = write::now();
        let Some(row) = load_write_row(client, &params, pk, now, timezone).await? else {
            return Err(ViewError::Unknown(
                "TypeError: 'NoneType' object is not subscriptable".to_owned(),
            ));
        };
        publish_model_activity(
            ctx,
            &params.slug,
            pk,
            Some(current_instance),
            params.user_id,
        )
        .await?;
        render_write_row(&row, timezone, 200)
    })
}

/// `CycleViewSet.destroy` (`base.py:479-519`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let pk = base::uuid_kwarg(ctx, "pk")?;

        let statement = client
            .prepare_cached(queries::CYCLE_DESTROY_LOOKUP.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&statement, &[&params.slug, &params.project_id, &pk])
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let name: String = row.try_get(1).map_err(ViewError::from)?;

        let issues_statement = client
            .prepare_cached(queries::CYCLE_ISSUE_IDS.sql)
            .await
            .map_err(ViewError::from)?;
        let issues = client
            .query(&issues_statement, &[&pk])
            .await
            .map_err(ViewError::from)?
            .iter()
            .map(|row| -> Result<Uuid, ViewError> { row.try_get(0).map_err(ViewError::from) })
            .collect::<Result<Vec<Uuid>, _>>()?;

        let mut requested = PyDict::with_capacity(3);
        requested.insert(PyStr::new("cycle_id"), Json::from(pk.to_string()));
        requested.insert(PyStr::new("cycle_name"), Json::from(name));
        requested.insert(
            PyStr::new("issues"),
            Json::List(
                issues
                    .iter()
                    .map(|issue| Json::from(issue.to_string()))
                    .collect(),
            ),
        );
        let activity = tasks::issue_activity(IssueActivityCall::view(
            IssueActivityBase::new(
                "cycle.activity.deleted",
                TaskArg::json_text(dumps(&Json::Dict(requested))?),
                params.user_id.to_string(),
                TaskArg::str(pk.to_string()),
                TaskArg::str(params.project_id.to_string()),
                TaskArg::Null,
                epoch(),
            ),
            origin(ctx)?,
        ));
        crate::common::jobs::dispatch_task(
            ctx,
            activity.task(),
            &activity,
            plane_jobs::queue::Job::from_message(&activity),
        )
        .await?;

        // `cycle.delete()` — H03: two `timezone.now()` calls (the attribute and
        // `auto_now`), then the cascade task.
        let deleted_at = write::now();
        let updated_at = write::now();
        client
            .execute(
                queries::CYCLE_SOFT_DELETE.sql,
                &[&deleted_at, &updated_at, &Some(params.user_id), &pk],
            )
            .await
            .map_err(ViewError::from)?;
        write::publish_soft_delete_cascade(ctx, "db", "cycle", pk).await?;

        client
            .execute(
                queries::USER_FAVORITE_SOFT_DELETE_USER.sql,
                &[&write::now(), &params.user_id, &pk, &params.project_id],
            )
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                queries::USER_RECENT_VISIT_HARD_DELETE.sql,
                &[&params.project_id, &pk, &params.slug],
            )
            .await
            .map_err(ViewError::from)?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Route 35 — date-check
// ---------------------------------------------------------------------------

enum RawArg<'a> {
    Json(&'a Json),
    Text(&'a str),
}

impl RawArg<'_> {
    fn truthy(&self) -> bool {
        match self {
            Self::Text(text) => !text.is_empty(),
            Self::Json(Json::Null | Json::Bool(false)) => false,
            Self::Json(Json::Int(value)) => value.as_i64().is_some_and(|value| value != 0),
            Self::Json(Json::Float(value)) => *value != 0.0,
            Self::Json(Json::Str(value)) => value.as_str().is_some_and(|text| !text.is_empty()),
            Self::Json(Json::List(items)) => !items.is_empty(),
            Self::Json(Json::Dict(map)) => !map.is_empty(),
            Self::Json(Json::Bool(true)) => true,
        }
    }

    /// Python `str(value)`; only a string can parse as `%Y-%m-%d`.
    fn python_str(&self) -> String {
        match self {
            Self::Text(text) => (*text).to_owned(),
            Self::Json(Json::Str(value)) => value.as_str().unwrap_or_default().to_owned(),
            Self::Json(Json::Int(value)) => value.to_string(),
            Self::Json(Json::Float(value)) => plane_query::pyfloat::repr(*value),
            Self::Json(Json::Bool(true)) => "True".to_owned(),
            Self::Json(Json::Bool(false)) => "False".to_owned(),
            Self::Json(Json::Null) => "None".to_owned(),
            Self::Json(Json::List(_) | Json::Dict(_)) => "<value>".to_owned(),
        }
    }
}

fn raw_arg<'a>(data: &'a Data, key: &str) -> Option<RawArg<'a>> {
    match data {
        Data::Json(Json::Dict(map)) => map.get(key).map(RawArg::Json),
        Data::Form(query) => query.get(key).map(RawArg::Text),
        Data::Json(_) => None,
    }
}

fn cycle_id_arg(data: &Data) -> Result<Option<Uuid>, ViewError> {
    let Some(value) = raw_arg(data, "cycle_id") else {
        return Ok(None);
    };
    match value {
        RawArg::Json(Json::Null) => Ok(None),
        RawArg::Json(Json::Str(text)) => Uuid::parse_str(text.as_str().unwrap_or_default())
            .map(Some)
            .map_err(|_| ViewError::Validation),
        RawArg::Text(text) => Uuid::parse_str(text)
            .map(Some)
            .map_err(|_| ViewError::Validation),
        _ => Err(ViewError::Unknown(
            "ValidationError: cycle_id is not a valid UUID".to_owned(),
        )),
    }
}

/// `CycleDateCheckEndpoint.post` (`base.py:521-559`).
pub fn date_check<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let data = ctx.data()?;
        let (Some(start_raw), Some(end_raw)) =
            (raw_arg(data, "start_date"), raw_arg(data, "end_date"))
        else {
            return Ok(base::error_body(
                400,
                "Start date and end date both are required",
            ));
        };
        if !start_raw.truthy() || !end_raw.truthy() {
            return Ok(base::error_body(
                400,
                "Start date and end date both are required",
            ));
        }
        let start_text = start_raw.python_str();
        let end_text = end_raw.python_str();
        let start_day = NaiveDate::parse_from_str(&start_text, "%Y-%m-%d").map_err(|_| {
            ViewError::Unknown(format!(
                "ValueError: time data '{start_text}' does not match format '%Y-%m-%d'"
            ))
        })?;
        let end_day = NaiveDate::parse_from_str(&end_text, "%Y-%m-%d").map_err(|_| {
            ViewError::Unknown(format!(
                "ValueError: time data '{end_text}' does not match format '%Y-%m-%d'"
            ))
        })?;

        let client = client(ctx)?;
        let params = params(ctx)?;
        let project = load_project(client, params.project_id).await?;
        let timezone = timezone_of(&project.timezone)?;
        let now = write::now();
        let start = convert_to_utc(start_day, timezone, true, now);
        let end = convert_to_utc(end_day, timezone, false, now);
        let excluded = cycle_id_arg(data)?;

        let statement = client
            .prepare_cached(queries::CYCLE_DATE_OVERLAP.sql)
            .await
            .map_err(ViewError::from)?;
        let overlap = client
            .query_opt(
                &statement,
                &[&params.slug, &params.project_id, &start, &end, &excluded],
            )
            .await
            .map_err(ViewError::from)?
            .is_some();
        if overlap {
            Ok(ViewResponse::json(
                200,
                serde_json::json!({
                    "error": "You have a cycle already on the given dates, if you want to create a draft cycle you can do that by removing dates",
                    "status": false,
                }),
            ))
        } else {
            Ok(ViewResponse::json(200, serde_json::json!({"status": true})))
        }
    })
}

// ---------------------------------------------------------------------------
// Routes 36/37 — favourites
// ---------------------------------------------------------------------------

/// `CycleFavoriteViewSet.create` (`base.py:573-586`).
pub fn favorite_create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let Some(raw) = ctx.data()?.get("cycle") else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let cycle_id = Uuid::parse_str(raw).map_err(|_| ViewError::Validation)?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(queries::CYCLE_FAVORITE_TARGET.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[&params.project_id, &cycle_id, &params.slug, &params.user_id],
            )
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let workspace_id: Uuid = row.try_get(1).map_err(ViewError::from)?;

        // `CycleFavoriteViewSet.create` never sends a `sequence`: H25's
        // aggregate decides, else the model default.
        let sequence = hooks::favorite::before_insert(client, workspace_id, None)
            .await
            .map_err(ViewError::from)?;
        let now = write::now();
        client
            .execute(
                queries::USER_FAVORITE_INSERT.sql,
                &[
                    &now,
                    &now,
                    &params.user_id,
                    &Uuid::new_v4(),
                    &workspace_id,
                    &params.project_id,
                    &params.user_id,
                    &cycle_id,
                    &sequence,
                ],
            )
            .await
            .map_err(integrity)?;
        Ok(ViewResponse::empty(204))
    })
}

/// `CycleFavoriteViewSet.destroy` (`base.py:587-598`).
pub fn favorite_destroy<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let params = params(ctx)?;
        let cycle_id = base::uuid_kwarg(ctx, "cycle_id")?;
        let client = client(ctx)?;
        let statement = client
            .prepare_cached(queries::USER_FAVORITE_LOOKUP.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[&params.project_id, &params.user_id, &params.slug, &cycle_id],
            )
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let id: Uuid = row.try_get(0).map_err(ViewError::from)?;
        // `delete(soft=False)` → Django's deletion collector over the
        // self-referential `parent` CASCADE (`plane/db/mixins.py:72-82`,
        // `plane/db/models/favorite.py:25-31`); shared with 126/111/150/184.
        write::favorite::hard_delete(client, id).await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Route 39 — user properties
// ---------------------------------------------------------------------------

fn apply_json_field(data: &Data, stored: &mut JsonText, key: &str) -> Result<(), ViewError> {
    match data {
        Data::Json(Json::Dict(map)) => {
            if let Some(value) = map.get(key) {
                if value.is_null() {
                    // `JSONField` is `null=False`: `save()` is IntegrityError.
                    return Err(ViewError::Integrity);
                }
                *stored = JsonText::from_json(value).map_err(ViewError::from)?;
            }
        }
        Data::Form(query) => {
            if let Some(value) = query.get(key) {
                *stored = JsonText::from_json(&Json::from(value)).map_err(ViewError::from)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// `CycleUserPropertiesEndpoint.patch` (`base.py:630-649`).
pub fn user_properties_patch<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = params(ctx)?;
        let cycle_id = base::uuid_kwarg(ctx, "cycle_id")?;

        let statement = client
            .prepare_cached(crate::cycles::queries::CYCLE_USER_PROPERTIES_BY_KEY.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[&params.user_id, &params.project_id, &cycle_id, &params.slug],
            )
            .await
            .map_err(ViewError::from)?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let created_at: DateTime<Utc> = row.try_get(0).map_err(ViewError::from)?;
        let created_by: Option<Uuid> = row.try_get(2).map_err(ViewError::from)?;
        let deleted_at: Option<DateTime<Utc>> = row.try_get(4).map_err(ViewError::from)?;
        let id: Uuid = row.try_get(5).map_err(ViewError::from)?;
        let workspace_id: Uuid = row.try_get(7).map_err(ViewError::from)?;
        let user_id: Uuid = row.try_get(9).map_err(ViewError::from)?;
        let mut filters: JsonText = row.try_get(10).map_err(ViewError::from)?;
        let mut display_filters: JsonText = row.try_get(11).map_err(ViewError::from)?;
        let mut display_properties: JsonText = row.try_get(12).map_err(ViewError::from)?;
        let mut rich_filters: JsonText = row.try_get(13).map_err(ViewError::from)?;

        let data = ctx.data()?;
        apply_json_field(data, &mut filters, "filters")?;
        apply_json_field(data, &mut rich_filters, "rich_filters")?;
        apply_json_field(data, &mut display_filters, "display_filters")?;
        apply_json_field(data, &mut display_properties, "display_properties")?;

        let updated_at = write::now();
        client
            .execute(
                queries::CYCLE_USER_PROPERTIES_UPDATE.sql,
                &[
                    &created_at,
                    &updated_at,
                    &created_by,
                    &Some(params.user_id),
                    &deleted_at,
                    &id,
                    &params.project_id,
                    &workspace_id,
                    &cycle_id,
                    &user_id,
                    &filters,
                    &display_filters,
                    &display_properties,
                    &rich_filters,
                    &id,
                ],
            )
            .await
            .map_err(integrity)?;

        let properties = base::UserProperties {
            id,
            created_at,
            updated_at,
            deleted_at,
            filters: Some(filters.into_string()),
            display_filters: Some(display_filters.into_string()),
            display_properties: Some(display_properties.into_string()),
            rich_filters: Some(rich_filters.into_string()),
            created_by,
            updated_by: Some(params.user_id),
            project_id: params.project_id,
            workspace_id,
            cycle_id,
            user_id,
        };
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let bytes = plane_serializers::render(&properties, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, bytes))
    })
}

// ---------------------------------------------------------------------------
// Routes 40/41/42 — archive/unarchive
// ---------------------------------------------------------------------------

fn archive_cycle_id(ctx: &ViewCtx<'_>, method: &str) -> Result<Uuid, ViewError> {
    let Some(raw) = ctx.kwarg("cycle_id") else {
        let message = if ctx.kwarg("pk").is_some() {
            format!(
                "TypeError: CycleArchiveUnarchiveEndpoint.{method}() got an unexpected keyword \
                 argument 'pk'"
            )
        } else {
            format!(
                "TypeError: CycleArchiveUnarchiveEndpoint.{method}() missing 1 required \
                 positional argument: 'cycle_id'"
            )
        };
        return Err(ViewError::Unknown(message));
    };
    Uuid::parse_str(raw).map_err(|_| ViewError::Validation)
}

async fn archive_lookup(
    client: &Client,
    params: &base::Params,
    cycle_id: Uuid,
) -> Result<Option<(Option<DateTime<Utc>>, Option<DateTime<Utc>>)>, ViewError> {
    let statement = client
        .prepare_cached(queries::CYCLE_ARCHIVE_LOOKUP.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&cycle_id, &params.project_id, &params.slug])
        .await
        .map_err(ViewError::from)?;
    match row {
        Some(row) => Ok(Some((
            row.try_get(1).map_err(ViewError::from)?,
            row.try_get(2).map_err(ViewError::from)?,
        ))),
        None => Ok(None),
    }
}

/// `CycleArchiveUnarchiveEndpoint.post` (`archive.py:587-605`), plus the
/// unconditional 500 the same method answers on routes 41/42.
pub fn archive_post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let cycle_id = archive_cycle_id(ctx, "post")?;
        let client = client(ctx)?;
        let params = params(ctx)?;
        let Some((end_date, _archived_at)) = archive_lookup(client, &params, cycle_id).await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let now = write::now();
        if end_date.is_none_or(|end| end >= now) {
            return Ok(base::error_body(
                400,
                "Only completed cycles can be archived",
            ));
        }
        let archived_at = write::now();
        let updated_at = write::now();
        client
            .execute(
                queries::CYCLE_SET_ARCHIVED.sql,
                &[&archived_at, &updated_at, &Some(params.user_id), &cycle_id],
            )
            .await
            .map_err(ViewError::from)?;
        // H04 for **every** user (`archive.py:599-604`).
        client
            .execute(
                queries::USER_FAVORITE_SOFT_DELETE_ALL.sql,
                &[&write::now(), &cycle_id, &params.project_id, &params.slug],
            )
            .await
            .map_err(ViewError::from)?;
        Ok(ViewResponse::json(
            200,
            serde_json::json!({"archived_at": python_str_datetime(archived_at)}),
        ))
    })
}

/// `CycleArchiveUnarchiveEndpoint.delete` (`archive.py:607-611`), plus the
/// unconditional 500 the same method answers on routes 41/42.
pub fn archive_delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let cycle_id = archive_cycle_id(ctx, "delete")?;
        let client = client(ctx)?;
        let params = params(ctx)?;
        let Some((_end_date, _archived_at)) = archive_lookup(client, &params, cycle_id).await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let updated_at = write::now();
        client
            .execute(
                queries::CYCLE_CLEAR_ARCHIVED.sql,
                &[&updated_at, &Some(params.user_id), &cycle_id],
            )
            .await
            .map_err(ViewError::from)?;
        Ok(ViewResponse::empty(204))
    })
}
