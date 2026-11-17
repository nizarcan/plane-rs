// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/api.py` — the write verbs of the caller's API tokens
//! (`ApiTokenEndpoint`), set A of the `users-me` writes.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 226 `POST /api/users/api-tokens/` | `post` | `api.py:21-39` | [`post`] |
//! | 227 `PATCH /api/users/api-tokens/{pk}/` | `patch` | `api.py:56-62` | [`patch`] |
//! | 227 `DELETE /api/users/api-tokens/{pk}/` | `delete` | `api.py:51-54` | [`delete`] |
//! | 226 | `PATCH`/`DELETE` | the `pk` `TypeError` | [`patch`]/[`delete`] |
//! | 227 | `POST` | the `pk` `TypeError` | [`post`] |
//!
//! ## The three signature 500s
//!
//! `routes.json` maps all four verbs onto both URL shapes, and one class serves
//! both (`plane/app/urls/api.py:9-19`). `post(self, request)` has no `pk`
//! parameter while 227 passes one, and `patch`/`delete` require `pk`
//! (`api.py:51,56`) while 226 has no kwarg. DRF calls
//! `handler(request, *args, **kwargs)` (`rest_framework/views.py:517-523`), so
//! Python raises a `TypeError` before the body runs and Plane's family handler
//! answers `500 {"error": "Something went wrong please try again later"}`
//! (`views/base.py:330-334`). `IsAuthenticated` already passed, so only a
//! signed-in caller reaches the 500.
//!
//! ## 226 POST bypasses its serializer
//!
//! `label`, `description` and `expired_at` are raw `request.data` values
//! (`api.py:22-24`); only the response goes through `APITokenSerializer`. The
//! three columns are therefore coerced by Django's model fields at INSERT time
//! ([`django_text`], [`django_datetime`]), and their failures are the family's:
//! a `null` label/description is an `IntegrityError` → 400
//! `{"error": "The payload is not valid"}`, a label over 255 characters is a
//! psycopg `DataError` → 500, an unparseable `expired_at` is a `ValidationError`
//! → 400 `{"error": "Please provide valid detail"}`, and a *non-string*
//! `expired_at` is the `TypeError` `datetime.fromisoformat` raises → 500.
//!
//! ## The token is stored in plaintext
//!
//! `APIToken.token` defaults to `"plane_api_" + uuid4().hex`, `unique`, and is
//! never hashed (`plane/db/models/api.py:19,31`); `APIKeyAuthentication`
//! matches it verbatim (`plane/auth`'s `api_key.rs:20-35`). 128 random bits make
//! the `unique` collision a 500 rather than a retry. Only this create response
//! ever reveals the value (`APITokenReadSerializer` excludes it,
//! `plane/app/serializers/api.py:28-38`).
//!
//! ## Order, no transactions
//!
//! 226: read the three body keys → `APIToken.objects.create` (H01 + the two
//! `auto_now` timestamps) → render. 227 PATCH: lookup (family 404) → serializer
//! (400) → `save()` (H01 update + `auto_now`) → render. 227 DELETE: lookup →
//! H03 (`deleted_at`, a full save, the cascade publish) → 204; the body is
//! never parsed.

use chrono::{DateTime, Utc};
use plane_core::error::APPLICATION_JSON;
use plane_db::hooks::audit::Audit;
use plane_db::models::api::APIToken;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use plane_serializers::generated::app_api::APITokenSerializer;
use uuid::Uuid;

use super::write_queries;
use crate::common::client;
use crate::views::write_support::{Body, body_get};
use crate::write::generated::a_p_i_token_serializer;
use crate::write::repr::python_repr;
use crate::write::{self, Serializer};

// ---------------------------------------------------------------------------
// 226 POST /api/users/api-tokens/
// ---------------------------------------------------------------------------

/// `ApiTokenEndpoint.post` (`plane/app/views/api.py:21-39`).
pub fn post<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `post(self, request)` (`:21`) has no `pk`; the details route (227)
        // binds one, which DRF passes as a keyword and Python rejects.
        if ctx.kwarg("pk").is_some() {
            return Err(ViewError::Unknown(
                "TypeError: ApiTokenEndpoint.post() got an unexpected keyword argument 'pk'"
                    .to_string(),
            ));
        }
        let user = ctx
            .user
            .user()
            .ok_or(ViewError::Api(plane_core::ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `request.data.get(...)` three times, before any write.
        let data = ctx.data()?;
        // `label = request.data.get("label", str(uuid4().hex))` (`:22`).
        let label = match body_get(data, "label")? {
            Body::Missing => Uuid::new_v4().simple().to_string(),
            other => django_text(other)?,
        };
        // `description = request.data.get("description", "")` (`:23`).
        let description = match body_get(data, "description")? {
            Body::Missing => String::new(),
            other => django_text(other)?,
        };
        // `expired_at = request.data.get("expired_at", None)` (`:24`). The
        // body string is kept for the response: the instance keeps the raw
        // value Python assigned, and DRF's `DateTimeField.to_representation`
        // returns a `str` unchanged (`rest_framework/fields.py:1297-1315`), so
        // the 201 body echoes it while the INSERT stores the parsed instant.
        let (expired_at, expired_at_raw) = expired_at_field(body_get(data, "expired_at")?)?;

        // `user_type = 1 if request.user.is_bot else 0` (`:27`).
        let user_type: i16 = if user.is_bot { 1 } else { 0 };

        // `APIToken.objects.create(...)` (`:29-35`). `auto_now_add` then
        // `auto_now` are two `timezone.now()` calls in model order
        // (`django/db/models/fields/__init__.py:864-877`), then H01's create
        // half (`plane/db/models/base.py:36-39`): `created_by` is the caller and
        // `updated_by` stays NULL.
        let created_at = write::now();
        let updated_at = write::now();
        let audit = Audit::on_create(Some(user.id));
        let token = APIToken {
            created_at,
            updated_at,
            created_by_id: audit.created_by_id,
            updated_by_id: audit.updated_by_id,
            deleted_at: None,
            id: Uuid::new_v4(),
            label,
            description,
            is_active: true,
            last_used: None,
            // `generate_token()`: `"plane_api_" + uuid4().hex`
            // (`plane/db/models/api.py:19`).
            token: format!("plane_api_{}", Uuid::new_v4().simple()),
            user_id: user.id,
            user_type,
            workspace_id: None,
            expired_at,
        };

        let statement = write_queries::API_TOKEN_INSERT.prepare(client).await?;
        client
            .execute(&statement, &insert_binds(&token))
            .await
            .map_err(map_insert_error)?;

        // `APITokenSerializer(api_token).data`, 201 — the write serializer, so
        // `token` is in the body.
        render_created_token(ctx, &token, expired_at_raw.as_deref())
    })
}

/// The bind vector of [`write_queries::API_TOKEN_INSERT`]: the `USER_API_TOKENS`
/// column order (the table's model order).
fn insert_binds(token: &APIToken) -> Vec<&(dyn tokio_postgres::types::ToSql + Sync)> {
    vec![
        &token.created_at,
        &token.updated_at,
        &token.created_by_id,
        &token.updated_by_id,
        &token.deleted_at,
        &token.id,
        &token.label,
        &token.description,
        &token.is_active,
        &token.last_used,
        &token.token,
        &token.user_id,
        &token.user_type,
        &token.workspace_id,
        &token.expired_at,
    ]
}

/// `IntegrityError` (a NOT NULL or unique violation) is the family 400
/// `{"error": "The payload is not valid"}`; a `DataError` from a 256+
/// character label is SQLSTATE 22001 and stays a 500.
fn map_insert_error(error: tokio_postgres::Error) -> ViewError {
    crate::views::write_support::integrity(error)
}

// ---------------------------------------------------------------------------
// 227 PATCH/DELETE /api/users/api-tokens/{pk}/
// ---------------------------------------------------------------------------

/// `ApiTokenEndpoint.patch` (`plane/app/views/api.py:56-62`).
pub fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // 226 does not pass the kwarg the signature requires.
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(ViewError::Unknown(
                "TypeError: ApiTokenEndpoint.patch() missing 1 required positional argument: 'pk'"
                    .to_string(),
            ));
        };
        let user_id = super::support::user_id(ctx)?;
        let client = client(ctx)?;
        let pk = crate::common::lookup::uuid_kwarg("pk", pk)?;

        // `APIToken.objects.get(user=request.user, pk=pk)` (`:57`) — the
        // manager's `deleted_at IS NULL`, so a soft-deleted row is the family
        // 404. Before the body is parsed.
        let mut token = load_token(client, pk, user_id).await?;

        // `APITokenSerializer(api_token, data=request.data, partial=True)`.
        // `token`, `expired_at`, `is_active`, `user_type`, `user` and
        // `workspace` are read-only (`plane/app/serializers/api.py:15-26`), so
        // a body value for them is ignored without being validated;
        // `deleted_at`, `label`, `description`, `created_by` and `updated_by`
        // are writable.
        let mut serializer = Serializer::new(ctx, true)?;
        a_p_i_token_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        apply(&mut token, &values);
        token.updated_at = write::now();
        // H01 update: `self._state.adding` is false, so `updated_by` is the
        // caller — a body `updated_by` is overwritten here
        // (`plane/db/models/base.py:40-42`).
        token.updated_by_id = Audit::on_update(Some(user_id));
        save(client, &token).await?;

        // `Response(serializer.data)`: the same 15-key serializer as the
        // create, so `token` is visible again. Every value here comes from the
        // row `API_TOKEN_BY_PK` loaded — including `expired_at`, which is a
        // `datetime` and renders normally.
        render_token(ctx, &token)
    })
}

/// `ApiTokenEndpoint.delete` (`plane/app/views/api.py:51-54`).
pub fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // 226 does not pass the kwarg the signature requires.
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(ViewError::Unknown(
                "TypeError: ApiTokenEndpoint.delete() missing 1 required positional argument: 'pk'"
                    .to_string(),
            ));
        };
        let user_id = super::support::user_id(ctx)?;
        let client = client(ctx)?;
        let pk = crate::common::lookup::uuid_kwarg("pk", pk)?;

        // `APIToken.objects.get(user=request.user, pk=pk)` (`:52`); the body is
        // never parsed.
        let token = load_token(client, pk, user_id).await?;

        // `api_token.delete()` → H03 (`plane/db/mixins.py:72-78`): `deleted_at`
        // and `updated_at` move, `updated_by` is the caller, then
        // `soft_delete_related_objects("db", "apitoken", pk, using=None)`.
        let now = write::now();
        write::soft_delete(client, "api_tokens", "id", token.id, Some(user_id), now).await?;
        write::publish_soft_delete_cascade(ctx, "db", "apitoken", token.id).await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `APIToken.objects.get(user=request.user, pk=pk)` (`api.py:47,52,57`).
async fn load_token(
    client: &deadpool_postgres::Client,
    pk: Uuid,
    user_id: Uuid,
) -> Result<APIToken, ViewError> {
    let statement = write_queries::API_TOKEN_BY_PK.prepare(client).await?;
    let Some(row) = client.query_opt(&statement, &[&pk, &user_id]).await? else {
        return Err(ViewError::ObjectDoesNotExist);
    };
    Ok(APIToken::from_row(&row, 0)?)
}

/// `ModelSerializer.update`'s assignments for `APITokenSerializer`, in its
/// field order.
fn apply(token: &mut APIToken, values: &crate::write::Validated) {
    if values.is_null("deleted_at") {
        token.deleted_at = None;
    } else if let Some(value) = values.datetime("deleted_at") {
        token.deleted_at = Some(value);
    }
    if let Some(value) = values.text("label") {
        token.label = value.to_owned();
    }
    if let Some(value) = values.text("description") {
        token.description = value.to_owned();
    }
    // `created_by`/`updated_by` are writable FKs (`allow_null=True`).
    // `BaseModel.save` only sets `created_by` on a create, so the body value
    // survives an update; `updated_by` is overwritten by H01 just after.
    if values.is_null("created_by") {
        token.created_by_id = None;
    } else if let Some(value) = values.uuid("created_by") {
        token.created_by_id = Some(value);
    }
    if values.is_null("updated_by") {
        token.updated_by_id = None;
    } else if let Some(value) = values.uuid("updated_by") {
        token.updated_by_id = Some(value);
    }
}

/// `instance.save()` with no `update_fields`
/// (`django/db/models/base.py:928-936`): one `UPDATE` of every column keyed on
/// the primary key. A unique violation reaches the family 400.
async fn save(client: &deadpool_postgres::Client, token: &APIToken) -> Result<(), ViewError> {
    let statement = write_queries::API_TOKEN_UPDATE.prepare(client).await?;
    client
        .execute(
            &statement,
            &[
                &token.created_at,
                &token.updated_at,
                &token.created_by_id,
                &token.updated_by_id,
                &token.deleted_at,
                &token.label,
                &token.description,
                &token.is_active,
                &token.last_used,
                &token.token,
                &token.user_id,
                &token.user_type,
                &token.workspace_id,
                &token.expired_at,
                &token.id,
            ],
        )
        .await
        .map(|_| ())
        .map_err(crate::views::write_support::integrity)
}

/// `APITokenSerializer(instance).data`, 15 keys **with** `token`, 200.
fn render_token(ctx: &ViewCtx<'_>, token: &APIToken) -> Result<ViewResponse, ViewError> {
    let serializer = APITokenSerializer {
        id: token.id,
        created_at: token.created_at,
        updated_at: token.updated_at,
        deleted_at: token.deleted_at,
        label: &token.label,
        description: &token.description,
        is_active: token.is_active,
        last_used: token.last_used,
        token: &token.token,
        user_type: i64::from(token.user_type),
        expired_at: token.expired_at,
        created_by: token.created_by_id,
        updated_by: token.updated_by_id,
        user: token.user_id,
        workspace: token.workspace_id,
    };
    let body = crate::common::render(ctx, &serializer)?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

/// `APITokenSerializer(api_token).data` for the **create**, 201.
///
/// The generated table renders `expired_at` as a datetime, which is right for
/// every value loaded from the database but wrong for 226: the view passes the
/// raw `request.data` value to `APIToken.objects.create`, so the instance's
/// attribute is still the body's `str` — and
/// `DateTimeField.to_representation` returns a `str` unchanged
/// (`rest_framework/fields.py:1297-1315`). The 201 body therefore echoes the
/// request's bytes while the row holds the parsed instant.
fn render_created_token(
    ctx: &ViewCtx<'_>,
    token: &APIToken,
    expired_at: Option<&str>,
) -> Result<ViewResponse, ViewError> {
    use plane_serializers::fields::{DrfUuid, drf_datetime, drf_datetime_opt};

    let prepared = crate::common::RequestRender::from_ctx(ctx);
    let render_ctx = prepared.render_ctx();
    let tz = render_ctx.tz();
    let mut writer = plane_serializers::DrfWriter::with_capacity(704);
    {
        let mut object = writer.begin_object();
        object.field_dyn("id", &render_ctx, &DrfUuid(token.id))?;
        object.field_dyn(
            "created_at",
            &render_ctx,
            &drf_datetime(token.created_at, tz),
        )?;
        object.field_dyn(
            "updated_at",
            &render_ctx,
            &drf_datetime(token.updated_at, tz),
        )?;
        object.field_dyn(
            "deleted_at",
            &render_ctx,
            &drf_datetime_opt(token.deleted_at, tz),
        )?;
        object.field_dyn("label", &render_ctx, &token.label)?;
        object.field_dyn("description", &render_ctx, &token.description)?;
        object.field_dyn("is_active", &render_ctx, &token.is_active)?;
        object.field_dyn(
            "last_used",
            &render_ctx,
            &drf_datetime_opt(token.last_used, tz),
        )?;
        object.field_dyn("token", &render_ctx, &token.token)?;
        object.field_dyn("user_type", &render_ctx, &token.user_type)?;
        match expired_at {
            Some(text) => object.field_dyn("expired_at", &render_ctx, text)?,
            None => object.field_dyn(
                "expired_at",
                &render_ctx,
                &drf_datetime_opt(token.expired_at, tz),
            )?,
        }
        object.field_dyn("created_by", &render_ctx, &token.created_by_id.map(DrfUuid))?;
        object.field_dyn("updated_by", &render_ctx, &token.updated_by_id.map(DrfUuid))?;
        object.field_dyn("user", &render_ctx, &DrfUuid(token.user_id))?;
        object.field_dyn("workspace", &render_ctx, &token.workspace_id.map(DrfUuid))?;
        object.end();
    }
    Ok(ViewResponse::raw(201, APPLICATION_JSON, writer.into_vec()))
}

// ---------------------------------------------------------------------------
// Django model-field coercions for the raw 226 body
// ---------------------------------------------------------------------------

/// `CharField.get_prep_value`/`TextField.get_prep_value`
/// (`django/db/models/fields/__init__.py:1186-1190, 1338-1342`):
/// `str(value)`, so a number becomes its decimal digits, a bool `True`/`False`,
/// and a list/dict Python's `repr` (`"{'a': 1}"` for a dict).
///
/// `None` is SQL `NULL` on a `NOT NULL` column — the caller turns it into the
/// family `IntegrityError`. A lone surrogate cannot be encoded to UTF-8, which
/// psycopg reports as a `UnicodeEncodeError` (family 500).
fn django_text(value: Body<'_>) -> Result<String, ViewError> {
    match value {
        Body::Missing => Ok(String::new()),
        Body::Form(text) => Ok(text.to_owned()),
        Body::Json(Json::Null) => Err(ViewError::Integrity),
        Body::Json(Json::Str(text)) => text.as_str().map(str::to_owned).ok_or_else(|| {
            ViewError::Unknown(
                "UnicodeEncodeError: 'utf-8' codec can't encode a lone surrogate".to_string(),
            )
        }),
        Body::Json(other) => Ok(python_repr(other)),
    }
}

/// The `expired_at` value of 226's body: the parsed instant for the INSERT and
/// the raw string for the response (`None` for an absent key or `null`).
fn expired_at_field(value: Body<'_>) -> Result<(Option<DateTime<Utc>>, Option<String>), ViewError> {
    match value {
        Body::Missing | Body::Json(Json::Null) => Ok((None, None)),
        Body::Form(text) => Ok((django_datetime(Body::Form(text))?, Some(text.to_owned()))),
        Body::Json(Json::Str(text)) => {
            let text = text.as_str().map(str::to_owned).ok_or_else(|| {
                ViewError::Unknown(
                    "UnicodeEncodeError: 'utf-8' codec can't encode a lone surrogate".to_string(),
                )
            })?;
            let instant = django_datetime(Body::Form(&text))?;
            Ok((instant, Some(text)))
        }
        other => django_datetime(other).map(|instant| (instant, None)),
    }
}

/// `DateTimeField.get_prep_value`
/// (`django/db/models/fields/__init__.py:1655-1690`) for `expired_at`:
///
/// * `None` → SQL `NULL`;
/// * a string → `parse_datetime(value)`, then `parse_date(value)` at midnight,
///   else a `ValidationError`; a **naive** result is localised in
///   `settings.TIME_ZONE` (UTC), not in the active user timezone;
/// * anything else → `datetime.fromisoformat` raises `TypeError`, which Django
///   does not catch: the family 500.
fn django_datetime(value: Body<'_>) -> Result<Option<DateTime<Utc>>, ViewError> {
    let text = match value {
        Body::Missing | Body::Json(Json::Null) => return Ok(None),
        Body::Form(text) => text,
        Body::Json(Json::Str(text)) => text.as_str().ok_or_else(|| {
            ViewError::Unknown(
                "UnicodeEncodeError: 'utf-8' codec can't encode a lone surrogate".to_string(),
            )
        })?,
        Body::Json(_) => {
            return Err(ViewError::Unknown(
                "TypeError: fromisoformat: argument must be str".to_string(),
            ));
        }
    };
    if let Some(parsed) = crate::write::pyiso::parse_datetime(text) {
        return match parsed.offset_us {
            Some(offset) => parsed
                .naive
                .checked_sub_signed(chrono::TimeDelta::microseconds(offset))
                .map(|utc| Some(utc.and_utc()))
                .ok_or_else(|| ViewError::Unknown("OverflowError: date value out of range".into())),
            // `timezone.make_aware(value, timezone.get_default_timezone())`
            // with `TIME_ZONE = "UTC"` (`plane/settings/common.py:319`).
            None => crate::write::pyiso::localize(parsed.naive, chrono_tz::UTC)
                .map(|utc| Some(utc.and_utc()))
                .ok_or_else(|| ViewError::Unknown("OverflowError: date value out of range".into())),
        };
    }
    // `parse_date(value)` → `datetime.datetime(y, m, d)`, naive midnight.
    if let Some(date) = crate::write::pyiso::parse_date(text) {
        let midnight = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| ViewError::Unknown("OverflowError: date value out of range".into()))?;
        return Ok(Some(midnight.and_utc()));
    }
    Err(ViewError::Validation)
}
