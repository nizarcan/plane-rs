// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `ModuleLinkViewSet`'s write actions (`plane/app/views/module/base.py:763-796`).
//!
//! | Route | Method | Handler |
//! | --- | --- | --- |
//! | `…/modules/{module_id}/module-links/` (108) | `POST` | [`create`] |
//! | `…/modules/{module_id}/module-links/{pk}/` (109) | `PUT` | [`update`] |
//! | the same | `PATCH` | [`partial_update`] |
//! | the same | `DELETE` | [`destroy`] |
//!
//! Unlike issue links, this viewset does **not** override the DRF mixins, so
//! `PUT` is a real update here (route 109 has no guard entry) and the create
//! response carries DRF's `Location` header (`get_success_headers`).
//!
//! ## The body
//!
//! `ModuleLinkSerializer` is `fields = "__all__"` with `workspace`, `project`,
//! `created_by`, `updated_by`, `created_at`, `updated_at` and `module`
//! read-only (`plane/app/serializers/module.py:156-168`), so the body can set
//! `deleted_at`, `title`, `url` and `metadata`. `to_internal_value`
//! (`:170-176`) prefixes `http://` onto the **raw** url string when it is
//! truthy and does not already start with `http://`/`https://` — **in place on
//! `request.data`**, so a urlencoded or file-less multipart body's immutable
//! `QueryDict` raises `AttributeError` (the family 500) before any field is
//! parsed, while a multipart body with a file part is copied mutable (see
//! [`guard_url_rewrite`]). Then the
//! `URLField` runs `CharField` (max length 200 from the model) and Django's
//! `URLValidator`. `validate_url` (`:178-186`) runs the same validator again
//! and would render `{"url": {"error": "Invalid URL format."}}`; it is
//! unreachable for a field (the DRF field already rejected the value), so the
//! visible field error is `{"url": ["Enter a valid URL."]}`.
//!
//! ## create (`base.py:767-775`)
//!
//! `perform_create` resolves the URL module first — `Module.objects.get(pk,
//! project_id, workspace__slug)` → 404 — and only then `serializer.save(
//! project_id=…, module_id=…)`. `create()` (`serializers/module.py:188-192`)
//! re-validates the url and rejects a duplicate `(url, module_id)` with
//! `{"error": "URL already exists."}` — the `module_id` is in `validated_data`
//! because `save()` merged its kwargs. `objects.create` runs
//! `ProjectBaseModel.save` (workspace from the project) and `BaseModel.save`
//! (`created_by` = the caller). Then the row is rendered from the in-memory
//! instance (`serializer.data`), not re-read, so `metadata` keeps the body's
//! key order.
//!
//! ## PUT / PATCH (`:763-796`)
//!
//! `get_object()` reads through `get_queryset()` — membership and
//! `archived_at IS NULL` — so a miss is 404. `update()` (`:194-203`) calls
//! `validate_url(validated_data.get("url"))` **unconditionally**: a `PATCH`
//! without a `url` validates `None` and answers `400 {"error": "Invalid URL
//! format."}` (upstream bug, ported). The duplicate check excludes the row
//! itself and says `{"error": "URL already exists for this Issue"}`. The
//! instance save is a full `save()`: absent keys keep the row's value, so a
//! `PUT` with only `url` does not clear `title`/`metadata` (DRF only sets the
//! keys it validated). `deleted_at` is writable, so a PATCH can soft-delete
//! the row through `save()` — no cascade task, unlike `DELETE`.
//!
//! ## destroy
//!
//! `DestroyModelMixin.destroy` → `instance.delete()` — H03: `deleted_at` and
//! `updated_at` stamped, `updated_by` set, plus the
//! `soft_delete_related_objects("db", "modulelink", pk)` task.

use chrono::{DateTime, Utc};

use deadpool_postgres::Client;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::JsonText;
use plane_db::hooks;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_query::json::Json;
use uuid::Uuid;

use super::base::{ModuleLinkRow, uuid_kwarg};
use super::queries as read_queries;
use super::write_issue_queries as queries;
use crate::common::{self, jobs};
use crate::issues::write_sub_queries::{RawField, raw_field};
use crate::write;
use crate::write::Serializer;
use crate::write::fields::{self, CharOptions, Detail, Raw};
use crate::write::url::{django_url_valid, guard_url_rewrite};

// ---------------------------------------------------------------------------
// 108 — POST …/module-links/
// ---------------------------------------------------------------------------

/// `ModuleLinkViewSet.create` (DRF's `CreateModelMixin` over
/// `base.py:767-775`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let module_id = uuid_kwarg(ctx, "module_id")?;
        let user = jobs::user_id(ctx)?;
        let now = write::now();

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
            .expect("a created link always has a validated url");

        // `perform_create`'s `Module.objects.get(pk, project_id,
        // workspace__slug)` (`base.py:769-773`), before the duplicate check.
        require_module(client, &slug, project_id, module_id).await?;

        // `ModuleLinkSerializer.create`'s duplicate check (`:190-191`).
        let statement = queries::MODULE_LINK_URL_TAKEN_ON_CREATE
            .prepare(client)
            .await?;
        let taken: bool = client
            .query_one(&statement, &[&url, &module_id])
            .await?
            .get(0);
        if taken {
            return Ok(error_body(400, "URL already exists."));
        }

        // `serializer.save(project_id=…, module_id=…)` (`:774-775`) —
        // `ProjectBaseModel.save` copies the workspace, H01 sets `created_by`.
        let workspace_id = project_workspace(client, project_id).await?;
        let link_id = Uuid::new_v4();
        let metadata = values
            .metadata
            .clone()
            .unwrap_or_else(|| JsonText::new("{}"));
        let statement = queries::MODULE_LINK_INSERT.prepare(client).await?;
        client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &user,
                    &values.deleted_at.flatten(),
                    &link_id,
                    &project_id,
                    &workspace_id,
                    &values.title.clone().flatten(),
                    &url,
                    &metadata,
                    &module_id,
                ],
            )
            .await?;

        // `Response(serializer.data, status=201, headers=get_success_headers(…))`
        // — the in-memory instance, so `metadata` keeps the body's key order.
        let row = ModuleLinkRow {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: values.deleted_at.flatten(),
            id: link_id,
            project_id,
            workspace_id,
            title: values.title.clone().flatten(),
            url: url.clone(),
            module_id,
            metadata: Some(metadata),
        };
        let body = common::render(ctx, &row.value())?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body).with_header("Location", url))
    })
}

// ---------------------------------------------------------------------------
// 109 — PUT / PATCH …/module-links/{pk}/
// ---------------------------------------------------------------------------

/// `ModuleLinkViewSet.update` (DRF's `UpdateModelMixin`, `partial=False`).
pub fn update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(update_impl(ctx, false))
}

/// `ModuleLinkViewSet.partial_update` (DRF's `UpdateModelMixin`,
/// `partial=True`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(update_impl(ctx, true))
}

async fn update_impl(ctx: &ViewCtx<'_>, partial: bool) -> Result<ViewResponse, ViewError> {
    let client = common::client(ctx)?;
    let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
    let project_id = uuid_kwarg(ctx, "project_id")?;
    let module_id = uuid_kwarg(ctx, "module_id")?;
    let pk = uuid_kwarg(ctx, "pk")?;
    let user = jobs::user_id(ctx)?;
    let now = write::now();

    // `self.get_object()` (`base.py:777-796`): the read queryset, so a
    // non-member or an archived project is a 404.
    let Some(previous) = load_object(client, &slug, project_id, module_id, user, pk).await? else {
        // `get_object_or_404` on the viewset's queryset: `Http404("No
        // ModuleLink matches the given query.")`
        // (`rest_framework/generics.py:87-89`).
        return Err(ViewError::Api(ApiError::not_found_detail(
            "No ModuleLink matches the given query.",
        )));
    };

    let data = ctx.data()?;
    // `to_internal_value`'s in-place rewrite runs before DRF parses
    // any field, so its `AttributeError`s precede every field error.
    guard_url_rewrite(data)?;
    let mut serializer = Serializer::new(ctx, partial)?;
    let values = match read_body(&mut serializer, data, partial)? {
        Some(values) => match serializer.finish() {
            Ok(_) => values,
            Err(errors) => return Ok(errors.into_response()),
        },
        None => return Ok(serializer_error(serializer)),
    };

    // `update()` calls `self.validate_url(validated_data.get("url"))`
    // unconditionally (`:195`): a PATCH without a url validates `None`, which
    // is Django's `ValidationError` → 400. Ported as-is.
    let Some(url) = values.url.clone() else {
        return Ok(error_body(400, "Invalid URL format."));
    };

    // `update()`'s duplicate check (`:196-201`).
    let statement = queries::MODULE_LINK_URL_TAKEN_ELSEWHERE
        .prepare(client)
        .await?;
    let taken: bool = client
        .query_one(&statement, &[&url, &module_id, &pk])
        .await?
        .get(0);
    if taken {
        return Ok(error_body(400, "URL already exists for this Issue"));
    }

    // `super().update(instance, validated_data)` then `instance.save()`: the
    // absent keys keep the row's value.
    let title = match &values.title {
        Some(title) => title.clone(),
        None => previous.title.clone(),
    };
    let metadata = match &values.metadata {
        Some(metadata) => metadata.clone(),
        None => previous
            .metadata
            .clone()
            .unwrap_or_else(|| JsonText::new("{}")),
    };
    let deleted_at = match &values.deleted_at {
        Some(deleted_at) => *deleted_at,
        None => previous.deleted_at,
    };
    let statement = queries::MODULE_LINK_UPDATE.prepare(client).await?;
    client
        .execute(
            &statement,
            &[&title, &url, &metadata, &deleted_at, &now, &user, &pk],
        )
        .await?;

    let row = ModuleLinkRow {
        created_at: previous.created_at,
        updated_at: now,
        created_by_id: previous.created_by_id,
        updated_by_id: Some(user),
        deleted_at,
        id: pk,
        project_id: previous.project_id,
        workspace_id: previous.workspace_id,
        title,
        url,
        module_id: previous.module_id,
        metadata: Some(metadata),
    };
    let body = common::render(ctx, &row.value())?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

// ---------------------------------------------------------------------------
// 109 — DELETE …/module-links/{pk}/
// ---------------------------------------------------------------------------

/// `ModuleLinkViewSet.destroy` (DRF's `DestroyModelMixin` → H03).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = common::client(ctx)?;
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?.to_owned();
        let project_id = uuid_kwarg(ctx, "project_id")?;
        let module_id = uuid_kwarg(ctx, "module_id")?;
        let pk = uuid_kwarg(ctx, "pk")?;
        let user = jobs::user_id(ctx)?;
        let now = write::now();

        if load_object(client, &slug, project_id, module_id, user, pk)
            .await?
            .is_none()
        {
            return Err(ViewError::Api(ApiError::not_found_detail(
                "No ModuleLink matches the given query.",
            )));
        }

        // `instance.delete()` — H03 (`plane/db/mixins.py:65-77`).
        hooks::soft_delete::delete_instance(client, "module_links", "id", pk, Some(user), now)
            .await?;
        write::publish_soft_delete_cascade(ctx, "db", "modulelink", pk).await?;

        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// The body
// ---------------------------------------------------------------------------

/// `ModuleLinkSerializer`'s writable fields, in `self.fields` order
/// (`plane/app/serializers/module.py:156-168`).
#[derive(Debug, Default)]
struct LinkValues {
    /// `deleted_at`: `Some(None)` is an explicit `null`.
    deleted_at: Option<Option<DateTime<Utc>>>,
    title: Option<Option<String>>,
    url: Option<String>,
    metadata: Option<JsonText>,
}

/// Run the serializer over the body in field order. `None` means a field
/// error was recorded (the caller renders `serializer.errors`).
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
    let deleted_at = serializer.datetime_field("deleted_at", true);
    values.deleted_at = tri_state(data, "deleted_at", deleted_at);

    // `title`: `CharField(max_length=255, allow_null=True, allow_blank=True)`.
    let title = serializer.char_field(
        "title",
        CharOptions {
            allow_null: true,
            allow_blank: true,
            trim_whitespace: true,
            max_length: Some(255),
        },
    );
    values.title = tri_state(data, "title", title);

    // `url`: `to_internal_value`'s prefix, then `URLField`.
    values.url = read_url(serializer, data, partial)?;

    // `metadata`: `JSONField(required=False)`, `allow_null=False`.
    values.metadata = serializer.json_field("metadata", false);

    if serializer.has_errors() {
        return Ok(None);
    }
    Ok(Some(values))
}

/// `(absent, explicit null, value)` from the raw body and the parser's result.
fn tri_state<T>(data: &Data, name: &str, parsed: Option<T>) -> Option<Option<T>> {
    match raw_field(data, name) {
        RawField::Absent => None,
        RawField::Null => Some(None),
        RawField::Text(_) | RawField::Other(_) => parsed.map(Some),
    }
}

/// `ModuleLinkSerializer.to_internal_value`'s `http://` prefix
/// (`plane/app/serializers/module.py:170-176`) followed by the `URLField`'s
/// `CharField` (max length 200) and `URLValidator`.
fn read_url(
    serializer: &mut Serializer<'_>,
    data: &Data,
    partial: bool,
) -> Result<Option<String>, ViewError> {
    let prefixed = match raw_field(data, "url") {
        RawField::Absent => {
            if !partial && serializer.missing_required("url") {
                serializer.push_error("url", Detail::required());
            }
            return Ok(None);
        }
        RawField::Null => {
            serializer.push_error("url", Detail::null());
            return Ok(None);
        }
        RawField::Text(value) => {
            if value.is_empty() || value.starts_with("http://") || value.starts_with("https://") {
                value.to_owned()
            } else {
                format!("http://{value}")
            }
        }
        RawField::Other(other) => {
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
        // `URLField`'s `max_length` from the model (`url = models.URLField()`,
        // Django's default 200).
        max_length: Some(200),
    };
    match fields::char(Raw::Text(&prefixed), &options) {
        Ok(value) => {
            if django_url_valid(&value) {
                Ok(Some(value))
            } else {
                serializer.push_error("url", Detail::new("Enter a valid URL.", "invalid"));
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

/// `Response({"error": message}, status)` — the body `update()`/`create()`
/// raise as a DRF `ValidationError`.
fn error_body(status: u16, message: &str) -> ViewResponse {
    let body = Json::object(vec![("error", Json::string(message))]);
    ViewResponse::raw(status, APPLICATION_JSON, body.to_drf())
}

// ---------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------

/// `Module.objects.get(pk, project_id, workspace__slug)`
/// (`base.py:769-773`): `DoesNotExist` is the family 404.
async fn require_module(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    module_id: Uuid,
) -> Result<(), ViewError> {
    let statement = queries::MODULE_IN_PROJECT.prepare(client).await?;
    let exists: bool = client
        .query_one(&statement, &[&module_id, &project_id, &slug])
        .await?
        .get(0);
    if exists {
        Ok(())
    } else {
        Err(ViewError::ObjectDoesNotExist)
    }
}

/// `self.project.workspace` in `ProjectBaseModel.save`
/// (`plane/db/models/project.py:237-239`) — the FK descriptor reads the base
/// manager, so there is no `deleted_at` filter.
async fn project_workspace(client: &Client, project_id: Uuid) -> Result<Uuid, ViewError> {
    let statement = queries::PROJECT_WORKSPACE_ANY.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&project_id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(row.get(0))
}

/// `self.get_queryset().get(pk=…)` (`base.py:777-796`) — the read catalog's
/// detail statement, which is the same queryset with the pk bound.
async fn load_object(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    module_id: Uuid,
    user: Uuid,
    pk: Uuid,
) -> Result<Option<ModuleLinkRow>, ViewError> {
    let statement = read_queries::MODULE_LINK_BY_PK.prepare(client).await?;
    let row = client
        .query_opt(
            &statement,
            &[&slug, &project_id, &module_id, &user, &Some(pk)],
        )
        .await?;
    row.map(|row| ModuleLinkRow::decode(&row)).transpose()
}
