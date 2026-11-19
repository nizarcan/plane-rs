// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WorkSpaceViewSet`'s write actions (`plane/app/views/workspace/base.py:82-173`)
//! — workspace-core set A.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 186 `POST /api/workspaces/` | `POST` | `create` (`base.py:82-151`) | [`create`] |
//! | 187 `PUT /api/workspaces/{slug}/` | `PUT` | `UpdateModelMixin.update` (`base.py:157` guarded by `WorkSpaceBasePermission`) | [`update`] |
//! | 187 | `PATCH` | `partial_update` (`base.py:157-159`) | [`partial_update`] |
//! | 187 | `DELETE` | `destroy` (`base.py:161-173`) | [`destroy`] |
//!
//! ## 186 — three early checks before the serializer
//!
//! `create` reads `request.data.get("slug"/"name")` **before** `is_valid`, and
//! the three checks are raw Python (`base.py:101-120`): a missing value is a
//! bare `{"error": …}`, `len()` over 80/48 is the family's 400, and
//! `contains_url(name)` shadows the serializer's `validate_name` URL branch. A
//! body that is not a mapping is an `AttributeError` inside `.get`, i.e. the
//! family 500 — after the `DISABLE_WORKSPACE_CREATION` read, which is the first
//! statement of the view.
//!
//! The two INSERTs are **not** in a transaction (no `atomic`, and
//! `ATOMIC_REQUESTS` is unset), so a failure after the workspace row leaves it
//! committed — the `wsc-12` 409 in the scenario is that shape.
//!
//! One rendering quirk is reproduced rather than fixed: a `logo_asset` of a
//! non-static entity type whose own workspace is NULL makes `Workspace.logo_url`
//! raise `AttributeError` on `self.workspace.slug`
//! (`plane/db/models/asset.py:93-104`), and DRF's `Field.get_attribute` turns
//! that into `SkipField` — `logo_url` is **omitted** from the body, the request
//! still answers 201/200 (`rest_framework/fields.py:426-456`).
//!
//! ## 187 — `get_object` and the 16-key body
//!
//! `get_object()` is the viewset queryset (`base.py:64-80`): the caller's
//! **active** membership, `deleted_at IS NULL` on the workspace, and the
//! `total_members` annotation — which is why the update response has 16 keys
//! and `role` is skipped (nothing annotates it).
//!
//! PUT is a real update here: `guard_actions.update` is not refused (the class
//! permission `WorkSpaceBasePermission` authorizes admins), and the serializer
//! is built with `partial=False`, so `name` and `slug` are required. PATCH is
//! the same handler with `partial=True`. `deleted_at` is in
//! `Meta.read_only_fields`, so a body value is ignored.
//!
//! DELETE runs, in order: `get_object` → every `profiles.last_workspace_id`
//! pointing here set to NULL → `get_object` **again** (DRF's mixin) → the H03
//! soft delete → the cascade publish → H05's `slug = f"{slug}__{epoch}"`. The
//! slug rewrite is a second statement, so a 40-character slug overflows
//! `varchar(48)` *after* the row is deleted and the task is published — a 500
//! with the deletion committed (`MODEL_HOOKS.md:506-516`).
//!
//! ## The `slug` field
//!
//! The generated table leaves `slug` as `TODO(slug)`; the handler calls
//! [`Serializer::slug_field`] with the model's validator chain in DRF's order:
//! `slug_validator`'s restricted list → `UniqueValidator` (live rows only) →
//! `MaxLengthValidator` → SlugField's own regex. `validate_slug`
//! (`serializers/workspace.py:61-70`) is dead in Python (both of its checks are
//! implied by the first and last links of that chain) and is not ported.

use plane_core::error::APPLICATION_JSON;
use plane_core::json::Object;
use plane_db::JsonText;
use plane_db::hooks::audit::Audit;
use plane_db::models::workspace::Workspace;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use plane_serializers::writer::{DrfWriter, ObjectWriter};
use plane_serializers::{DrfUuid, drf_datetime};
use uuid::Uuid;

use crate::common::{asset_url, instance_config};
use crate::views::write_support::{Body, body_get, integrity, is_integrity};
use crate::workspaces::base::RESTRICTED_WORKSPACE_SLUGS;
use crate::workspaces::rows;
use crate::workspaces::write_a_queries as queries;
use crate::workspaces::write_queries as shared_queries;
use crate::write::{self, Serializer, SlugOptions};

/// `plane.app.views.workspace.base.WorkSpaceViewSet`.
const VIEW: &str = "plane.app.views.workspace.base.WorkSpaceViewSet";

/// The role a workspace creator gets (`base.py:128,136`): `ROLE.ADMIN`.
const ROLE_ADMIN: i64 = 20;

/// `WorkSpaceSerializer`'s writable fields in `self.fields` order — the order
/// DRF reports `serializer.errors` in.
///
/// The generated table declares them in the same order; the test below asserts
/// it, because the sort is what makes an error appended by [`Serializer::slug_field`]
/// land in `slug`'s position rather than at the end.
const FIELD_ORDER: &[&str] = &[
    "name",
    "logo",
    "slug",
    "organization_size",
    "timezone",
    "background_color",
    "logo_asset",
];

/// Register the write verbs of routes 186 and 187.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset(VIEW)?
        .action("create", create)?
        .action("update", update)?
        .action("partial_update", partial_update)?
        .action("destroy", destroy)?;
    Ok(())
}

/// A `workspaces` row plus the logo asset columns the response reads.
struct WorkspaceRow {
    model: Workspace,
    asset: asset_url::AssetRef,
    /// `None` for a row that did not come through the annotated queryset.
    total_members: Option<i64>,
}

impl WorkspaceRow {
    /// Decode [`queries::WORKSPACE_FOR_MEMBER_ASSET`].
    fn from_member_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            model: Workspace::from_row(row, 0)?,
            asset: asset_url::AssetRef::from_row(row, 14)?,
            total_members: Some(row.try_get(19)?),
        })
    }
}

/// `Workspace.logo_url` as `serializer.data` sees it
/// (`plane/db/models/workspace.py:145-154`).
#[derive(Debug)]
enum LogoUrl {
    /// The property returned a value (possibly `None`).
    Value(Option<String>),
    /// The property raised `AttributeError`, and DRF's `Field.get_attribute`
    /// turned that into `SkipField` for a non-required field — the key is
    /// **omitted** from `serializer.data`, not `null`
    /// (`rest_framework/fields.py:426-456`). Reachable through a `logo_asset`
    /// of a non-static entity type whose own workspace is NULL: the description
    /// branch reads `self.workspace.slug` and finds `None`
    /// (`plane/db/models/asset.py:93-104`).
    Omitted,
}

/// `Workspace.logo_url` for a write response, where the asset row is loaded.
///
/// The shared `asset_url::workspace_logo_url` reports that `AttributeError` as
/// a `ViewError`; here it is not an error the view sees, because the serializer
/// swallows it.
fn logo_url(logo: Option<&str>, asset: &asset_url::AssetRef) -> LogoUrl {
    if asset.is_some() {
        return match asset.asset_url() {
            Ok(value) => LogoUrl::Value(value),
            // The only error `asset_url` produces is the missing workspace's
            // attribute access above.
            Err(_) => LogoUrl::Omitted,
        };
    }
    LogoUrl::Value(logo.filter(|logo| !logo.is_empty()).map(str::to_owned))
}

/// How `serializer.data` places the `total_members`/`role` keys.
#[derive(Debug, Clone, Copy)]
enum Appended {
    /// 187: the `total_members` annotation sits in its declared position and
    /// `role` is skipped (no attribute).
    InPlace,
    /// 186: the two keys are appended **after** `owner` (`base.py:134-136`).
    AtEnd { total_members: i64, role: i64 },
}

/// The five asset columns of [`queries::LOGO_ASSET_REF`].
async fn logo_asset_ref(
    client: &deadpool_postgres::Client,
    id: Uuid,
) -> Result<asset_url::AssetRef, ViewError> {
    let statement = queries::LOGO_ASSET_REF.prepare(client).await?;
    match client.query_opt(&statement, &[&id]).await? {
        Some(row) => Ok(asset_url::AssetRef::from_row(&row, 0)?),
        // The FK row vanished between DRF's lookup and the render; the Python
        // would read the cached instance instead. An empty ref is "no asset".
        None => Ok(asset_url::AssetRef::default()),
    }
}

/// `get_configuration_value([{"key": "DISABLE_WORKSPACE_CREATION", …}])[0]`
/// (`plane/license/utils/instance_value.py:17-39`, call at `base.py:84-91`).
///
/// # Errors
///
/// A database failure, as the family's 500.
async fn workspace_creation_disabled(ctx: &ViewCtx<'_>) -> Result<bool, ViewError> {
    let value =
        instance_config::configuration_value(ctx, "DISABLE_WORKSPACE_CREATION", "0").await?;
    Ok(value.as_deref() == Some("1"))
}

/// `request.data.get("name", False)` truthiness and `len()` for the early
/// checks. Python truthiness only (`""`, `0`, `[]`, `{}`, `None` are falsy).
fn body_truthy(value: &Body<'_>) -> bool {
    match value {
        Body::Missing => false,
        Body::Form(text) => !text.is_empty(),
        Body::Json(Json::Null) => false,
        Body::Json(Json::Bool(flag)) => *flag,
        Body::Json(Json::Int(number)) => number.as_i64().is_some_and(|value| value != 0),
        Body::Json(Json::Float(number)) => *number != 0.0,
        Body::Json(Json::Str(text)) => text.as_str().is_some_and(|text| !text.is_empty()),
        Body::Json(Json::List(items)) => !items.is_empty(),
        Body::Json(Json::Dict(entries)) => !entries.is_empty(),
    }
}

/// `len(value)` after the truthiness test: only a string, list or dict reaches
/// it, anything else is the `TypeError` the view 500s on (`base.py:110`).
fn body_len(value: &Body<'_>) -> Result<usize, ViewError> {
    match value {
        Body::Form(text) => Ok(text.chars().count()),
        Body::Json(Json::Str(text)) => Ok(text.char_len()),
        Body::Json(Json::List(items)) => Ok(items.len()),
        Body::Json(Json::Dict(entries)) => Ok(entries.len()),
        Body::Missing => Ok(0),
        Body::Json(other) => Err(ViewError::Unknown(format!(
            "TypeError: object of type '{}' has no len()",
            python_type_name(other)
        ))),
    }
}

/// `type(value).__name__`, for the `TypeError` above.
fn python_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "NoneType",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::List(_) => "list",
        Json::Dict(_) => "dict",
    }
}

/// `contains_url(value)` over a body value (`plane/utils/url.py:32-53`):
/// `len(value) > 1000` then `value.split("\n")`, so a non-string is the
/// `AttributeError` the view 500s on.
fn body_contains_url(value: &Body<'_>) -> Result<bool, ViewError> {
    match value {
        Body::Form(text) => Ok(write::email::contains_url(text)),
        Body::Json(Json::Str(text)) => Ok(text.as_str().is_some_and(write::email::contains_url)),
        Body::Missing => Ok(false),
        Body::Json(other) => Err(ViewError::Unknown(format!(
            "AttributeError: '{}' object has no attribute 'split'",
            python_type_name(other)
        ))),
    }
}

/// `WorkSpaceSerializer.validate_name` (`plane/app/serializers/workspace.py:48-59`),
/// the `validate_<field>` hook the generated table cannot express.
///
/// DRF runs it only when the field's own validation produced a value, after the
/// field's validators (`rest_framework/serializers.py:430-445`); a failure
/// replaces the value with one `name` error. On a create the URL branch is
/// shadowed by the view's raw `contains_url` check (`base.py:116-120`), which
/// runs first and rejects with a different body.
fn validate_name(serializer: &mut Serializer<'_>) {
    let Some(name) = serializer.value("name").and_then(|value| match value {
        write::Value::Text(text) => Some(text.clone()),
        _ => None,
    }) else {
        return;
    };
    // `contains_url(value)` (`plane/utils/url.py:32-53`).
    if write::email::contains_url(&name) {
        serializer.push_error(
            "name",
            write::Detail::new("Name must not contain URLs", "invalid"),
        );
        return;
    }
    // `has_alphanumeric(value)` (`plane/utils/content_validator.py:246-261`):
    // CPython's `str.isalnum` per character, so any script's letter or digit
    // counts and a symbol-only name does not.
    if !name.chars().any(char::is_alphanumeric) {
        serializer.push_error(
            "name",
            write::Detail::new("Name must contain at least one letter or number", "invalid"),
        );
    }
}

/// `str(request.data.get("company_role", ""))` for the membership row.
///
/// A missing key is `""`; an explicit `null` stays None (`TextField` NULL); any
/// other value goes through Django's `TextField.get_prep_value`, i.e. Python's
/// `str()` — the repr for everything but a string.
fn company_role(value: &Body<'_>) -> Option<String> {
    match value {
        Body::Missing => Some(String::new()),
        Body::Form(text) => Some((*text).to_string()),
        Body::Json(Json::Null) => None,
        Body::Json(Json::Str(text)) => Some(text.as_str().unwrap_or_default().to_string()),
        Body::Json(other) => Some(write::repr::python_repr(other)),
    }
}

/// `"already exists" in str(IntegrityError)` (`base.py:147`).
///
/// psycopg's exception text is the server's: the constraint in `message()` and
/// the colliding key in `detail()`, which is where Postgres puts the words for
/// a unique violation.
fn reports_already_exists(error: &tokio_postgres::Error) -> bool {
    let Some(db_error) = error.as_db_error() else {
        return false;
    };
    db_error.message().contains("already exists")
        || db_error
            .detail()
            .is_some_and(|detail| detail.contains("already exists"))
}

/// The 400 body `{"error": …}` a view returns itself.
fn error_body(status: u16, message: &str) -> Result<ViewResponse, ViewError> {
    Ok(write::error_body(status, message))
}

/// The `except IntegrityError` arm of 186 (`base.py:146-151`).
///
/// A unique violation whose text says "already exists" is the 409 with a
/// **bare string** value (`{"slug": "The workspace with the slug already
/// exists"}`); every other database error escapes the handler, and DRF answers
/// the implicit `return None` with a 500. Both INSERTs of the view sit inside
/// the same `try`, so the membership insert uses this arm too.
fn create_db_error(error: tokio_postgres::Error) -> Result<ViewResponse, ViewError> {
    if is_integrity(&error) && reports_already_exists(&error) {
        let body = plane_serde::to_vec(
            plane_serde::Dialect::Drf,
            &Object::new().field(
                "slug",
                "The workspace with the slug already exists".to_string(),
            ),
        )?;
        return Ok(ViewResponse::raw(409, APPLICATION_JSON, body));
    }
    Err(ViewError::from(error))
}

/// `WorkSpaceViewSet.create` (`plane/app/views/workspace/base.py:82-151`).
fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if workspace_creation_disabled(ctx).await? {
            return error_body(403, "Workspace creation is not allowed");
        }
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = crate::common::client(ctx)?;

        // `WorkSpaceSerializer(data=request.data)` parses the body first; a
        // malformed one is DRF's 400 before any of the raw checks run.
        let data = ctx.data()?;

        // `slug = request.data.get("slug", False)` / `name = …`
        // (`base.py:101-102`).
        let name_value = body_get(data, "name")?;
        let slug_value = body_get(data, "slug")?;
        if !body_truthy(&name_value) || !body_truthy(&slug_value) {
            return error_body(400, "Both name and slug are required");
        }
        if body_len(&name_value)? > 80 || body_len(&slug_value)? > 48 {
            return error_body(400, "The maximum length for name is 80 and for slug is 48");
        }
        if body_contains_url(&name_value)? {
            return error_body(400, "Name cannot contain a URL");
        }

        let mut serializer = Serializer::new(ctx, false)?;
        write::generated::work_space_serializer::validate(&mut serializer, client).await?;
        validate_name(&mut serializer);
        // The generator leaves `slug` to the handler (`TODO(slug)`): DRF's
        // `SlugField` chain is the validator order `slug_field` documents.
        let _ = serializer
            .slug_field(
                client,
                "slug",
                SlugOptions::required(48)
                    .restricted(RESTRICTED_WORKSPACE_SLUGS)
                    .unique(
                        &shared_queries::WORKSPACE_SLUG_TAKEN,
                        "Workspace with this slug already exists.",
                        None,
                    ),
            )
            .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(mut errors) => {
                errors.sort_by_field_order(FIELD_ORDER);
                return Ok(errors.into_response());
            }
        };

        // `serializer.save(owner=request.user)` — the validated fields plus
        // `owner`, the model defaults the serializer left absent, H01 on create.
        let now = write::now();
        let workspace = Workspace {
            created_at: now,
            updated_at: now,
            created_by_id: Some(user),
            updated_by_id: None,
            deleted_at: None,
            id: Uuid::new_v4(),
            name: values.text("name").unwrap_or_default().to_string(),
            logo: values.text("logo").map(str::to_string),
            logo_asset_id: values.uuid("logo_asset"),
            owner_id: user,
            slug: values.text("slug").unwrap_or_default().to_string(),
            organization_size: values.text("organization_size").map(str::to_string),
            timezone: values.text("timezone").unwrap_or("UTC").to_string(),
            // `background_color = models.CharField(default=get_random_color)`.
            background_color: values
                .text("background_color")
                .map(str::to_string)
                .unwrap_or_else(random_color),
        };

        let statement = queries::WORKSPACE_INSERT.prepare(client).await?;
        if let Err(error) = client
            .execute(
                &statement,
                &[
                    &workspace.created_at,
                    &workspace.updated_at,
                    &workspace.created_by_id,
                    &workspace.updated_by_id,
                    &workspace.deleted_at,
                    &workspace.id,
                    &workspace.name,
                    &workspace.logo,
                    &workspace.logo_asset_id,
                    &workspace.owner_id,
                    &workspace.slug,
                    &workspace.organization_size,
                    &workspace.timezone,
                    &workspace.background_color,
                ],
            )
            .await
        {
            return create_db_error(error);
        }

        // `WorkspaceMember.objects.create(workspace_id=…, member=request.user,
        // role=20, company_role=request.data.get("company_role", ""))`
        // (`base.py:125-130`).
        let member_id = Uuid::new_v4();
        let member_role: i16 = 20;
        let company_role = company_role(&body_get(data, "company_role")?);
        let default_props = JsonText::new(default_props());
        let issue_props = JsonText::new(issue_props());
        let statement = queries::WORKSPACE_MEMBER_CREATE.prepare(client).await?;
        if let Err(error) = client
            .execute(
                &statement,
                &[
                    &now,
                    &now,
                    &member_id,
                    &member_role,
                    &Some(user),
                    &user,
                    &workspace.id,
                    &company_role,
                    &default_props,
                    &default_props,
                    &issue_props,
                ],
            )
            .await
        {
            return create_db_error(error);
        }

        // `serializer.data` is computed on its first access, which Python's
        // count line performs (`base.py:133`), so `logo_url` resolves before the
        // count query runs.
        let asset = match workspace.logo_asset_id {
            Some(id) => logo_asset_ref(client, id).await?,
            None => asset_url::AssetRef::default(),
        };
        let row = WorkspaceRow {
            model: workspace,
            asset,
            total_members: None,
        };

        let statement = queries::WORKSPACE_MEMBER_COUNT.prepare(client).await?;
        let row_count = client.query_one(&statement, &[&row.model.id]).await?;
        let total_members: i64 = row_count.get(0);

        // `workspace_seed.delay(serializer.data["id"])`: `serializer.data["id"]`
        // is a **UUID object** (`PrimaryKeyRelatedField` without a queryset
        // returns `value.pk`), which kombu encodes as a `uuid` extension.
        let task = plane_jobs::celery::task_name::WORKSPACE_SEED;
        let message = plane_jobs::celery::tasks::workspace_seed(row.model.id);
        crate::common::jobs::dispatch_task(
            ctx,
            task,
            &message,
            plane_jobs::queue::Job::from_message(&message),
        )
        .await?;

        // `data = serializer.data; data["total_members"] = …; data["role"] = 20`
        // appends the two keys **after** the 15 serializer keys (`base.py:134-136`).
        let body = render_workspace(
            ctx,
            &row,
            Appended::AtEnd {
                total_members,
                role: ROLE_ADMIN,
            },
        )?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `PUT` (`UpdateModelMixin.update` with `partial=False`).
fn update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { update_workspace(ctx, false).await })
}

/// `PATCH` (`base.py:157-159` → `UpdateModelMixin.partial_update`).
fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move { update_workspace(ctx, true).await })
}

/// The shared body of 187's PUT and PATCH.
async fn update_workspace(ctx: &ViewCtx<'_>, partial: bool) -> Result<ViewResponse, ViewError> {
    let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
    let user = ctx
        .user
        .id()
        .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
    let client = crate::common::client(ctx)?;

    let Some(mut row) = get_object(client, slug, user).await? else {
        return rows::not_found_detail("Workspace");
    };

    let mut serializer = Serializer::new(ctx, partial)?;
    write::generated::work_space_serializer::validate(&mut serializer, client).await?;
    validate_name(&mut serializer);
    let _ = serializer
        .slug_field(
            client,
            "slug",
            SlugOptions::required(48)
                .restricted(RESTRICTED_WORKSPACE_SLUGS)
                .unique(
                    &shared_queries::WORKSPACE_SLUG_TAKEN,
                    "Workspace with this slug already exists.",
                    Some(row.model.id),
                ),
        )
        .await?;
    let values = match serializer.finish() {
        Ok(values) => values,
        Err(mut errors) => {
            errors.sort_by_field_order(FIELD_ORDER);
            return Ok(errors.into_response());
        }
    };

    // `ModelSerializer.update`: assign every validated field, then `save()`.
    if let Some(name) = values.text("name") {
        row.model.name = name.to_string();
    }
    if values.contains("logo") {
        row.model.logo = values.text("logo").map(str::to_string);
    }
    if let Some(slug) = values.text("slug") {
        row.model.slug = slug.to_string();
    }
    if values.contains("organization_size") {
        row.model.organization_size = values.text("organization_size").map(str::to_string);
    }
    if let Some(timezone) = values.text("timezone") {
        row.model.timezone = timezone.to_string();
    }
    if let Some(background_color) = values.text("background_color") {
        row.model.background_color = background_color.to_string();
    }
    if values.contains("logo_asset") {
        let new_id = values.uuid("logo_asset");
        row.model.logo_asset_id = new_id;
        // The FK descriptor's cache is replaced by DRF's field, so `logo_url`
        // reads the new asset.
        row.asset = match new_id {
            Some(id) => logo_asset_ref(client, id).await?,
            None => asset_url::AssetRef::default(),
        };
    }

    // H01 on update: `updated_at` from `auto_now`, `updated_by` from the user;
    // `created_at`, `created_by` and `deleted_at` keep their stored values.
    row.model.updated_at = write::now();
    row.model.updated_by_id = Audit::on_update(Some(user));

    let statement = queries::WORKSPACE_SAVE.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &row.model.created_at,
                &row.model.updated_at,
                &row.model.created_by_id,
                &row.model.updated_by_id,
                &row.model.deleted_at,
                &row.model.name,
                &row.model.logo,
                &row.model.logo_asset_id,
                &row.model.owner_id,
                &row.model.slug,
                &row.model.organization_size,
                &row.model.timezone,
                &row.model.background_color,
                &row.model.id,
            ],
        )
        .await
        .map_err(integrity)?;

    let body = render_workspace(ctx, &row, Appended::InPlace)?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

/// `WorkSpaceViewSet.destroy` (`plane/app/views/workspace/base.py:161-173`).
fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = crate::common::client(ctx)?;

        // `workspace = self.get_object()` (`:171`).
        let Some(workspace) = get_object(client, slug, user).await? else {
            return rows::not_found_detail("Workspace");
        };

        // `self.remove_last_workspace_ids_from_user_settings(workspace.id)`
        // (`:172`, `:161-165`): a queryset UPDATE, no audit columns.
        let statement = queries::PROFILES_CLEAR_LAST_WORKSPACE
            .prepare(client)
            .await?;
        client.execute(&statement, &[&workspace.model.id]).await?;

        // `super().destroy()` calls `self.get_object()` a second time
        // (`rest_framework/mixins.py:90-95`).
        let Some(workspace) = get_object(client, slug, user).await? else {
            return rows::not_found_detail("Workspace");
        };

        // H03: `deleted_at = now`, a full save whose observable columns are
        // `updated_at`/`updated_by`, then the cascade publish.
        let now = write::now();
        write::soft_delete(
            client,
            "workspaces",
            "id",
            workspace.model.id,
            Audit::on_update(Some(user)),
            now,
        )
        .await?;
        write::publish_soft_delete_cascade(ctx, "db", "workspace", workspace.model.id).await?;

        // H05: `self.slug = f"{self.slug}__{int(self.deleted_at.timestamp())}";
        // self.save(update_fields=["slug"])` (`plane/db/models/workspace.py:170-174`).
        // Only `slug` is written, and the length is checked by the column, so a
        // long slug fails **after** the row is deleted and the task published.
        let new_slug = plane_db::hooks::workspace::after_soft_delete(&workspace.model.slug, now);
        let statement = client
            .prepare_cached(plane_db::hooks::workspace::UPDATE_SLUG_SQL)
            .await?;
        client
            .execute(&statement, &[&new_slug, &workspace.model.id])
            .await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `get_object()` over the viewset queryset (`base.py:64-80,171`).
///
/// `Ok(None)` is the `Http404` `get_object_or_404` raises for a missing row:
/// DRF renders it as `{"detail": "No Workspace matches the given query."}`, not
/// the family `{"error": …}` a bare `DoesNotExist` would produce
/// (`django/shortcuts.py:get_object_or_404`,
/// `rest_framework/generics.py:18-21`). `.get()` on more than one row is
/// `MultipleObjectsReturned`, a 500.
async fn get_object(
    client: &deadpool_postgres::Client,
    slug: &str,
    user: Uuid,
) -> Result<Option<WorkspaceRow>, ViewError> {
    let statement = queries::WORKSPACE_FOR_MEMBER_ASSET.prepare(client).await?;
    let rows = client.query(&statement, &[&user, &slug]).await?;
    match rows.len() {
        0 => Ok(None),
        1 => Ok(Some(WorkspaceRow::from_member_row(&rows[0])?)),
        _ => Err(rows::multiple_objects("Workspace")),
    }
}

/// `serializer.data` for 186 and 187 — the same 15 keys in
/// `_readable_fields` order, with the two views' different handling of
/// `total_members`/`role` ([`Appended`]) and of a `logo_url` that raised
/// ([`LogoUrl::Omitted`]).
///
/// Built field by field instead of through the generated
/// `WorkSpaceSerializer` struct because both variants need a key *omitted* in a
/// way the struct cannot express (a `SkipField`), and 186 appends two keys
/// after `owner` rather than in their declared positions.
fn render_workspace(
    ctx: &ViewCtx<'_>,
    row: &WorkspaceRow,
    appended: Appended,
) -> Result<Vec<u8>, ViewError> {
    let tz = rows::timezone(ctx);
    let model = &row.model;
    let mut writer = DrfWriter::with_capacity(768);
    {
        let mut obj = writer.begin_object();
        field_uuid(&mut obj, "id", Some(model.id))?;
        if matches!(appended, Appended::InPlace) {
            // `annotate(total_members=…)`: present on the instance, so the key
            // is written in its declared position (`fields.py:530-551`).
            if let Some(total_members) = row.total_members {
                obj.field("total_members", &total_members)?;
            }
        }
        match logo_url(model.logo.as_deref(), &row.asset) {
            LogoUrl::Value(value) => obj.field("logo_url", &value)?,
            LogoUrl::Omitted => {}
        }
        field_datetime(&mut obj, "created_at", Some(model.created_at), &tz)?;
        field_datetime(&mut obj, "updated_at", Some(model.updated_at), &tz)?;
        field_datetime(&mut obj, "deleted_at", model.deleted_at, &tz)?;
        obj.field("name", &model.name)?;
        obj.field("logo", &model.logo)?;
        obj.field("slug", &model.slug)?;
        obj.field("organization_size", &model.organization_size)?;
        obj.field("timezone", &model.timezone)?;
        obj.field("background_color", &model.background_color)?;
        field_uuid(&mut obj, "created_by", model.created_by_id)?;
        field_uuid(&mut obj, "updated_by", model.updated_by_id)?;
        field_uuid(&mut obj, "logo_asset", model.logo_asset_id)?;
        field_uuid(&mut obj, "owner", Some(model.owner_id))?;
        if let Appended::AtEnd {
            total_members,
            role,
        } = appended
        {
            obj.field("total_members", &total_members)?;
            obj.field("role", &role)?;
        }
        obj.end();
    }
    Ok(writer.into_vec())
}

/// `PrimaryKeyRelatedField.to_representation` for one uuid column.
fn field_uuid(
    obj: &mut ObjectWriter<'_>,
    name: &str,
    value: Option<Uuid>,
) -> Result<(), ViewError> {
    obj.field(name, &value.map(DrfUuid))?;
    Ok(())
}

/// `DateTimeField.to_representation` in the active timezone.
fn field_datetime(
    obj: &mut ObjectWriter<'_>,
    name: &str,
    value: Option<chrono::DateTime<chrono::Utc>>,
    tz: &chrono_tz::Tz,
) -> Result<(), ViewError> {
    let rendered = value.map(|value| drf_datetime(value, tz));
    obj.field(name, &rendered)?;
    Ok(())
}

/// `plane.db.models.workspace.get_default_props` (`workspace.py:22-59`) — the
/// membership defaults. Shared with set B's invitation accept, which is why the
/// constant lives in `projects::invite_write`; this is the same string.
fn default_props() -> &'static str {
    crate::projects::invite_write::WORKSPACE_DEFAULT_PROPS
}

/// `plane.db.models.workspace.get_issue_props` (`workspace.py:110-111`).
fn issue_props() -> &'static str {
    crate::projects::invite_write::WORKSPACE_ISSUE_PROPS
}

/// `plane.utils.color.get_random_color` (`color.py:9`):
/// `"#" + "".join(random.choices(string.hexdigits, k=6))`.
///
/// `string.hexdigits` is `0123456789abcdefABCDEF`, so the case is mixed. The
/// harness normalises the column (`scenarios/normalize.yaml`, `hex_colour`).
fn random_color() -> String {
    format!(
        "#{}",
        plane_auth::random::get_random_string(6, "0123456789abcdefABCDEF")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_order_matches_the_generated_table() {
        let generated: Vec<&str> = write::generated::work_space_serializer::FIELDS
            .iter()
            .map(|field| field.name)
            .collect();
        assert_eq!(generated, FIELD_ORDER);
    }

    #[test]
    fn a_random_colour_is_six_hex_digits() {
        for _ in 0..20 {
            let colour = random_color();
            assert_eq!(colour.len(), 7, "{colour}");
            assert!(colour.starts_with('#'));
            assert!(
                colour[1..].chars().all(|c| c.is_ascii_hexdigit()),
                "{colour}"
            );
        }
    }

    #[test]
    fn python_truthiness_matches_the_early_checks() {
        // `not name` is True for every falsy JSON value, including `0` and `[]`.
        assert!(!body_truthy(&Body::Missing));
        assert!(!body_truthy(&Body::Json(&Json::Null)));
        assert!(!body_truthy(&Body::Json(&Json::from(0i64))));
        assert!(body_truthy(&Body::Json(&Json::from(1i64))));
        assert!(body_truthy(&Body::Json(&Json::from("x"))));
        assert!(!body_truthy(&Body::Json(&Json::from(""))));
        assert!(body_truthy(&Body::Form("x")));
        assert!(!body_truthy(&Body::Form("")));
    }

    #[test]
    fn len_of_a_number_is_the_python_type_error() {
        let error = body_len(&Body::Json(&Json::from(3i64))).expect_err("TypeError");
        assert!(
            matches!(error, ViewError::Unknown(ref message)
                if message.contains("object of type 'int' has no len()")),
            "{error:?}"
        );
    }
}
