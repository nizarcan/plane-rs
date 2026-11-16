// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IssueLinkViewSet`'s write actions (`plane/app/views/issue/link.py:56-126`).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/issue-links/` | `create` | `:56-79` |
//! | `PATCH …/issue-links/{pk}/` | `partial_update` | `:81-111` |
//! | `DELETE …/issue-links/{pk}/` | `destroy` | `:113-126` |
//!
//! `PUT` is refused before dispatch by `BaseViewSet`'s guard (`routes.json`
//! 75's `guard_actions.update`).
//!
//! ## What Django does per action
//!
//! **create** binds the URL issue with `Issue.objects.get(pk, workspace__slug,
//! project_id)` — the soft-delete manager, so archived/draft/triage issues are
//! reachable; a miss is `ObjectDoesNotExist`, which the family
//! `handle_exception` renders as 404 `{"error": "The required object does not
//! exist."}`. The serializer then
//!
//! 1. prefixes `http://` when the **raw** `url` does not already start with
//!    `http://`/`https://` (`to_internal_value`, `:577-583`),
//! 2. runs `CharField`, then `validate_url`'s `URLValidator` (`:585-592`);
//!    the field error is `{"url": {"error": "Invalid URL format."}}` — a dict
//!    body, not a message list, because `validate_<field>` raised
//!    `ValidationError(dict)`,
//! 3. `create()` rejects a duplicate `(url, issue_id)` with a bare
//!    `{"error": "URL already exists for this Issue"}` (`:594-599`).
//!
//! `objects.create` runs `ProjectBaseModel.save` (workspace from the project)
//! and `BaseModel.save` (`created_by` = the request user, `updated_by` null).
//! Then `crawl_work_item_link_title.delay(id, url)` (`:62`), the
//! `link.activity.created` task with the serialized link as `requested_data`
//! (`:63-72`), and the response is the row re-read through `get_queryset()`
//! (`:74-77`) — so a link created in an **archived project** is 404 after it
//! was written.
//!
//! **partial_update** reads the row from the default manager (no membership
//! or archived-project filter, so a miss is the 404 above), snapshots
//! `requested_data = json.dumps(request.data)` and
//! `current_instance = json.dumps(IssueLinkSerializer(issue_link).data)` —
//! both *before* validation, so the prefix the serializer applies to `url` is
//! not in them. The duplicate check in `update()` excludes the row itself.
//! The crawl task re-runs only when the URL changed (`:90-92`).
//!
//! **destroy** publishes `link.activity.deleted` with
//! `requested_data=json.dumps({"link_id": str(pk)})` and the pre-delete
//! `current_instance`, then soft-deletes (H03: `deleted_at`, `updated_at` and
//! `updated_by` stamped, plus the cascade task).
//!
//! ## Writable fields
//!
//! `IssueLinkSerializer` is `fields = "__all__"` with `workspace`, `project`,
//! `created_by`, `updated_by`, `created_at`, `updated_at` and `issue`
//! read-only (`plane/app/serializers/issue.py:560-575`), so the body can set
//! `deleted_at`, `title`, `url` and `metadata` — `deleted_at` included, which
//! lets a client soft-delete a link with a PATCH.

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::JsonText;
use plane_db::hooks;
use plane_db::models::issue::IssueLink;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::tasks::{self, IssueActivityBase, IssueActivityCall};
use plane_query::json::Json;
use plane_serializers::PgJson;
use plane_serializers::generated::app_issue::IssueLinkSerializer;
use plane_serializers::generated::app_user::UserLiteSerializer;
use uuid::Uuid;

use super::lists::list_common::{error_body, kwarg_uuid, user_timezone};
use super::sub_resources::sub_common::{self as shared, UserLiteRow};
use super::sub_resources::sub_queries as read_queries;
use super::write_sub_queries as queries;
use crate::common::{self, client, jobs};
use crate::write;
use crate::write::Serializer;
use crate::write::fields::{self, CharOptions, Detail, Raw};

/// Register the three actions on the view that already serves `list` and
/// `retrieve` (`sub_links.rs`).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.link.IssueLinkViewSet")?
        .action("create", create)?
        .action("partial_update", partial_update)?
        .action("destroy", destroy)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// create
// ---------------------------------------------------------------------------

pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let user = shared::user_id(ctx)?;
        let timezone = user_timezone(ctx);
        let now = write::now();

        // `Issue.objects.get(pk=issue_id, workspace__slug=slug,
        // project_id=project_id)` (`:58`).
        let statement = queries::ISSUE_ANY_IN_PROJECT.prepare(client).await?;
        let exists: bool = client
            .query_one(&statement, &[&issue_id, &project_id, &slug])
            .await?
            .get(0);
        if !exists {
            return Err(ViewError::ObjectDoesNotExist);
        }

        let data = ctx.data()?;
        let mut serializer = Serializer::new(ctx, false)?;
        let values = match read_body(&mut serializer, data, false)? {
            Some(values) => match serializer.finish() {
                Ok(_) => values,
                Err(errors) => return Ok(errors.into_response()),
            },
            None => return Ok(serializer_error(serializer)),
        };

        // `IssueLinkSerializer.create`'s duplicate check (`:594-599`).
        let statement = queries::LINK_URL_TAKEN.prepare(client).await?;
        let taken: bool = client
            .query_one(&statement, &[&values.url, &issue_id])
            .await?
            .get(0);
        if taken {
            return Ok(error_body(400, "URL already exists for this Issue"));
        }

        // `Issue.objects.create(**validated_data)` + `ProjectBaseModel.save`
        // (`:59`).
        let workspace_id = project_workspace(client, project_id).await?;
        let link_id = Uuid::new_v4();
        let metadata = values
            .metadata
            .clone()
            .unwrap_or_else(|| JsonText::new("{}"));
        let statement = queries::LINK_INSERT.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &Some(user),
                    &None::<Uuid>,
                    &values.deleted_at.flatten(),
                    &link_id,
                    &project_id,
                    &workspace_id,
                    &values.title.clone().flatten(),
                    &values.url,
                    &issue_id,
                    &metadata,
                ],
            )
            .await?;

        // The in-memory serializer's `data` for `requested_data`: the row as
        // created, with the body's `metadata` insertion order (a `jsonb`
        // round trip would re-order the keys, and Django does not re-read it
        // before `json.dumps`).
        let created = load_by_pk(client, &slug, project_id, issue_id, link_id)
            .await?
            .ok_or_else(|| {
                ViewError::Unknown("the link vanished between the write and the read".to_owned())
            })?;
        let metadata_in_memory = metadata.clone();
        let requested_data = plane_jobs::celery::args::TaskArg::json_text(queries::django_json(
            &link_json(&created, timezone, Some(&metadata_in_memory)),
        )?);

        // `crawl_work_item_link_title.delay(serializer.data.get("id"),
        // serializer.data.get("url"))` (`:62`).
        // `serializer.data.get("id")` is a `uuid.UUID`: `IssueLinkSerializer.id`
        // is DRF's `ReadOnlyField` (the model pk is `editable=False`), which
        // returns the value as-is, so kombu's `uuid` envelope is what reaches
        // the wire.
        publish_crawl(
            ctx,
            plane_jobs::celery::args::TaskArg::uuid(link_id),
            values
                .url
                .clone()
                .expect("a created link always has a validated url"),
        )
        .await?;

        // `issue_activity.delay(type="link.activity.created", …)` (`:63-72`).
        publish_activity(
            ctx,
            "link.activity.created",
            requested_data,
            plane_jobs::celery::args::TaskArg::Null,
            now,
            issue_id,
            project_id,
            user,
        )
        .await?;

        // `self.get_queryset().get(id=serializer.data.get("id"))` (`:74`) —
        // membership and `archived_at IS NULL` on the project.
        let Some(row) =
            load_response_row(client, &slug, project_id, issue_id, user, link_id).await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let entry = LinkEntry {
            link: &row.link,
            created_by: row.created_by.as_ref().map(UserLiteRow::serializer),
        };
        let body = common::render(ctx, &entry.serializer())?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// partial_update
// ---------------------------------------------------------------------------

pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = shared::user_id(ctx)?;
        let timezone = user_timezone(ctx);
        let now = write::now();

        // `IssueLink.objects.get(workspace__slug=…, project_id=…,
        // issue_id=…, pk=…)` (`:82`).
        let Some(previous) = load_by_pk(client, &slug, project_id, issue_id, pk).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        // `requested_data = json.dumps(request.data)` and
        // `current_instance = json.dumps(IssueLinkSerializer(issue_link).data)`
        // (`:84-85`), both before validation.
        let requested_data = crate::write::request_data_json_text(ctx)?;
        let current_instance = queries::django_json(&link_json(&previous, timezone, None))?;

        let data = ctx.data()?;
        let mut serializer = Serializer::new(ctx, true)?;
        let values = match read_body(&mut serializer, data, true)? {
            Some(values) => match serializer.finish() {
                Ok(_) => values,
                Err(errors) => return Ok(errors.into_response()),
            },
            None => return Ok(serializer_error(serializer)),
        };

        // `IssueLinkSerializer.update`'s duplicate check (`:601-609`): the URL
        // only when the body carries one, excluding this row.
        if let Some(url) = values.url.as_deref() {
            let statement = queries::LINK_URL_TAKEN_ELSEWHERE.prepare(client).await?;
            let taken: bool = client
                .query_one(&statement, &[&url, &issue_id, &pk])
                .await?
                .get(0);
            if taken {
                return Ok(error_body(400, "URL already exists for this Issue"));
            }
        }

        // `serializer.save()` → `instance.save()`: every writable field is
        // set from `validated_data` (absent keys keep the row's value), then
        // `BaseModel.save` stamps `updated_by` and `auto_now` (`:88`).
        let title = match &values.title {
            Some(title) => title.clone(),
            None => previous.link.title.clone(),
        };
        let url = values
            .url
            .clone()
            .unwrap_or_else(|| previous.link.url.clone());
        let metadata = match &values.metadata {
            Some(metadata) => metadata.clone(),
            None => previous.link.metadata.clone(),
        };
        let deleted_at = match &values.deleted_at {
            Some(deleted_at) => *deleted_at,
            None => previous.link.deleted_at,
        };
        let statement = queries::LINK_UPDATE.prepare(client).await?;
        client
            .execute(
                &statement,
                &[&title, &url, &metadata, &deleted_at, &now, &Some(user), &pk],
            )
            .await?;

        // `updated_url = serializer.data.get("url")`; a changed URL is
        // re-crawled (`:89-92`).
        if url != previous.link.url {
            publish_crawl(
                ctx,
                plane_jobs::celery::args::TaskArg::uuid(pk),
                url.clone(),
            )
            .await?;
        }

        publish_activity(
            ctx,
            "link.activity.updated",
            requested_data,
            plane_jobs::celery::args::TaskArg::json_text(current_instance),
            now,
            issue_id,
            project_id,
            user,
        )
        .await?;

        // `self.get_queryset().get(id=…)` (`:106`) — a PATCH that sets
        // `deleted_at` makes this 404 after the activity, as in Django.
        let Some(row) = load_response_row(client, &slug, project_id, issue_id, user, pk).await?
        else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let entry = LinkEntry {
            link: &row.link,
            created_by: row.created_by.as_ref().map(UserLiteRow::serializer),
        };
        let body = common::render(ctx, &entry.serializer())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// destroy
// ---------------------------------------------------------------------------

pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = shared::user_id(ctx)?;
        let timezone = user_timezone(ctx);
        let now = write::now();

        let Some(row) = load_by_pk(client, &slug, project_id, issue_id, pk).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let current_instance = queries::django_json(&link_json(&row, timezone, None))?;

        // `issue_activity.delay(type="link.activity.deleted",
        // requested_data=json.dumps({"link_id": str(pk)}), …)` (`:115-124`).
        let requested = Json::object(vec![("link_id", Json::uuid(pk))]);
        let requested =
            plane_jobs::celery::args::TaskArg::json_text(queries::django_json(&requested)?);
        let base = IssueActivityBase::new(
            "link.activity.deleted",
            requested,
            user.to_string(),
            plane_jobs::celery::args::TaskArg::str(issue_id.to_string()),
            plane_jobs::celery::args::TaskArg::str(project_id.to_string()),
            plane_jobs::celery::args::TaskArg::json_text(current_instance),
            now.timestamp(),
        );
        let message =
            tasks::issue_activity(IssueActivityCall::view(base, queries::base_host(ctx)?));
        let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
        jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task)).await?;

        // `issue_link.delete()` — H03.
        hooks::soft_delete::delete_instance(client, "issue_links", "id", pk, Some(user), now)
            .await?;
        write::publish_soft_delete_cascade(ctx, "db", "issuelink", pk).await?;

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// The body
// ---------------------------------------------------------------------------

/// `IssueLinkSerializer`'s writable fields (`plane/app/serializers/issue.py:
/// 560-575`), in `self.fields` order.
#[derive(Debug, Default)]
struct LinkValues {
    /// `deleted_at`: `Some(None)` is an explicit `null`.
    deleted_at: Option<Option<DateTime<Utc>>>,
    title: Option<Option<String>>,
    url: Option<String>,
    metadata: Option<JsonText>,
}

/// Run `Serializer` over the body in field order. `None` means a field error
/// was recorded (the caller renders `serializer.errors`).
///
/// The `url` is read by hand because `to_internal_value` rewrites the **raw**
/// value before `CharField` and then runs `URLValidator`, neither of which a
/// bare `char_field` call can reproduce; every other field goes through the
/// shared parsers.
// The field parsers have to run in serializer order and each records its own
// value and error; a struct initializer would reorder the calls.
#[allow(clippy::field_reassign_with_default)]
fn read_body(
    serializer: &mut Serializer<'_>,
    data: &Data,
    partial: bool,
) -> Result<Option<LinkValues>, ViewError> {
    let mut values = LinkValues::default();

    // `deleted_at`: `DateTimeField(required=False, allow_null=True)`.
    values.deleted_at = tri_state(
        data,
        "deleted_at",
        serializer.datetime_field("deleted_at", true),
    );

    // `title`: `CharField(max_length=255, allow_null=True, allow_blank=True)`.
    values.title = tri_state(
        data,
        "title",
        serializer.char_field(
            "title",
            CharOptions {
                allow_null: true,
                allow_blank: true,
                trim_whitespace: true,
                max_length: Some(255),
            },
        ),
    );

    // `url` + `to_internal_value`'s prefix + `validate_url`.
    values.url = read_url(serializer, data, partial)?;

    // `metadata`: `JSONField(required=False)`.
    let metadata = serializer.json_field("metadata", false);
    if !matches!(
        queries::raw_field(data, "metadata"),
        queries::RawField::Absent | queries::RawField::Null
    ) {
        values.metadata = metadata;
    }

    if serializer.has_errors() {
        return Ok(None);
    }
    Ok(Some(values))
}

/// `(absent, explicit null, value)` from the raw body and the parser's result.
fn tri_state<T>(data: &Data, name: &str, parsed: Option<T>) -> Option<Option<T>> {
    match queries::raw_field(data, name) {
        queries::RawField::Absent => None,
        queries::RawField::Null => Some(None),
        queries::RawField::Text(_) | queries::RawField::Other(_) => parsed.map(Some),
    }
}

/// `IssueLinkSerializer.to_internal_value` + `validate_url`
/// (`plane/app/serializers/issue.py:577-592`).
fn read_url(
    serializer: &mut Serializer<'_>,
    data: &Data,
    partial: bool,
) -> Result<Option<String>, ViewError> {
    let prefixed = match queries::raw_field(data, "url") {
        queries::RawField::Absent => {
            if !partial && serializer.missing_required("url") {
                serializer.push_error("url", Detail::required());
            }
            return Ok(None);
        }
        queries::RawField::Null => {
            serializer.push_error("url", Detail::null());
            return Ok(None);
        }
        queries::RawField::Text(value) => {
            if value.is_empty() || value.starts_with("http://") || value.starts_with("https://") {
                value.to_owned()
            } else {
                format!("http://{value}")
            }
        }
        queries::RawField::Other(other) => {
            // `data.get("url", "")` then `.startswith(...)` on a non-string:
            // `AttributeError`, which the family handler renders as 500.
            return Err(ViewError::Unknown(format!(
                "'{}' object has no attribute 'startswith'",
                python_type_name(other)
            )));
        }
    };
    let options = CharOptions {
        allow_null: false,
        allow_blank: false,
        trim_whitespace: true,
        max_length: None,
    };
    match fields::char(Raw::Text(&prefixed), &options) {
        Ok(value) => {
            if django_url_valid(&value) {
                Ok(Some(value))
            } else {
                serializer.push_body(
                    "url",
                    Json::object(vec![("error", Json::string("Invalid URL format."))]),
                );
                Ok(None)
            }
        }
        Err(detail) => {
            serializer.push_error("url", detail);
            Ok(None)
        }
    }
}

fn python_type_name(value: &plane_serde::pyjson::Value) -> &'static str {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Null => "NoneType",
        Py::Bool(_) => "bool",
        Py::Int(_) => "int",
        Py::Float(_) => "float",
        Py::Str(_) => "str",
        Py::List(_) => "list",
        Py::Dict(_) => "dict",
    }
}

/// The 400 body when a field error was recorded before `finish()`.
fn serializer_error(serializer: Serializer<'_>) -> ViewResponse {
    match serializer.finish() {
        Ok(_) => ViewResponse::empty(500),
        Err(errors) => errors.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Statements / rendering
// ---------------------------------------------------------------------------

struct LinkRow {
    link: IssueLink,
    created_by: Option<UserLiteRow>,
}

/// The row plus its nested serializer, so the outer serializer can borrow it.
struct LinkEntry<'a> {
    link: &'a IssueLink,
    created_by: Option<UserLiteSerializer<'a>>,
}

impl LinkRow {
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        let link = IssueLink::from_row(row, 0)
            .map_err(|error| ViewError::Unknown(format!("decoding the link row: {error}")))?;
        let created_by = UserLiteRow::from_row(row, IssueLink::COLUMN_COUNT)
            .map_err(|error| ViewError::Unknown(format!("decoding created_by: {error}")))?;
        Ok(Self { link, created_by })
    }
}

impl<'a> LinkEntry<'a> {
    fn serializer(&self) -> IssueLinkSerializer<'_> {
        IssueLinkSerializer {
            id: self.link.id,
            created_by_detail: self.created_by.as_ref(),
            created_at: self.link.created_at,
            updated_at: self.link.updated_at,
            deleted_at: self.link.deleted_at,
            title: self.link.title.as_deref(),
            url: &self.link.url,
            metadata: Some(PgJson::from_json_text(&self.link.metadata)),
            created_by: self.link.created_by_id,
            updated_by: self.link.updated_by_id,
            project: self.link.project_id,
            workspace: self.link.workspace_id,
            issue: self.link.issue_id,
        }
    }
}

/// `IssueLinkSerializer(issue_link).data` as a document, in `_readable_fields`
/// order. [`queries::django_json`] turns it into the `current_instance` /
/// `requested_data` string.
fn link_json(row: &LinkRow, timezone: Tz, metadata: Option<&JsonText>) -> Json {
    let link = &row.link;
    let created_by_detail = match &row.created_by {
        Some(user) => Json::object(vec![
            ("id", Json::uuid(user.id)),
            ("first_name", Json::string(user.first_name.as_str())),
            ("last_name", Json::string(user.last_name.as_str())),
            ("avatar", Json::string(user.avatar.as_str())),
            ("avatar_url", Json::opt_string(user.avatar_url.as_deref())),
            ("is_bot", Json::Bool(user.is_bot)),
            ("display_name", Json::string(user.display_name.as_str())),
        ]),
        None => Json::Null,
    };
    let metadata_text = metadata.unwrap_or(&link.metadata);
    Json::object(vec![
        ("id", Json::uuid(link.id)),
        ("created_by_detail", created_by_detail),
        (
            "created_at",
            queries::drf_datetime(link.created_at, timezone),
        ),
        (
            "updated_at",
            queries::drf_datetime(link.updated_at, timezone),
        ),
        (
            "deleted_at",
            link.deleted_at
                .map_or(Json::Null, |value| queries::drf_datetime(value, timezone)),
        ),
        ("title", Json::opt_string(link.title.as_deref())),
        ("url", Json::string(link.url.as_str())),
        (
            "metadata",
            queries::json_column(metadata_text).unwrap_or(Json::Null),
        ),
        ("created_by", Json::opt_uuid(link.created_by_id)),
        ("updated_by", Json::opt_uuid(link.updated_by_id)),
        ("project", Json::uuid(link.project_id)),
        ("workspace", Json::uuid(link.workspace_id)),
        ("issue", Json::uuid(link.issue_id)),
    ])
}

/// `IssueLink.objects.get(workspace__slug, project_id, issue_id, pk)` — the
/// default manager, which is what both lookup sites use.
async fn load_by_pk(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    pk: Uuid,
) -> Result<Option<LinkRow>, ViewError> {
    let statement = queries::LINK_BY_PK.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&slug, &project_id, &issue_id, &pk])
        .await?;
    row.map(|row| LinkRow::decode(&row)).transpose()
}

/// `self.get_queryset().get(id=…)` (`link.py:39-54`): membership, an active
/// project-membership row and `archived_at IS NULL` on the project — the read
/// catalog's `LINKS` statement, with the detail pk bound.
async fn load_response_row(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    issue_id: Uuid,
    user: Uuid,
    pk: Uuid,
) -> Result<Option<LinkRow>, ViewError> {
    let statement = read_queries::LINKS.prepare(client).await?;
    let row = client
        .query_opt(
            &statement,
            &[&slug, &project_id, &issue_id, &user, &Some(pk)],
        )
        .await?;
    row.map(|row| LinkRow::decode(&row)).transpose()
}

/// `Project.objects.get(pk=…).workspace_id` (`ProjectBaseModel.save`).
async fn project_workspace(client: &Client, project_id: Uuid) -> Result<Uuid, ViewError> {
    let statement = queries::PROJECT_WORKSPACE.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(row.get(0))
}

/// `crawl_work_item_link_title.delay(id, url)` (`link.py:62,90`).
async fn publish_crawl(
    ctx: &ViewCtx<'_>,
    id: plane_jobs::celery::args::TaskArg,
    url: String,
) -> Result<(), ViewError> {
    let task = plane_jobs::celery::task_name::CRAWL_WORK_ITEM_LINK_TITLE;
    let message = tasks::crawl_work_item_link_title(id, url);
    jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task)).await?;
    Ok(())
}

/// `issue_activity.delay(...)` in the dominant view shape (`notification=True`,
/// `origin` last), as the three link call sites write it.
#[allow(clippy::too_many_arguments)]
async fn publish_activity(
    ctx: &ViewCtx<'_>,
    activity_type: &str,
    requested_data: plane_jobs::celery::args::TaskArg,
    current_instance: plane_jobs::celery::args::TaskArg,
    epoch: DateTime<Utc>,
    issue_id: Uuid,
    project_id: Uuid,
    user: Uuid,
) -> Result<(), ViewError> {
    let base = IssueActivityBase::new(
        activity_type,
        requested_data,
        user.to_string(),
        plane_jobs::celery::args::TaskArg::str(issue_id.to_string()),
        plane_jobs::celery::args::TaskArg::str(project_id.to_string()),
        current_instance,
        epoch.timestamp(),
    );
    let message = tasks::issue_activity(IssueActivityCall::view(base, queries::base_host(ctx)?));
    let task = plane_jobs::celery::task_name::ISSUE_ACTIVITY;
    jobs::dispatch_task(ctx, task, &message, plane_jobs::queue::Job::new(task)).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Django's URLValidator
// ---------------------------------------------------------------------------

/// `URLValidator.__call__` (`django/core/validators.py:107-223`, Django 5.2)
/// for the one caller: `IssueLinkSerializer.validate_url`.
///
/// The Python is three checks and two regexes:
///
/// 1. `len(value) > 2048` (`MAX_URL_LENGTH`) or a `\t`/`\r`/`\n` anywhere is
///    invalid; the scheme (`value.split("://")[0].lower()`) must be one of
///    `http`, `https`, `ftp`, `ftps`;
/// 2. `^(?:[a-z0-9.+-]*)://(?:userinfo@)?(ipv4|ipv6|host)(:port)?([/?#]\S*)?$`
///    (`re.IGNORECASE`);
/// 3. the IPv6 literal (inside brackets) must parse, and
///    `urlsplit(value).hostname` must be non-empty and at most 253 characters.
///
/// The host grammar is `DomainNameValidator`'s: a `hostname` label, any number
/// of `domain` labels, then a `tld` of at least two letters/hyphens (or
/// `xn--` + alphanumerics) that may end with a dot, with the Unicode-letter
/// range `U+00A1..=U+FFFF` allowed anywhere in a label and no label starting or
/// ending with `-`. `localhost` is the one single-label host.
///
/// The Rust port cannot use a regex: the Python patterns contain lookbehind
/// (`(?<!-)`), which the `regex` crate does not support. The grammar is parsed
/// by hand instead; the test below pins it against cases recorded from Django.
fn django_url_valid(value: &str) -> bool {
    /// `django.utils.http.MAX_URL_LENGTH`.
    const MAX_URL_LENGTH: usize = 2048;
    // `unsafe_chars = frozenset("\t\r\n")`.
    if value.chars().count() > MAX_URL_LENGTH || value.contains(['\t', '\r', '\n']) {
        return false;
    }
    // `scheme = value.split("://")[0].lower()`.
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    if !matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "ftp" | "ftps"
    ) {
        return false;
    }
    // `^(?:[a-z0-9.+-]*)://`
    if !scheme
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '+' | '-'))
    {
        return false;
    }
    // `(?:[/?#][^\s]*)?\Z`: the first path delimiter ends the authority, and
    // a path may not contain whitespace.
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(index) => (&rest[..index], Some(&rest[index..])),
        None => (rest, None),
    };
    if path.is_some_and(|path| path.chars().any(char::is_whitespace)) {
        return false;
    }
    if !valid_authority(authority) {
        return false;
    }
    // The `urlsplit(value).hostname` length check; the regex already rejected
    // an empty host.
    let hostname = hostname_of(authority);
    !hostname.is_empty() && hostname.chars().count() <= 253
}

/// `(?:userinfo@)?host(:port)?`.
fn valid_authority(authority: &str) -> bool {
    if authority.is_empty() {
        return false;
    }
    // The userinfo alternative: `[^\s:@/]+(?::[^\s:@/]*)?@`. Try every `@` as
    // the separator (the regex's backtracking does the same).
    for (index, _) in authority.match_indices('@') {
        if valid_userinfo(&authority[..index]) && valid_host_port(&authority[index + 1..]) {
            return true;
        }
    }
    valid_host_port(authority)
}

/// `[^\s:@/]+(?::[^\s:@/]*)?`.
fn valid_userinfo(userinfo: &str) -> bool {
    let forbidden =
        |character: char| character.is_whitespace() || matches!(character, ':' | '@' | '/');
    match userinfo.split_once(':') {
        Some((user, password)) => {
            !user.is_empty() && !user.chars().any(forbidden) && !password.chars().any(forbidden)
        }
        None => !userinfo.is_empty() && !userinfo.chars().any(forbidden),
    }
}

/// `(ipv4|ipv6|host)(:[0-9]{1,5})?`.
fn valid_host_port(host_port: &str) -> bool {
    if let Some(rest) = host_port.strip_prefix('[') {
        // `ipv6_re = \[[0-9a-f:.]+\]`, then Python's `validate_ipv6_address`.
        let Some((inside, after)) = rest.split_once(']') else {
            return false;
        };
        if inside.is_empty()
            || !inside
                .chars()
                .all(|character| character.is_ascii_hexdigit() || matches!(character, ':' | '.'))
        {
            return false;
        }
        if inside.parse::<std::net::Ipv6Addr>().is_err() {
            return false;
        }
        return after.is_empty() || after.strip_prefix(':').is_some_and(valid_port);
    }
    match host_port.rsplit_once(':') {
        Some((host, port)) if valid_port(port) => valid_host(host),
        Some(_) => false,
        None => valid_host(host_port),
    }
}

/// `[0-9]{1,5}`.
fn valid_port(port: &str) -> bool {
    (1..=5).contains(&port.len()) && port.bytes().all(|byte| byte.is_ascii_digit())
}

/// `(?:ipv4|host_re)`.
fn valid_host(host: &str) -> bool {
    valid_ipv4(host) || host.eq_ignore_ascii_case("localhost") || valid_domain(host)
}

/// `ipv4_re`: four decimal octets with no leading zero.
fn valid_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            if part.is_empty() || part.len() > 3 {
                return false;
            }
            if part.len() > 1 && part.starts_with('0') {
                return false;
            }
            part.bytes().all(|byte| byte.is_ascii_digit())
                && part.parse::<u16>().is_ok_and(|value| value <= 255)
        })
}

/// `host_re = (hostname_re domain_re tld_re | localhost)`.
fn valid_domain(host: &str) -> bool {
    // `tld_re`'s trailing `\.?`.
    let host = host.strip_suffix('.').unwrap_or(host);
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|label| label.is_empty()) {
        return false;
    }
    let (first, rest) = labels.split_first().expect("at least two labels");
    // `hostname_re`: `[a-zUL0-9](?:[-a-zUL0-9]{0,61}[a-zUL0-9])?`.
    if first.chars().count() > 63 || !label_characters(first) {
        return false;
    }
    if first.starts_with('-') || first.ends_with('-') {
        return false;
    }
    // `domain_re`: `(?:\.(?!-)[-a-zUL0-9]{1,63}(?<!-))*`.
    let (tld, middle) = rest.split_last().expect("at least one domain label");
    for label in middle {
        if label.chars().count() > 63
            || !label_characters(label)
            || label.starts_with('-')
            || label.ends_with('-')
        {
            return false;
        }
    }
    // `tld_re`: `\.(?!-)(?:[a-zUL-]{2,63}|xn--[a-z0-9]{1,59})(?<!-)\.?`.
    let length = tld.chars().count();
    if tld.starts_with('-') || tld.ends_with('-') {
        return false;
    }
    if let Some(rest) = tld
        .to_ascii_lowercase()
        .strip_prefix("xn--")
        .map(str::to_owned)
    {
        return (1..=59).contains(&rest.chars().count())
            && rest
                .chars()
                .all(|character| character.is_ascii_alphanumeric());
    }
    (2..=63).contains(&length)
        && tld.chars().all(|character| {
            character == '-' || character.is_ascii_alphabetic() || unicode_letter(character)
        })
}

/// `[a-zUL0-9-]` (`re.IGNORECASE`): ASCII letters, digits, hyphen and the
/// `U+00A1..=U+FFFF` range.
fn label_characters(label: &str) -> bool {
    label.chars().all(|character| {
        character.is_ascii_alphanumeric() || character == '-' || unicode_letter(character)
    })
}

fn unicode_letter(character: char) -> bool {
    ('\u{a1}'..='\u{ffff}').contains(&character)
}

/// `urlsplit(authority).hostname`: the host without the brackets of an IPv6
/// literal or the port, lowercased, `""` when there is no host.
fn hostname_of(authority: &str) -> String {
    let authority = match authority.split_once('@') {
        Some((_, rest)) => rest,
        None => authority,
    };
    if let Some(rest) = authority.strip_prefix('[') {
        return match rest.split_once(']') {
            Some((inside, _)) => inside.to_ascii_lowercase(),
            None => String::new(),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
            host.to_ascii_lowercase()
        }
        _ => authority.to_ascii_lowercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::django_url_valid;

    /// Expected values recorded from Django 5.2's `URLValidator` on the same
    /// inputs (`apps/api/.venv`, CPython 3.12.13).
    #[test]
    fn url_validator_matches_django() {
        let cases: &[(&str, bool)] = &[
            ("https://example.com", true),
            ("http://example.com", true),
            ("http://localhost", true),
            ("http://localhost:8000", true),
            ("http://127.0.0.1", true),
            ("http://127.0.0.1:65536", true),
            ("http://[::1]", true),
            ("http://[::1]:8080", true),
            ("http://[zz]", false),
            ("http://[::1", false),
            ("http://example", false),
            ("http://example.c", false),
            ("http://example.com.", true),
            ("http://example..com", false),
            ("http://-example.com", false),
            ("http://exa mple.com", false),
            ("http://example.com:123456", false),
            ("http://user:pass@example.com", true),
            ("http://user@example.com", true),
            ("http://@example.com", false),
            ("ftp://example.com", true),
            ("ftps://example.com", true),
            ("file://example.com", false),
            ("http://例え.jp", true),
            ("http://example.123", false),
            ("http://example.xn--p1ai", true),
            ("http://xn--e1afmkfd.xn--p1ai", true),
            ("http://example.com/path?q=1#frag", true),
            ("http://example.com/a b", false),
            ("http://example.com\\path", false),
            ("http://not a url", false),
            ("http://javascript:alert(1)", false),
            ("http://example.com:0", true),
            ("http://192.168.1.1", true),
            ("http://256.1.1.1", false),
            ("http://01.2.3.4", false),
            ("http://example.com#", true),
            ("http://example.com?x=1", true),
            ("http://例子.测试", true),
            ("http://example.com/ä", true),
            ("http://example.com/a\tb", false),
            ("HTTP://EXAMPLE.COM", true),
            ("http://example.com:abc", false),
            ("http://a.b.c.d.example.com", true),
            ("http://example.com/", true),
            ("http://example.com:80/x", true),
            ("http://example.com:1234567", false),
            ("http://user:@example.com", true),
            ("http://user:pa@ss@example.com", false),
            ("http://example.com..", false),
            ("http://ex-ample.com", true),
            ("http://example.co-m", true),
            ("http://example-.com", false),
            ("http://.example.com", false),
            ("http://example.com./x", true),
            ("http://EXAMPLE.COM", true),
            ("http://example.com/%20", true),
            ("http://example.com:x", false),
            ("https://sub.domain.example.museum", true),
            ("http://example.com?", true),
            ("http://example.com#frag x", false),
        ];
        for (value, expected) in cases {
            assert_eq!(django_url_valid(value), *expected, "input {value:?}");
        }
    }

    #[test]
    fn the_length_limit_counts_characters() {
        let mut at_limit = String::from("http://example.com/");
        at_limit.push_str(&"a".repeat(2048 - at_limit.chars().count()));
        assert_eq!(at_limit.chars().count(), 2048);
        assert!(django_url_valid(&at_limit));
        at_limit.push('a');
        assert!(!django_url_valid(&at_limit));
    }
}
