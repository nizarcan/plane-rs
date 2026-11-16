// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The module-core writes (`plane/app/views/module/base.py` and the
//! archive/unarchive half of `module/archive.py`).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/modules/` | 103 [`create`] | `base.py:295-352` |
//! | `PATCH …/modules/{pk}/` | 104 [`partial_update`] | `base.py:652-722` |
//! | `DELETE …/modules/{pk}/` | 104 [`destroy`] | `base.py:724-760` |
//! | `PATCH …/modules/{module_id}/user-properties/` | 112 [`user_properties_patch`] | `base.py:836-855` |
//! | `POST/DELETE …/modules/{module_id}/archive/` | 113 [`archive_post`]/[`archive_delete`] | `archive.py:544-565` |
//! | `POST/DELETE …/archived-modules/` and `…/{pk}/` | 114/115 — the same two handlers' 500 | `archive.py:544-565` |
//!
//! The writes are a sequence of autocommitting statements (no request-level
//! transaction upstream): create runs H21, the INSERT and the member
//! `bulk_create`, then re-reads the annotated `.values()` row; PATCH replaces
//! the member rows (H04 + `bulk_create`), saves the full row and re-reads;
//! destroy publishes one delete activity per module issue, soft-deletes (H03 +
//! cascade task), H04-deletes every module issue and the caller's favourite and
//! hard-deletes every user's recent visit.
//!
//! Upstream quirks ported as-is: 114/115 pass no `module_id` to a handler that
//! requires one (family 500); a PATCH on a missing/foreign module is the
//! view's own 404 (the `.first()` guard is explicit, unlike cycles); archive
//! POST deletes **every** user's favourite; the archive guard accepts only
//! `completed`/`cancelled`, so the fixture's `in-progress` module is a 400.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks;
use plane_db::models::module::Module as ModuleModel;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall, ModelActivityCall};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::generated::app_module::ModuleUserPropertiesSerializer;
use plane_serializers::{
    DrfDateTime, DrfError, DrfSerialize, DrfUuid, DrfUuidList, DrfWriter, PgJson, RenderCtx,
};
use uuid::Uuid;

use crate::common::client;
use crate::modules::base::{self, ModuleRow};
use crate::modules::write_queries as queries;
use crate::write::generated::module_write_serializer;
use crate::write::{self, Serializer, Validated};

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

/// `json.dumps(…, cls=DjangoJSONEncoder)` (`Dialect::Django`).
fn dumps(value: &Json) -> Result<String, ViewError> {
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes).map_err(|error| ViewError::Unknown(error.to_string()))
}

/// A stored `jsonb` column as the parsed value `json.dumps` would re-encode.
fn json_column(value: Option<&JsonText>) -> Result<Json, ViewError> {
    match value {
        Some(text) => Ok(
            plane_serde::pyjson::loads(text.as_str().as_bytes(), "utf-8")
                .map_err(|error| ViewError::Unknown(error.to_string()))?,
        ),
        None => Ok(Json::Null),
    }
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

struct ProjectRow {
    workspace_id: Uuid,
}

/// `Project.objects.get(workspace__slug=slug, pk=project_id)`
/// (`module/base.py:297`).
async fn load_project(
    client: &Client,
    slug: &str,
    project_id: Uuid,
) -> Result<ProjectRow, ViewError> {
    let statement = client
        .prepare_cached(queries::MODULE_PROJECT.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&project_id, &slug])
        .await
        .map_err(ViewError::from)?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(ProjectRow {
        workspace_id: row.try_get(1).map_err(ViewError::from)?,
    })
}

/// `Module.objects.filter(name=…).exists()` — the serializer's duplicate-name
/// guard, raised as `ValidationError({"error": …})` from `save()`
/// (`serializers/module.py:68-72`, `:94-100`).
async fn duplicate_name(
    client: &Client,
    name: &str,
    project_id: Uuid,
    exclude: Option<Uuid>,
) -> Result<bool, ViewError> {
    let query = if exclude.is_some() {
        &queries::MODULE_NAME_EXISTS_OTHER
    } else {
        &queries::MODULE_NAME_EXISTS
    };
    let statement = client
        .prepare_cached(query.sql)
        .await
        .map_err(ViewError::from)?;
    let row = match exclude {
        Some(id) => client
            .query_opt(&statement, &[&name, &project_id, &id])
            .await
            .map_err(ViewError::from)?,
        None => client
            .query_opt(&statement, &[&name, &project_id])
            .await
            .map_err(ViewError::from)?,
    };
    Ok(row.is_some())
}

// ---------------------------------------------------------------------------
// The annotated write row (`ModuleViewSet.get_queryset().filter(pk=…)`)
// ---------------------------------------------------------------------------

/// Decode one `MODULE_WRITE_ROW`: the list projection plus `archived_at`.
fn decode_write_row(row: &tokio_postgres::Row) -> Result<ModuleRow, ViewError> {
    let mut module = base::decode_list_row(row)?;
    module.archived_at = row.try_get(28).map_err(ViewError::from)?;
    Ok(module)
}

async fn load_write_row(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    user_id: Uuid,
    module_id: Uuid,
) -> Result<Option<ModuleRow>, ViewError> {
    let statement = client
        .prepare_cached(queries::MODULE_WRITE_ROW.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&slug, &project_id, &user_id, &module_id])
        .await
        .map_err(ViewError::from)?;
    match row {
        Some(row) => Ok(Some(decode_write_row(&row)?)),
        None => Ok(None),
    }
}

/// The create response's `.values()` projection, in the Python's order
/// (`module/base.py:306-337`).
#[derive(Debug)]
struct CreateValue<'a>(&'a ModuleRow);

impl DrfSerialize for CreateValue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        write_module_fields(self.0, w, ctx, Order::Create)
    }
}

/// The PATCH response's `.values()` projection, in the Python's order
/// (`module/base.py:674-706`): the estimate sums and `is_favorite` move ahead
/// of the counts, and `total_issues` moves behind `started_issues`.
#[derive(Debug)]
struct PatchValue<'a>(&'a ModuleRow);

impl DrfSerialize for PatchValue<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        write_module_fields(self.0, w, ctx, Order::Patch)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Order {
    Create,
    Patch,
}

/// Both `.values()` dicts share every key and differ only in the computed
/// block's order.
fn write_module_fields(
    row: &ModuleRow,
    w: &mut DrfWriter,
    ctx: &RenderCtx<'_>,
    order: Order,
) -> Result<(), DrfError> {
    let timezone = *ctx.tz();
    let mut obj = w.begin_object();
    obj.field_dyn("id", ctx, &DrfUuid(row.id))?;
    obj.field_dyn("workspace_id", ctx, &DrfUuid(row.workspace_id))?;
    obj.field_dyn("project_id", ctx, &DrfUuid(row.project_id))?;
    obj.field("name", &row.name)?;
    obj.field("description", &row.description)?;
    obj.field_json(
        "description_text",
        row.description_text.as_ref().map(PgJson::from),
    )?;
    obj.field_json(
        "description_html",
        row.description_html.as_ref().map(PgJson::from),
    )?;
    obj.field("start_date", &row.start_date)?;
    obj.field("target_date", &row.target_date)?;
    obj.field("status", &row.status)?;
    obj.field("lead_id", &row.lead_id)?;
    obj.field("member_ids", &DrfUuidList(&row.member_ids))?;
    obj.field_json("view_props", row.view_props.as_ref().map(PgJson::from))?;
    obj.field("sort_order", &row.sort_order)?;
    obj.field("external_source", &row.external_source)?;
    obj.field("external_id", &row.external_id)?;
    obj.field_json("logo_props", row.logo_props.as_ref().map(PgJson::from))?;
    match order {
        Order::Create => {
            obj.field("is_favorite", &row.is_favorite.unwrap_or_default())?;
            obj.field(
                "cancelled_issues",
                &row.cancelled_issues.unwrap_or_default(),
            )?;
            obj.field(
                "completed_issues",
                &row.completed_issues.unwrap_or_default(),
            )?;
            obj.field("total_issues", &row.total_issues)?;
            obj.field("started_issues", &row.started_issues.unwrap_or_default())?;
            obj.field(
                "unstarted_issues",
                &row.unstarted_issues.unwrap_or_default(),
            )?;
            obj.field(
                "completed_estimate_points",
                &row.completed_estimate_points.unwrap_or_default(),
            )?;
            obj.field(
                "total_estimate_points",
                &row.total_estimate_points.unwrap_or_default(),
            )?;
            obj.field("backlog_issues", &row.backlog_issues.unwrap_or_default())?;
        }
        Order::Patch => {
            obj.field(
                "completed_estimate_points",
                &row.completed_estimate_points.unwrap_or_default(),
            )?;
            obj.field(
                "total_estimate_points",
                &row.total_estimate_points.unwrap_or_default(),
            )?;
            obj.field("is_favorite", &row.is_favorite.unwrap_or_default())?;
            obj.field(
                "cancelled_issues",
                &row.cancelled_issues.unwrap_or_default(),
            )?;
            obj.field(
                "completed_issues",
                &row.completed_issues.unwrap_or_default(),
            )?;
            obj.field("started_issues", &row.started_issues.unwrap_or_default())?;
            obj.field("total_issues", &row.total_issues)?;
            obj.field(
                "unstarted_issues",
                &row.unstarted_issues.unwrap_or_default(),
            )?;
            obj.field("backlog_issues", &row.backlog_issues.unwrap_or_default())?;
        }
    }
    obj.field(
        "created_at",
        &DrfDateTime(row.created_at.with_timezone(&timezone)),
    )?;
    obj.field(
        "updated_at",
        &DrfDateTime(row.updated_at.with_timezone(&timezone)),
    )?;
    obj.end();
    Ok(())
}

fn render_write_row(
    row: &ModuleRow,
    timezone: Tz,
    order: Order,
    status: u16,
) -> Result<ViewResponse, ViewError> {
    let bytes = match order {
        Order::Create => plane_serializers::render(&CreateValue(row), &RenderCtx::full(&timezone))?,
        Order::Patch => plane_serializers::render(&PatchValue(row), &RenderCtx::full(&timezone))?,
    };
    Ok(ViewResponse::raw(status, APPLICATION_JSON, bytes))
}

/// `json.dumps(ModuleSerializer(module).data, cls=DjangoJSONEncoder)` — the
/// PATCH activity's `current_instance` (`base.py:669`).
///
/// `ModuleSerializer` renders `created_at`/`updated_at` through DRF's
/// `DateTimeField`, i.e. an ISO-8601 **string** in the active (request user's)
/// timezone with microseconds; `json.dumps` then keeps it verbatim. Every
/// other field is a model attribute or an annotation `get_queryset()` adds, so
/// none is skipped.
fn current_instance_json(row: &ModuleRow, timezone: Tz) -> Result<String, ViewError> {
    let mut obj = PyDict::with_capacity(29);
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
    obj.insert(
        PyStr::new("description_text"),
        json_column(row.description_text.as_ref())?,
    );
    obj.insert(
        PyStr::new("description_html"),
        json_column(row.description_html.as_ref())?,
    );
    obj.insert(
        PyStr::new("start_date"),
        row.start_date
            .map_or(Json::Null, |date| Json::from(date.to_string())),
    );
    obj.insert(
        PyStr::new("target_date"),
        row.target_date
            .map_or(Json::Null, |date| Json::from(date.to_string())),
    );
    obj.insert(PyStr::new("status"), Json::from(row.status.as_str()));
    obj.insert(
        PyStr::new("lead_id"),
        row.lead_id
            .map_or(Json::Null, |id| Json::from(id.to_string())),
    );
    obj.insert(
        PyStr::new("member_ids"),
        Json::List(
            row.member_ids
                .iter()
                .map(|id| Json::from(id.to_string()))
                .collect(),
        ),
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
        PyStr::new("logo_props"),
        json_column(row.logo_props.as_ref())?,
    );
    obj.insert(
        PyStr::new("total_estimate_points"),
        Json::from(row.total_estimate_points.unwrap_or_default()),
    );
    obj.insert(
        PyStr::new("completed_estimate_points"),
        Json::from(row.completed_estimate_points.unwrap_or_default()),
    );
    obj.insert(
        PyStr::new("is_favorite"),
        Json::from(row.is_favorite.unwrap_or_default()),
    );
    obj.insert(PyStr::new("total_issues"), Json::from(row.total_issues));
    obj.insert(
        PyStr::new("cancelled_issues"),
        Json::from(row.cancelled_issues.unwrap_or_default()),
    );
    obj.insert(
        PyStr::new("completed_issues"),
        Json::from(row.completed_issues.unwrap_or_default()),
    );
    obj.insert(
        PyStr::new("started_issues"),
        Json::from(row.started_issues.unwrap_or_default()),
    );
    obj.insert(
        PyStr::new("unstarted_issues"),
        Json::from(row.unstarted_issues.unwrap_or_default()),
    );
    obj.insert(
        PyStr::new("backlog_issues"),
        Json::from(row.backlog_issues.unwrap_or_default()),
    );
    obj.insert(
        PyStr::new("created_at"),
        Json::from(DrfDateTime(row.created_at.with_timezone(&timezone)).to_drf_string()),
    );
    obj.insert(
        PyStr::new("updated_at"),
        Json::from(DrfDateTime(row.updated_at.with_timezone(&timezone)).to_drf_string()),
    );
    obj.insert(
        PyStr::new("archived_at"),
        row.archived_at.map_or(Json::Null, |value| {
            Json::from(DrfDateTime(value.with_timezone(&timezone)).to_drf_string())
        }),
    );
    dumps(&Json::Dict(obj))
}

// ---------------------------------------------------------------------------
// Body validation
// ---------------------------------------------------------------------------

/// `ModuleWriteSerializer`'s `validate()` (`serializers/module.py:55-62`):
/// `ValidationError("Start date cannot exceed target date")` is a non-field
/// error and only runs when `to_internal_value` succeeded.
fn validate_dates(serializer: &Serializer<'_>) -> Option<ViewResponse> {
    let Some(write::Value::Date(start)) = serializer.value("start_date") else {
        return None;
    };
    let Some(write::Value::Date(target)) = serializer.value("target_date") else {
        return None;
    };
    (start > target).then(|| {
        write::field_string_error("non_field_errors", "Start date cannot exceed target date")
    })
}

/// The `lead` column: `lead_id` (declared first) and `lead` (the model
/// relation) both write `validated_data["lead"]`; DRF visits `lead_id` first,
/// so a body that carries both ends on the later `lead` field's value.
fn lead_value(values: &Validated) -> Option<Option<Uuid>> {
    if values.contains("lead") {
        Some(values.uuid("lead"))
    } else if values.contains("lead_id") {
        Some(values.uuid("lead_id"))
    } else {
        None
    }
}

/// A nullable field's validated value: `None` = absent, `Some(None)` = `null`.
fn nullable<T>(values: &Validated, name: &str, value: Option<T>) -> Option<Option<T>> {
    values.contains(name).then_some(value)
}

// ---------------------------------------------------------------------------
// Activity
// ---------------------------------------------------------------------------

async fn publish_model_activity(
    ctx: &ViewCtx<'_>,
    slug: &str,
    module_id: Uuid,
    current_instance: Option<String>,
    user: Uuid,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::MODEL_ACTIVITY;
    let message = tasks::model_activity(ModelActivityCall {
        model_name: "module".to_owned(),
        model_id: module_id.to_string(),
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
// Route 103 — create
// ---------------------------------------------------------------------------

/// `ModuleViewSet.create` (`module/base.py:295-352`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let project = load_project(client, &params.slug, params.project_id).await?;

        let mut serializer = Serializer::new(ctx, false)?;
        module_write_serializer::validate(&mut serializer, client).await?;
        if !serializer.has_errors()
            && let Some(response) = validate_dates(&serializer)
        {
            return Ok(response);
        }
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        let name = values
            .text("name")
            .ok_or_else(|| ViewError::Unknown("validated name vanished".to_owned()))?
            .to_owned();
        if duplicate_name(client, &name, params.project_id, None).await? {
            return Ok(write::error_body(
                400,
                "Module with this name already exists",
            ));
        }

        // H21: `sort_order` is rewritten only when the project already has a
        // live module (`plane/db/models/module.py:115-124`).
        let requested_sort_order = values.float("sort_order").unwrap_or(65535.0);
        let sort_order =
            hooks::module::before_insert(client, params.project_id, requested_sort_order)
                .await
                .map_err(ViewError::from)?;

        let now = write::now();
        let audit = hooks::audit::Audit::on_create(Some(params.user_id));
        let module = ModuleModel {
            created_at: now,
            updated_at: now,
            created_by_id: audit.created_by_id,
            updated_by_id: audit.updated_by_id,
            deleted_at: None,
            id: Uuid::new_v4(),
            project_id: params.project_id,
            workspace_id: project.workspace_id,
            name,
            description: values.text("description").unwrap_or_default().to_owned(),
            description_text: values.json("description_text").cloned(),
            description_html: values.json("description_html").cloned(),
            start_date: values.date("start_date"),
            target_date: values.date("target_date"),
            status: values.text("status").unwrap_or("planned").to_owned(),
            lead_id: lead_value(&values).flatten(),
            view_props: values
                .json("view_props")
                .cloned()
                .unwrap_or_else(empty_object),
            sort_order,
            external_source: values.text("external_source").map(str::to_owned),
            external_id: values.text("external_id").map(str::to_owned),
            archived_at: None,
            logo_props: values
                .json("logo_props")
                .cloned()
                .unwrap_or_else(empty_object),
        };
        client
            .execute(
                queries::MODULE_INSERT.sql,
                &[
                    &module.created_at,
                    &module.updated_at,
                    &module.created_by_id,
                    &module.updated_by_id,
                    &module.deleted_at,
                    &module.id,
                    &module.project_id,
                    &module.workspace_id,
                    &module.name,
                    &module.description,
                    &module.description_text,
                    &module.description_html,
                    &module.start_date,
                    &module.target_date,
                    &module.status,
                    &module.lead_id,
                    &module.view_props,
                    &module.sort_order,
                    &module.external_source,
                    &module.external_id,
                    &module.archived_at,
                    &module.logo_props,
                ],
            )
            .await
            .map_err(integrity)?;

        // `ModuleMember.objects.bulk_create(…, ignore_conflicts=True)` — no
        // `save()`, so the audit columns and the workspace are passed
        // explicitly (`serializers/module.py:76-90`).
        if let Some(members) = values.uuid_list("member_ids") {
            insert_members(
                client,
                &module,
                members,
                module.created_by_id,
                module.updated_by_id,
            )
            .await?;
        }

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let Some(row) = load_write_row(
            client,
            &params.slug,
            params.project_id,
            params.user_id,
            module.id,
        )
        .await?
        else {
            // `module["id"]` on the `None` the default manager returns.
            return Err(ViewError::Unknown(
                "TypeError: 'NoneType' object is not subscriptable".to_owned(),
            ));
        };
        publish_model_activity(ctx, &params.slug, module.id, None, params.user_id).await?;
        render_write_row(&row, timezone, Order::Create, 201)
    })
}

/// The `bulk_create` of `ModuleMember` rows. Every row carries its own
/// `created_at`/`updated_at` (`auto_now_add`/`auto_now` per row in
/// `_batched_insert`), and `ON CONFLICT DO NOTHING` is
/// `ignore_conflicts=True`.
async fn insert_members(
    client: &Client,
    module: &ModuleModel,
    members: &[Uuid],
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
) -> Result<(), ViewError> {
    if members.is_empty() {
        return Ok(());
    }
    let mut created = Vec::with_capacity(members.len());
    let mut updated = Vec::with_capacity(members.len());
    let mut ids = Vec::with_capacity(members.len());
    for _ in members {
        created.push(write::now());
        updated.push(write::now());
        ids.push(Uuid::new_v4());
    }
    client
        .execute(
            queries::MODULE_MEMBER_INSERT.sql,
            &[
                &created_by,
                &updated_by,
                &module.project_id,
                &module.workspace_id,
                &module.id,
                &created,
                &updated,
                &ids,
                &members,
            ],
        )
        .await
        .map_err(integrity)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Route 104 — partial_update, destroy
// ---------------------------------------------------------------------------

/// `ModuleViewSet.partial_update` (`module/base.py:652-722`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let pk = base::uuid_kwarg(ctx, "pk")?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);

        // `module_queryset.first()` (`:656`); the view's own 404 guard.
        let Some(row) =
            load_write_row(client, &params.slug, params.project_id, params.user_id, pk).await?
        else {
            return Ok(write::error_body(404, "Module not found"));
        };
        if row.archived_at.is_some() {
            return Ok(write::error_body(400, "Archived module cannot be updated"));
        }
        let current_instance = current_instance_json(&row, timezone)?;

        let mut serializer = Serializer::new(ctx, true)?;
        module_write_serializer::validate(&mut serializer, client).await?;
        if !serializer.has_errors()
            && let Some(response) = validate_dates(&serializer)
        {
            return Ok(response);
        }
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `ModuleWriteSerializer.update` (`serializers/module.py:94-120`).
        if let Some(name) = values.text("name")
            && duplicate_name(client, name, row.project_id, Some(pk)).await?
        {
            return Ok(write::error_body(
                400,
                "Module with this name already exists",
            ));
        }

        let statement = client
            .prepare_cached(queries::MODULE_MODEL_ROW.sql)
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
        let mut module = ModuleModel::from_row(&stored, 0).map_err(ViewError::from)?;

        // Members are replaced **before** the row update, and the H04 delete
        // carries no `updated_at` and no cascade task
        // (`serializers/module.py:102-118`).
        if values.contains("member_ids") {
            client
                .execute(
                    queries::MODULE_MEMBER_SOFT_DELETE.sql,
                    &[&write::now(), &module.id],
                )
                .await
                .map_err(ViewError::from)?;
            insert_members(
                client,
                &module,
                values.uuid_list("member_ids").unwrap_or_default(),
                module.created_by_id,
                module.updated_by_id,
            )
            .await?;
        }

        if let Some(name) = values.text("name") {
            module.name = name.to_owned();
        }
        if let Some(description) = values.text("description") {
            module.description = description.to_owned();
        }
        if let Some(value) = nullable(
            &values,
            "description_text",
            values.json("description_text").cloned(),
        ) {
            module.description_text = value;
        }
        if let Some(value) = nullable(
            &values,
            "description_html",
            values.json("description_html").cloned(),
        ) {
            module.description_html = value;
        }
        if let Some(value) = nullable(&values, "start_date", values.date("start_date")) {
            module.start_date = value;
        }
        if let Some(value) = nullable(&values, "target_date", values.date("target_date")) {
            module.target_date = value;
        }
        if let Some(status) = values.text("status") {
            module.status = status.to_owned();
        }
        if let Some(value) = lead_value(&values) {
            module.lead_id = value;
        }
        if let Some(view_props) = values.json("view_props") {
            module.view_props = view_props.clone();
        }
        if let Some(sort_order) = values.float("sort_order") {
            module.sort_order = sort_order;
        }
        if let Some(value) = nullable(
            &values,
            "external_source",
            values.text("external_source").map(str::to_owned),
        ) {
            module.external_source = value;
        }
        if let Some(value) = nullable(
            &values,
            "external_id",
            values.text("external_id").map(str::to_owned),
        ) {
            module.external_id = value;
        }
        if let Some(logo_props) = values.json("logo_props") {
            module.logo_props = logo_props.clone();
        }
        // H01: only `updated_by`/`updated_at` change on an update.
        module.updated_at = write::now();
        module.updated_by_id = Some(params.user_id);

        client
            .execute(
                queries::MODULE_UPDATE.sql,
                &[
                    &module.created_at,
                    &module.updated_at,
                    &module.created_by_id,
                    &module.updated_by_id,
                    &module.deleted_at,
                    &module.id,
                    &module.project_id,
                    &module.workspace_id,
                    &module.name,
                    &module.description,
                    &module.description_text,
                    &module.description_html,
                    &module.start_date,
                    &module.target_date,
                    &module.status,
                    &module.lead_id,
                    &module.view_props,
                    &module.sort_order,
                    &module.external_source,
                    &module.external_id,
                    &module.archived_at,
                    &module.logo_props,
                    &module.id,
                ],
            )
            .await
            .map_err(integrity)?;

        let Some(row) =
            load_write_row(client, &params.slug, params.project_id, params.user_id, pk).await?
        else {
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
        render_write_row(&row, timezone, Order::Patch, 200)
    })
}

/// `ModuleViewSet.destroy` (`module/base.py:724-760`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let pk = base::uuid_kwarg(ctx, "pk")?;

        let statement = client
            .prepare_cached(queries::MODULE_DESTROY_LOOKUP.sql)
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
            .prepare_cached(queries::MODULE_ISSUE_IDS.sql)
            .await
            .map_err(ViewError::from)?;
        let issues = client
            .query(&issues_statement, &[&pk])
            .await
            .map_err(ViewError::from)?
            .iter()
            .map(|row| -> Result<Uuid, ViewError> { row.try_get(0).map_err(ViewError::from) })
            .collect::<Result<Vec<Uuid>, _>>()?;

        // One activity per live module issue, published **before** any delete
        // (`base.py:729-742`). `issue_id` is `str(issue)`; `project_id` is the
        // URL's UUID object (TASKS.md row 57).
        let mut requested = PyDict::with_capacity(1);
        requested.insert(PyStr::new("module_id"), Json::from(pk.to_string()));
        let requested_data = dumps(&Json::Dict(requested))?;
        let mut current = PyDict::with_capacity(1);
        current.insert(PyStr::new("module_name"), Json::from(name.as_str()));
        let current_instance = dumps(&Json::Dict(current))?;
        let origin = origin(ctx)?;
        for issue in &issues {
            let activity = tasks::issue_activity(IssueActivityCall::view(
                IssueActivityBase::new(
                    "module.activity.deleted",
                    TaskArg::json_text(requested_data.clone()),
                    params.user_id.to_string(),
                    TaskArg::str(issue.to_string()),
                    TaskArg::uuid(params.project_id),
                    TaskArg::json_text(current_instance.clone()),
                    epoch(),
                ),
                origin.clone(),
            ));
            crate::common::jobs::dispatch_task(
                ctx,
                activity.task(),
                &activity,
                plane_jobs::queue::Job::new(activity.task()),
            )
            .await?;
        }

        // `module.delete()` — H03: two `timezone.now()` calls (the attribute
        // and `auto_now`), then the cascade task.
        let deleted_at = write::now();
        let updated_at = write::now();
        client
            .execute(
                queries::MODULE_SOFT_DELETE.sql,
                &[&deleted_at, &updated_at, &Some(params.user_id), &pk],
            )
            .await
            .map_err(ViewError::from)?;
        write::publish_soft_delete_cascade(ctx, "db", "module", pk).await?;

        client
            .execute(
                queries::MODULE_ISSUE_SOFT_DELETE.sql,
                &[&write::now(), &pk, &params.project_id],
            )
            .await
            .map_err(ViewError::from)?;
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
// Route 112 — user properties
// ---------------------------------------------------------------------------

/// `request.data.get(key, current)` for a `JSONField(null=False)` column: an
/// explicit `null` is the `IntegrityError` `save()` raises.
fn apply_json_field(data: &Data, stored: &mut JsonText, key: &str) -> Result<(), ViewError> {
    match data {
        Data::Json(Json::Dict(map)) => {
            if let Some(value) = map.get(key) {
                if value.is_null() {
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

/// `ModuleUserPropertiesEndpoint.patch` (`module/base.py:836-855`).
pub fn user_properties_patch<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let module_id = base::uuid_kwarg(ctx, "module_id")?;

        let statement = client
            .prepare_cached(crate::modules::queries::MODULE_USER_PROPERTIES_BY_KEY.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(
                &statement,
                &[
                    &params.user_id,
                    &params.project_id,
                    &module_id,
                    &params.slug,
                ],
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
                queries::MODULE_USER_PROPERTIES_UPDATE.sql,
                &[
                    &created_at,
                    &updated_at,
                    &created_by,
                    &Some(params.user_id),
                    &deleted_at,
                    &id,
                    &params.project_id,
                    &workspace_id,
                    &module_id,
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

        let filters = filters.into_string();
        let display_filters = display_filters.into_string();
        let display_properties = display_properties.into_string();
        let rich_filters = rich_filters.into_string();
        let properties = ModuleUserPropertiesSerializer {
            id,
            created_at,
            updated_at,
            deleted_at,
            filters: Some(PgJson::new(&filters)),
            display_filters: Some(PgJson::new(&display_filters)),
            display_properties: Some(PgJson::new(&display_properties)),
            rich_filters: Some(PgJson::new(&rich_filters)),
            created_by,
            updated_by: Some(params.user_id),
            project: params.project_id,
            workspace: workspace_id,
            module: module_id,
            user: user_id,
        };
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let bytes = plane_serializers::render(&properties, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, bytes))
    })
}

// ---------------------------------------------------------------------------
// Routes 113/114/115 — archive/unarchive
// ---------------------------------------------------------------------------

fn archive_module_id(ctx: &ViewCtx<'_>, method: &str) -> Result<Uuid, ViewError> {
    let Some(raw) = ctx.kwarg("module_id") else {
        let message = if ctx.kwarg("pk").is_some() {
            format!(
                "TypeError: ModuleArchiveUnarchiveEndpoint.{method}() got an unexpected keyword \
                 argument 'pk'"
            )
        } else {
            format!(
                "TypeError: ModuleArchiveUnarchiveEndpoint.{method}() missing 1 required \
                 positional argument: 'module_id'"
            )
        };
        return Err(ViewError::Unknown(message));
    };
    Uuid::parse_str(raw).map_err(|_| ViewError::Validation)
}

async fn archive_lookup(
    client: &Client,
    params: &crate::cycles::base::Params,
    module_id: Uuid,
) -> Result<Option<(String, Option<DateTime<Utc>>)>, ViewError> {
    let statement = client
        .prepare_cached(queries::MODULE_ARCHIVE_LOOKUP.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&module_id, &params.project_id, &params.slug])
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

/// `ModuleArchiveUnarchiveEndpoint.post` (`archive.py:544-559`), plus the
/// unconditional 500 the same method answers on routes 114/115.
pub fn archive_post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let module_id = archive_module_id(ctx, "post")?;
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let Some((status, _archived_at)) = archive_lookup(client, &params, module_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if status != "completed" && status != "cancelled" {
            return Ok(write::error_body(
                400,
                "Only completed or cancelled modules can be archived",
            ));
        }
        // `module.archived_at = timezone.now(); module.save()`: the attribute
        // and `auto_now` are two calls; H01 sets `updated_by`.
        let archived_at = write::now();
        let updated_at = write::now();
        client
            .execute(
                queries::MODULE_SET_ARCHIVED.sql,
                &[&archived_at, &updated_at, &Some(params.user_id), &module_id],
            )
            .await
            .map_err(ViewError::from)?;
        // H04 for **every** user (`archive.py:553-558`).
        client
            .execute(
                queries::USER_FAVORITE_SOFT_DELETE_ALL.sql,
                &[&write::now(), &module_id, &params.project_id, &params.slug],
            )
            .await
            .map_err(ViewError::from)?;
        Ok(ViewResponse::json(
            200,
            serde_json::json!({"archived_at": python_str_datetime(archived_at)}),
        ))
    })
}

/// `ModuleArchiveUnarchiveEndpoint.delete` (`archive.py:561-565`), plus the
/// unconditional 500 the same method answers on routes 114/115.
pub fn archive_delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let module_id = archive_module_id(ctx, "delete")?;
        let client = client(ctx)?;
        let params = base::params(ctx)?;
        let Some((_status, _archived_at)) = archive_lookup(client, &params, module_id).await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let updated_at = write::now();
        client
            .execute(
                queries::MODULE_CLEAR_ARCHIVED.sql,
                &[&updated_at, &Some(params.user_id), &module_id],
            )
            .await
            .map_err(ViewError::from)?;
        Ok(ViewResponse::empty(204))
    })
}
