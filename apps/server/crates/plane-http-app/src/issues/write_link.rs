// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `IssueLinkViewSet`'s write actions (`plane/app/views/issue/link.py:56-126`).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/issue-links/` | `create` | `:56-79` |
//! | `PUT …/issue-links/{pk}/` | `update` | DRF `UpdateModelMixin.update`, `:80-109` |
//! | `PATCH …/issue-links/{pk}/` | `partial_update` | `:80-109` |
//! | `DELETE …/issue-links/{pk}/` | `destroy` | `:111-126` |
//!
//! `PUT` is **not** refused by `BaseViewSet`'s guard: the viewset declares
//! `ProjectEntityPermission`, a real permission class, so
//! `_resolved_action_is_authorized` returns at
//! `plane/app/views/base.py:141-142` and DRF's `UpdateModelMixin.update` runs
//! the **full** serializer (`rest_framework/mixins.py:63-75`). Only `PATCH`
//! has a viewset implementation, so `PUT` is the generic mixin: `get_object()`
//! through `get_queryset()`, `partial=False`, and a response built from the
//! in-memory instance rather than a re-read.
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
//!    `http://`/`https://` (`to_internal_value`, `:577-583`) — the write is
//!    in place on `request.data`, so a urlencoded or file-less multipart
//!    body's immutable `QueryDict` raises `AttributeError` (the family 500)
//!    before any field is parsed; a multipart body with a file part is copied
//!    mutable and the prefixed value is what the field validates (see
//!    [`guard_url_rewrite`]),
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
//! The crawl task re-runs only when the URL changed (`:90-92`). The response
//! re-reads the row through `get_queryset()` (`:105-106`), so a PATCH that
//! sets `deleted_at` is 404 — the `PUT` path below is not.
//!
//! **update** is DRF's mixin, unchanged:
//!
//! 1. `self.get_object()` — `get_queryset()`, which does filter membership and
//!    `archived_at IS NULL`, so a row the PATCH path can still read is a 404
//!    here (`generics.py:79-89`, `"No IssueLink matches the given query."`).
//! 2. The **full** serializer (`partial=False`), so `url` is required.
//! 3. `serializer.update`'s duplicate check with the body's `url`; it always
//!    runs because `url` is required. Only the keys `validated_data` carries
//!    are assigned, so an absent `title`/`metadata`/`deleted_at` keeps the
//!    row's value.
//! 4. `Response(serializer.data)` from the in-memory instance, so the
//!    `metadata` key order is the body's (no `jsonb` round trip) and a PUT
//!    that sets `deleted_at` still answers 200.
//!
//! No task is published by `update`: the crawl and the activity are
//! `create`'s and `partial_update`'s only (`link.py:56-126`).
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
use plane_core::error::{APPLICATION_JSON, ApiError};
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
use crate::write::url::{django_url_valid, guard_url_rewrite};

/// Register the four actions on the view that also serves `list` and
/// `retrieve` (`sub_links.rs`).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.issue.link.IssueLinkViewSet")?
        .action("create", create)?
        .action("update", update)?
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
        // `to_internal_value`'s in-place rewrite runs before DRF parses
        // any field, so its `AttributeError`s precede every field error.
        guard_url_rewrite(data)?;
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
// update
// ---------------------------------------------------------------------------

/// `IssueLinkViewSet.update` — DRF's `UpdateModelMixin.update` with
/// `partial=False` (`rest_framework/mixins.py:63-75`), which is route 75's
/// `PUT`. The viewset implements only `partial_update`, so the guard's
/// `project_implemented` is false; `ProjectEntityPermission` is what keeps the
/// guard from refusing this action (`plane/app/views/base.py:141-142`).
pub fn update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let issue_id = kwarg_uuid(ctx, "issue_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = shared::user_id(ctx)?;
        let now = write::now();

        // `self.get_object()` (`mixins.py:65`): `get_queryset()` — membership,
        // an active project membership and `archived_at IS NULL` — then
        // `get_object_or_404`, whose `Http404` DRF re-raises as
        // `NotFound(*(exc.args))` (`rest_framework/views.py:82-83`). The model
        // name is `IssueLink` (`generics.py:87-89`).
        let Some(previous) =
            load_response_row(client, &slug, project_id, issue_id, user, pk).await?
        else {
            return Err(ViewError::Api(ApiError::not_found_detail(
                "No IssueLink matches the given query.",
            )));
        };

        // The **full** serializer: `url` is required, every other writable
        // field keeps its row value when absent.
        let data = ctx.data()?;
        // `to_internal_value`'s in-place rewrite runs before DRF parses
        // any field, so its `AttributeError`s precede every field error.
        guard_url_rewrite(data)?;
        let mut serializer = Serializer::new(ctx, false)?;
        let values = match read_body(&mut serializer, data, false)? {
            Some(values) => match serializer.finish() {
                Ok(_) => values,
                Err(errors) => return Ok(errors.into_response()),
            },
            None => return Ok(serializer_error(serializer)),
        };
        let url = values
            .url
            .clone()
            .expect("a full update always has a validated url");

        // `IssueLinkSerializer.update`'s duplicate check (`:601-609`). Unlike
        // `partial_update`'s it cannot be skipped: `url` is required here.
        let statement = queries::LINK_URL_TAKEN_ELSEWHERE.prepare(client).await?;
        let taken: bool = client
            .query_one(&statement, &[&url, &issue_id, &pk])
            .await?
            .get(0);
        if taken {
            return Ok(error_body(400, "URL already exists for this Issue"));
        }

        // `serializer.save()` → `ModelSerializer.update`, which assigns only
        // the keys `validated_data` carries, then `instance.save()`:
        // `BaseModel.save` stamps `updated_by` and `auto_now` `updated_at`
        // (`plane/db/models/base.py:23-45`).
        let title = match &values.title {
            Some(title) => title.clone(),
            None => previous.link.title.clone(),
        };
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

        // `Response(serializer.data)` from the in-memory instance
        // (`mixins.py:75`): no re-read, so a PUT that sets `deleted_at` still
        // answers 200, and `metadata` keeps the body's key order because it
        // never round-trips through `jsonb`. No crawl and no activity task:
        // `update` is DRF's mixin, not `partial_update` (`link.py:80-109`).
        let updated = LinkRow {
            link: IssueLink {
                created_at: previous.link.created_at,
                updated_at: now,
                created_by_id: previous.link.created_by_id,
                updated_by_id: Some(user),
                deleted_at,
                id: previous.link.id,
                project_id: previous.link.project_id,
                workspace_id: previous.link.workspace_id,
                title,
                url,
                issue_id: previous.link.issue_id,
                metadata,
            },
            created_by: previous.created_by,
        };
        let entry = LinkEntry {
            link: &updated.link,
            created_by: updated.created_by.as_ref().map(UserLiteRow::serializer),
        };
        let body = common::render(ctx, &entry.serializer())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
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
        // `to_internal_value`'s in-place rewrite runs before DRF parses
        // any field, so its `AttributeError`s precede every field error.
        guard_url_rewrite(data)?;
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
