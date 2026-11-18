// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Labels on `/api/v1/` (routes 290/291).
//!
//! Python: `plane/api/views/issue.py:949-1188`.
//!
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | `pr/labels/` | `GET` | `LabelListCreateAPIEndpoint.get` (`:1103-1110`) |
//! | same | `POST` | `.post` (`:995-1043`) |
//! | `pr/labels/{pk}/` | `GET` | `LabelDetailAPIEndpoint.get` (`:1115-1125`) |
//! | same | `PATCH` | `.patch` (`:1132-1167`) |
//! | same | `DELETE` | `.delete` (`:1176-1187`) |
//!
//! ## What differs from the app surface
//!
//! * the queryset orders `-created_at` (the app orders `sort_order`) and also
//!   hides labels of an archived project (`labels_queries.rs`);
//! * `LabelCreateUpdateSerializer` has **no** `validate_name`
//!   (`plane/api/serializers/issue.py:370-398`), so a duplicate name reaches
//!   the partial unique index; the create catches the `IntegrityError` and
//!   answers 409 `{"error": "Label with the same name already exists in the
//!   project", "id": …}` (`issue.py:1028-1043`), while the PATCH has no
//!   `IntegrityError` catch and answers the family 400;
//! * both writes run an `external_id`/`external_source` 409 before the save
//!   (`:1003-1022`, `:1141-1152`);
//! * the create re-fetches the row and answers **201** with the *read*
//!   serializer; the PATCH does the same with 200.
//!
//! ## Bugs ported as they are
//!
//! * the `IntegrityError` handler filters on `name=request.data.get("name")`,
//!   the **raw** body value, so a name that only collides after
//!   `trim_whitespace` leaves `label = None` and `str(label.id)` raises
//!   `AttributeError` → 500 (`:1029-1037`);
//! * `?expand=parent` renders the label's parent through `IssueLiteSerializer`
//!   (`plane/api/serializers/base.py:111`), which reads `sequence_id` off a
//!   `Label`: a non-null parent is an `AttributeError` → 500. A null parent
//!   stays null (`:126-132`).

use std::collections::{HashMap, HashSet};

use deadpool_postgres::Client;
use http::Method;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::label::Label;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::{PyDict, PyStr, Value as Json};
use plane_serializers::generated::api_issue::LabelSerializer;
use plane_serializers::generated::api_project::ProjectLiteSerializer;
use plane_serializers::generated::api_user::UserLiteSerializer;
use plane_serializers::generated::api_workspace::WorkspaceLiteSerializer;
use plane_serializers::{Expansion, Expansions, PgJson};
use uuid::Uuid;

use plane_http_app::assets::workspace_write::body_get;
use plane_http_app::common::{client, lookup, render::RequestRender};
use plane_http_app::projects::lite;
use plane_http_app::write::{self, Serializer, Validated};

use crate::common::render::expansions;
use crate::labels_queries as queries;
use crate::pyvalue::python_str;

/// Register both classes.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.api.views.issue.LabelListCreateAPIEndpoint")?
        .method(Method::GET, list)?
        .method(Method::POST, create)?;
    registry
        .api_view("plane.api.views.issue.LabelDetailAPIEndpoint")?
        .method(Method::GET, retrieve)?
        .method(Method::PATCH, patch)?
        .method(Method::DELETE, destroy)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

/// `LabelListCreateAPIEndpoint.get` (`plane/api/views/issue.py:1103-1110`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let member = member_id(ctx)?;
        let client = client(ctx)?;

        let (labels, total, window, next, prev) = crate::projects::fetch_page(
            ctx,
            client,
            queries::LABELS_BY_PROJECT.sql,
            &[&slug, &project_id, &member],
            queries::LABELS_COUNT_BY_PROJECT.sql,
            &[&slug, &project_id, &member],
            1000,
            |row| Label::from_row(row, 0).map_err(ViewError::from),
        )
        .await?;

        let requested = ctx.expand().unwrap_or_default();
        let prepared = RequestRender::from_ctx(ctx);
        let body = render_labels(ctx, client, &labels, &requested, &prepared).await?;
        Ok(crate::projects::page_envelope(
            total,
            &window,
            &next,
            &prev,
            labels.len(),
            body,
        ))
    })
}

/// `LabelDetailAPIEndpoint.get` (`plane/api/views/issue.py:1115-1125`): the read
/// serializer **without** `fields=`/`expand=`.
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let member = member_id(ctx)?;
        let client = client(ctx)?;

        let Some(label) = label_in_project(client, slug, project_id, pk, member).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(
            &serializer(&label),
            &plane_serializers::RenderCtx::full(&timezone),
        )
        .map_err(ViewError::from)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

/// `LabelListCreateAPIEndpoint.post` (`plane/api/views/issue.py:995-1043`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let user = member_id(ctx)?;
        let client = client(ctx)?;

        let data = ctx.data()?;
        let serializer = Serializer::new(ctx, false)?;
        let validated = match validate(serializer, client).await? {
            Ok(validated) => validated,
            Err(response) => return Ok(response),
        };

        // The external-id 409 runs before any write (`:1003-1022`).
        if let Some(response) = external_conflict(client, slug, project_id, None, data).await? {
            return Ok(response);
        }

        // `Label.save()`: H20 (`MAX(sort_order)`), then the workspace through
        // `self.project`. H20 **replaces** the body's `sort_order` whenever the
        // project already has a live label (`plane/db/models/label.py:38-44`).
        let max_statement = client
            .prepare_cached(queries::LABEL_MAX_SORT_ORDER.sql)
            .await?;
        let largest: Option<f64> = client
            .query_one(&max_statement, &[&project_id])
            .await?
            .get(0);
        let sort_order = largest.map_or_else(
            || validated.sort_order.unwrap_or(65535.0),
            |largest| largest + 10_000.0,
        );
        let Some(workspace_id) = project_workspace(client, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        let now = write::now();
        let label = Label {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            workspace_id,
            project_id: Some(project_id),
            parent_id: validated.parent.flatten(),
            name: validated.name.unwrap_or_default(),
            description: validated.description.unwrap_or_default(),
            color: validated.color.unwrap_or_default(),
            sort_order,
            external_source: validated.external_source.flatten(),
            external_id: validated.external_id.flatten(),
        };
        let statement = client.prepare_cached(queries::LABEL_INSERT.sql).await?;
        if let Err(error) = client
            .execute(
                &statement,
                &[
                    &label.created_at,
                    &label.updated_at,
                    &label.created_by_id,
                    &label.updated_by_id,
                    &label.deleted_at,
                    &label.id,
                    &label.workspace_id,
                    &label.project_id,
                    &label.parent_id,
                    &label.name,
                    &label.description,
                    &label.color,
                    &label.sort_order,
                    &label.external_source,
                    &label.external_id,
                ],
            )
            .await
        {
            if is_integrity(&error) {
                // `Label.objects.filter(..., name=request.data.get("name"))`
                // (`:1029-1033`): the raw body value, `str()`-converted by
                // the CharField lookup.
                let raw_name = body_get(data, "name")
                    .as_ref()
                    .map(python_str)
                    .unwrap_or_default();
                let lookup = client
                    .prepare_cached(queries::LABEL_BY_NAME_FIRST.sql)
                    .await?;
                let Some(row) = client
                    .query_opt(&lookup, &[&slug, &project_id, &raw_name])
                    .await?
                else {
                    return Err(ViewError::Unknown(
                        "AttributeError: 'NoneType' object has no attribute 'id'".to_owned(),
                    ));
                };
                let existing = Label::from_row(&row, 0)?;
                return conflict(
                    "Label with the same name already exists in the project",
                    existing.id,
                );
            }
            return Err(ViewError::from(error));
        }

        // `label = Label.objects.get(pk=serializer.instance.id)` then the read
        // serializer (`:1024-1027`).
        let Some(label) = label_by_id(client, label.id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        render_created(ctx, &label, 201)
    })
}

/// `LabelDetailAPIEndpoint.patch` (`plane/api/views/issue.py:1132-1167`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user = member_id(ctx)?;
        let client = client(ctx)?;

        let Some(mut label) = label_in_project(client, slug, project_id, pk, user).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };

        let data = ctx.data()?;
        let serializer = Serializer::new(ctx, true)?;
        let validated = match validate(serializer, client).await? {
            Ok(validated) => validated,
            Err(response) => return Ok(response),
        };

        // `request.data.get("external_id") and
        // state.external_id != str(request.data.get("external_id")) and …`
        // (`:1141-1152`).
        if let Some(response) =
            external_conflict(client, slug, project_id, Some(&label), data).await?
        {
            return Ok(response);
        }

        if let Some(parent) = validated.parent {
            label.parent_id = parent;
        }
        if let Some(name) = validated.name {
            label.name = name;
        }
        if let Some(description) = validated.description {
            label.description = description;
        }
        if let Some(color) = validated.color {
            label.color = color;
        }
        if let Some(sort_order) = validated.sort_order {
            label.sort_order = sort_order;
        }
        if let Some(external_source) = validated.external_source {
            label.external_source = external_source;
        }
        if let Some(external_id) = validated.external_id {
            label.external_id = external_id;
        }
        // `Model.save()`: `WorkspaceBaseModel.save` re-reads the project, then
        // H01's `updated_by`/`updated_at` and a full-row UPDATE.
        let Some(workspace_id) = project_workspace(client, project_id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        label.workspace_id = workspace_id;
        label.updated_at = write::now();
        label.updated_by_id = Some(user);
        let statement = client
            .prepare_cached(plane_http_app::labels::queries::LABEL_UPDATE.sql)
            .await?;
        client
            .execute(
                &statement,
                &[
                    &label.created_at,
                    &label.updated_at,
                    &label.created_by_id,
                    &label.updated_by_id,
                    &label.deleted_at,
                    &label.id,
                    &label.workspace_id,
                    &label.project_id,
                    &label.parent_id,
                    &label.name,
                    &label.description,
                    &label.color,
                    &label.sort_order,
                    &label.external_source,
                    &label.external_id,
                    &label.id,
                ],
            )
            .await
            .map_err(|error| match is_integrity(&error) {
                true => ViewError::Integrity,
                false => ViewError::from(error),
            })?;

        let Some(label) = label_by_id(client, label.id).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        render_created(ctx, &label, 200)
    })
}

/// `LabelDetailAPIEndpoint.delete` (`plane/api/views/issue.py:1176-1187`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let user = member_id(ctx)?;
        let client = client(ctx)?;

        let Some(label) = label_in_project(client, slug, project_id, pk, user).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        // `label.delete()`: H03 — `deleted_at`, a full `Label.save()` with
        // H01's audit columns, then the cascade task.
        let now = write::now();
        let mut deleted = label.clone();
        deleted.deleted_at = Some(now);
        deleted.updated_at = now;
        deleted.updated_by_id = Some(user);
        let statement = client
            .prepare_cached(plane_http_app::labels::queries::LABEL_UPDATE.sql)
            .await?;
        client
            .execute(
                &statement,
                &[
                    &deleted.created_at,
                    &deleted.updated_at,
                    &deleted.created_by_id,
                    &deleted.updated_by_id,
                    &deleted.deleted_at,
                    &deleted.id,
                    &deleted.workspace_id,
                    &deleted.project_id,
                    &deleted.parent_id,
                    &deleted.name,
                    &deleted.description,
                    &deleted.color,
                    &deleted.sort_order,
                    &deleted.external_source,
                    &deleted.external_id,
                    &deleted.id,
                ],
            )
            .await
            .map_err(|error| match is_integrity(&error) {
                true => ViewError::Integrity,
                false => ViewError::from(error),
            })?;
        write::publish_soft_delete_cascade(ctx, "db", "label", label.id).await?;
        Ok(ViewResponse::empty(204))
    })
}

// ---------------------------------------------------------------------------
// Validation (`plane/api/serializers/issue.py:370-398`)
// ---------------------------------------------------------------------------

/// The validated `LabelCreateUpdateSerializer` fields.
#[derive(Debug, Default)]
struct Body {
    name: Option<String>,
    color: Option<String>,
    description: Option<String>,
    /// `Some(None)` is an explicit `null` (the column is nullable).
    external_source: Option<Option<String>>,
    external_id: Option<Option<String>>,
    /// `Some(None)` is an explicit `null`.
    parent: Option<Option<Uuid>>,
    sort_order: Option<f64>,
}

/// `LabelCreateUpdateSerializer(data=…)` / `(label, data=…, partial=True)`.
async fn validate(
    mut serializer: Serializer<'_>,
    client: &Client,
) -> Result<Result<Body, ViewResponse>, ViewError> {
    plane_http_app::write::generated::label_create_update_serializer::validate(
        &mut serializer,
        client,
    )
    .await?;
    let validated = match serializer.finish() {
        Ok(validated) => validated,
        Err(errors) => return Ok(Err(errors.into_response())),
    };
    Ok(Ok(body(&validated)))
}

/// `ModelSerializer.validated_data` → the columns the save writes.
fn body(validated: &Validated) -> Body {
    Body {
        name: validated.text("name").map(str::to_owned),
        color: validated.text("color").map(str::to_owned),
        description: validated.text("description").map(str::to_owned),
        external_source: validated
            .contains("external_source")
            .then(|| validated.text("external_source").map(str::to_owned)),
        external_id: validated
            .contains("external_id")
            .then(|| validated.text("external_id").map(str::to_owned)),
        parent: validated
            .contains("parent")
            .then(|| validated.uuid("parent")),
        sort_order: validated.float("sort_order"),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `request.data.get("external_id") and …` 409 check for create/patch
/// (`plane/api/views/issue.py:1003-1022`, `:1141-1152`).
async fn external_conflict(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    existing: Option<&Label>,
    data: &plane_drf::parsers::Data,
) -> Result<Option<ViewResponse>, ViewError> {
    let Some(external_id) = body_get(data, "external_id").filter(truthy) else {
        return Ok(None);
    };
    // A create requires both members; a patch accepts a missing
    // `external_source` and compares against the instance's value.
    let source = match body_get(data, "external_source").filter(truthy) {
        Some(source) => python_str(&source),
        None => match existing {
            Some(label) => label.external_source.clone().unwrap_or_default(),
            None => return Ok(None),
        },
    };
    let id_text = python_str(&external_id);
    match existing {
        // `not (state.external_id != str(request.data.get("external_id")))`
        Some(label) if label.external_id.as_deref() == Some(id_text.as_str()) => Ok(None),
        Some(label) => {
            let statement = client
                .prepare_cached(queries::LABEL_EXTERNAL_EXISTS_EXCLUDING.sql)
                .await?;
            if client
                .query_opt(
                    &statement,
                    &[&project_id, &slug, &source, &id_text, &label.id],
                )
                .await?
                .is_none()
            {
                return Ok(None);
            }
            conflict(
                "Label with the same external id and external source already exists",
                label.id,
            )
            .map(Some)
        }
        None => {
            let statement = client
                .prepare_cached(queries::LABEL_EXTERNAL_EXISTS.sql)
                .await?;
            if client
                .query_opt(&statement, &[&project_id, &slug, &source, &id_text])
                .await?
                .is_none()
            {
                return Ok(None);
            }
            let statement = client
                .prepare_cached(queries::LABEL_EXTERNAL_FIRST.sql)
                .await?;
            let Some(row) = client
                .query_opt(&statement, &[&slug, &project_id, &id_text, &source])
                .await?
            else {
                // `.first()` on a row the `exists()` just found: unreachable
                // unless a concurrent delete ran.
                return Err(ViewError::Unknown(
                    "AttributeError: 'NoneType' object has no attribute 'id'".to_owned(),
                ));
            };
            let label = Label::from_row(&row, 0)?;
            conflict(
                "Label with the same external id and external source already exists",
                label.id,
            )
            .map(Some)
        }
    }
}

/// `Response({"error": …, "id": str(...)}, 409)`.
fn conflict(message: &str, id: Uuid) -> Result<ViewResponse, ViewError> {
    let mut body = PyDict::new();
    body.insert(PyStr::new("error"), Json::Str(PyStr::from(message)));
    body.insert(PyStr::new("id"), Json::Str(PyStr::from(id.to_string())));
    let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &Json::Dict(body))
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    Ok(ViewResponse::raw(409, APPLICATION_JSON, rendered))
}

/// `Label.objects.filter(pk=…)` re-fetch.
async fn label_by_id(client: &Client, id: Uuid) -> Result<Option<Label>, ViewError> {
    let statement = client.prepare_cached(queries::LABEL_BY_ID.sql).await?;
    let row = client.query_opt(&statement, &[&id]).await?;
    row.as_ref()
        .map(|row| Label::from_row(row, 0))
        .transpose()
        .map_err(ViewError::from)
}

/// `self.get_queryset().get(pk=…)` / `get_object()`.
async fn label_in_project(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    pk: Uuid,
    member: Uuid,
) -> Result<Option<Label>, ViewError> {
    let statement = client.prepare_cached(queries::LABEL_IN_PROJECT.sql).await?;
    let row = client
        .query_opt(&statement, &[&slug, &project_id, &member, &pk])
        .await?;
    row.as_ref()
        .map(|row| Label::from_row(row, 0))
        .transpose()
        .map_err(ViewError::from)
}

/// `Project.objects.get(pk=…)`'s workspace half, for `WorkspaceBaseModel.save`
/// (`plane/db/models/workspace.py:192-195`).
async fn project_workspace(client: &Client, project_id: Uuid) -> Result<Option<Uuid>, ViewError> {
    let statement = client
        .prepare_cached(plane_http_app::labels::queries::PROJECT_FOR_LABEL.sql)
        .await?;
    Ok(client
        .query_opt(&statement, &[&project_id])
        .await?
        .map(|row| row.get(1)))
}

/// `LabelSerializer(label)` — the read serializer.
fn serializer(label: &Label) -> LabelSerializer<'_> {
    LabelSerializer {
        id: label.id,
        created_at: label.created_at,
        updated_at: label.updated_at,
        deleted_at: label.deleted_at,
        name: &label.name,
        description: &label.description,
        color: &label.color,
        sort_order: label.sort_order,
        external_source: label.external_source.as_deref(),
        external_id: label.external_id.as_deref(),
        created_by: label.created_by_id,
        updated_by: label.updated_by_id,
        workspace: label.workspace_id,
        project: label.project_id,
        parent: label.parent_id,
    }
}

/// `LabelSerializer(label).data` for the create/patch responses, with no
/// `fields=`/`expand=`.
fn render_created(
    ctx: &ViewCtx<'_>,
    label: &Label,
    status: u16,
) -> Result<ViewResponse, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let body = plane_serializers::render(
        &serializer(label),
        &plane_serializers::RenderCtx::full(&timezone),
    )
    .map_err(ViewError::from)?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// The list's `LabelSerializer(..., fields=…, expand=…)`.
async fn render_labels(
    _ctx: &ViewCtx<'_>,
    client: &Client,
    labels: &[Label],
    requested: &[&str],
    prepared: &RequestRender<'_>,
) -> Result<Vec<u8>, ViewError> {
    let serializers: Vec<LabelSerializer<'_>> = labels.iter().map(serializer).collect();
    if requested.is_empty() {
        return plane_http_app::common::render_list(&serializers, &prepared.render_ctx());
    }

    // `?expand=parent` renders a Label through `IssueLiteSerializer`
    // (`plane/api/serializers/base.py:111`), whose `sequence_id` a Label does
    // not have. A non-null parent is the AttributeError → family 500; a NULL
    // one stays null (`:126-132`).
    if requested.contains(&"parent") && labels.iter().any(|label| label.parent_id.is_some()) {
        return Err(ViewError::Unknown(
            "AttributeError: 'Label' object has no attribute 'sequence_id'".to_owned(),
        ));
    }

    let mut workspace_ids: HashSet<Uuid> = HashSet::new();
    let mut project_ids: HashSet<Uuid> = HashSet::new();
    let mut user_ids: HashSet<Uuid> = HashSet::new();
    for label in labels {
        if requested.contains(&"workspace") {
            workspace_ids.insert(label.workspace_id);
        }
        if let Some(project) = label.project_id
            && requested.contains(&"project")
        {
            project_ids.insert(project);
        }
        if requested.contains(&"created_by") {
            user_ids.extend(label.created_by_id);
        }
        if requested.contains(&"updated_by") {
            user_ids.extend(label.updated_by_id);
        }
    }
    let mut workspaces = lite::load_workspaces(client, &workspace_ids).await?;
    let users = lite::load_users(client, &user_ids).await?;
    let projects = load_projects(client, &project_ids).await?;
    let mut asset_ids = lite::asset_ids(
        &HashMap::<Uuid, lite::LiteProject>::new(),
        &workspaces,
        &users,
    );
    asset_ids.extend(
        projects
            .values()
            .filter_map(|project| project.cover_image_asset_id),
    );
    let urls = lite::load_asset_urls(client, &asset_ids).await?;
    lite::apply_workspaces(&mut workspaces, &urls)?;

    let mut out = Vec::with_capacity(serializers.len() * 512 + 2);
    out.push(b'[');
    for (index, (label, value)) in labels.iter().zip(&serializers).enumerate() {
        if index > 0 {
            out.push(b',');
        }
        let workspace = workspaces
            .get(&label.workspace_id)
            .map(|row| WorkspaceLiteSerializer {
                name: &row.name,
                slug: &row.slug,
                id: row.id,
            });
        let project = label
            .project_id
            .and_then(|id| projects.get(&id))
            .map(project_serializer);
        let created_by = label
            .created_by_id
            .and_then(|id| users.get(&id))
            .map(|user| UserLiteSerializer {
                id: user.id,
                first_name: &user.first_name,
                last_name: &user.last_name,
                email: user.email.as_deref(),
                avatar: &user.avatar,
                avatar_url: user.avatar_url.as_deref(),
                display_name: &user.display_name,
            });
        let updated_by = label
            .updated_by_id
            .and_then(|id| users.get(&id))
            .map(|user| UserLiteSerializer {
                id: user.id,
                first_name: &user.first_name,
                last_name: &user.last_name,
                email: user.email.as_deref(),
                avatar: &user.avatar,
                avatar_url: user.avatar_url.as_deref(),
                display_name: &user.display_name,
            });

        let mut resolved: Vec<Expansion<'_>> = Vec::new();
        if let Some(workspace) = &workspace {
            resolved.push(Expansion {
                name: "workspace",
                value: plane_serializers::DrfValue::nested(workspace),
            });
        }
        if let Some(project) = &project {
            resolved.push(Expansion {
                name: "project",
                value: plane_serializers::DrfValue::nested(project),
            });
        }
        for (name, user) in [("created_by", &created_by), ("updated_by", &updated_by)] {
            if let Some(user) = user {
                resolved.push(Expansion {
                    name,
                    value: plane_serializers::DrfValue::nested(user),
                });
            }
        }
        let values = expansions(requested, &resolved, |name| match name {
            "workspace" => Some(label.workspace_id),
            "project" => label.project_id,
            "created_by" => label.created_by_id,
            "updated_by" => label.updated_by_id,
            _ => None,
        });
        let render_ctx = prepared
            .render_ctx()
            .with_expansions(Expansions::new(requested, &values));
        out.extend_from_slice(
            &plane_serializers::render(value, &render_ctx).map_err(ViewError::from)?,
        );
    }
    out.push(b']');
    Ok(out)
}

/// `ProjectLiteSerializer` fields the v1 serializer renders.
pub struct ProjectLite {
    pub id: Uuid,
    pub identifier: String,
    pub name: String,
    pub cover_image: Option<String>,
    pub cover_image_asset_id: Option<Uuid>,
    pub cover_image_url: Option<String>,
    pub icon_prop: Option<plane_db::JsonText>,
    pub emoji: Option<String>,
    pub description: String,
    pub archived_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn project_serializer(project: &ProjectLite) -> ProjectLiteSerializer<'_> {
    ProjectLiteSerializer {
        id: project.id,
        identifier: &project.identifier,
        name: &project.name,
        cover_image: project.cover_image.as_deref(),
        icon_prop: project.icon_prop.as_ref().map(PgJson::from),
        emoji: project.emoji.as_deref(),
        description: &project.description,
        cover_image_url: project.cover_image_url.as_deref(),
        archived_at: project.archived_at,
    }
}

/// The v1 `ProjectLiteSerializer`'s columns. The app surface's
/// `projects::queries::PROJECTS_BY_IDS` cannot be reused: it omits
/// `icon_prop`/`emoji`/`archived_at`, which this serializer renders.
pub async fn load_projects(
    client: &Client,
    ids: &HashSet<Uuid>,
) -> Result<HashMap<Uuid, ProjectLite>, ViewError> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let wanted: Vec<Uuid> = ids.iter().copied().collect();
    let statement = client
        .prepare_cached(queries::PROJECT_LITE_BY_IDS.sql)
        .await?;
    let rows = client.query(&statement, &[&wanted]).await?;
    let mut projects = HashMap::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.get(0);
        projects.insert(
            id,
            ProjectLite {
                id,
                identifier: row.get(1),
                name: row.get(2),
                cover_image: row.get(3),
                cover_image_asset_id: row.get(4),
                cover_image_url: None,
                icon_prop: row.get(5),
                emoji: row.get(6),
                description: row.get(7),
                archived_at: row.get(8),
            },
        );
    }
    Ok(projects)
}

/// `Label.objects` (`SoftDeletionManager`): 23 = SQLSTATE 23*.
fn is_integrity(error: &tokio_postgres::Error) -> bool {
    error
        .code()
        .is_some_and(|code| code.code().starts_with("23"))
}

/// Python truthiness for the `external_*` guards.
fn truthy(value: &Json) -> bool {
    plane_http_app::assets::workspace_write::truthy(value)
}

/// The route's `request.user.id`.
fn member_id(ctx: &ViewCtx<'_>) -> Result<Uuid, ViewError> {
    ctx.user
        .id()
        .ok_or(ViewError::Api(ApiError::NotAuthenticated))
}
