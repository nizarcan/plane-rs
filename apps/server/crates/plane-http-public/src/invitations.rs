// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Workspace invitations on `/api/v1/` (routes 340–343).
//!
//! Python: `plane/api/views/invite.py:24-154`. `WorkspaceInvitationsViewset` is
//! a `BaseViewSet` (`api.BaseViewSet`), so it keeps `AnonRateThrottle` and
//! emits no `X-RateLimit-*` headers, and its 404 says "required object"
//! (`plane/api/views/base.py:183-243`).
//!
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | `ws/invitations/` | `GET`/`POST` | `list`/`create` (`:55-94`) |
//! | `ws/invitations/{pk}/` | `GET`/`PUT`/`PATCH`/`DELETE` | `retrieve`/`update`/`partial_update`/`destroy` (`:75-154`) |
//! | `ws/invitations.{format}`, `ws/invitations/{pk}.{format}` | all bound verbs | — |
//!
//! ## The format-suffix routes are 500s
//!
//! The router's `format_suffix_patterns` pass `format=…` to the view method,
//! and no method of the viewset accepts it, so every format route raises
//! `TypeError: list() got an unexpected keyword argument 'format'` **after**
//! `initial()` (`invite.py:55,75,89,112,141`): authentication, permissions and
//! throttles have already run, and the family handler turns it into its 500.
//! [`format_error`] reproduces that, and it is why the invalid-key and
//! non-owner cases on these routes answer 403/403 rather than 500.
//!
//! ## Behaviours ported as they are
//!
//! * `POST` **succeeds** (201): the serializer has no `token` field and
//!   `create` never sets one (`invite.py:89-94`), but Django's
//!   `Field.get_default()` returns `""` for a `CharField` without a default, so
//!   the row is written with an empty token. `plans/V1_API.md:302-305` expected
//!   a NOT NULL 400; the live oracle creates the invite, and the empty token is
//!   visible in the response only through the DB.
//! * `partial_update` refuses any truthy `email` (`invite.py:114-118`), while
//!   the inherited `PUT` (which only `partial_update` overrides, `:112`) builds
//!   its serializer through `get_serializer`, whose context has no `slug`.
//!   `WorkspaceInviteSerializer.validate` reads `self.context["slug"]`
//!   (`serializers/invite.py:49`), so every `PUT` whose field validation passes
//!   raises `KeyError` → 400 `{"error": "The required key does not exist."}`.
//!   Field errors (email/role) are still reported first.

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::RenderCtx;
use plane_serializers::generated::api_invite::WorkspaceInviteSerializer;
use uuid::Uuid;

use crate::invitations_queries as queries;

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.api.views.invite.WorkspaceInvitationsViewset")?
        .action("list", list)?
        .action("retrieve", retrieve)?
        .action("create", create)?
        .action("update", update)?
        .action("partial_update", partial_update)?
        .action("destroy", destroy)?;
    Ok(())
}

/// `TypeError` from the missing `format` parameter, as the family 500.
fn format_error(ctx: &ViewCtx<'_>) -> Option<ViewError> {
    ctx.kwarg("format").map(|format| {
        ViewError::Unknown(format!(
            "TypeError: the view method does not accept the format kwarg ({format:?})"
        ))
    })
}

/// One `workspace_member_invites` row.
struct Invite {
    id: Uuid,
    email: String,
    role: i16,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    responded_at: Option<DateTime<Utc>>,
    accepted: bool,
    token: String,
    message: Option<String>,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    deleted_at: Option<DateTime<Utc>>,
    workspace_id: Uuid,
}

impl Invite {
    /// [`queries::INVITE_BY_ID`]'s columns.
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.get(0),
            email: row.get(1),
            role: row.get(2),
            created_at: row.get(3),
            updated_at: row.get(4),
            responded_at: row.get(5),
            accepted: row.get(6),
            token: row.get(7),
            message: row.get(8),
            created_by: row.get(9),
            updated_by: row.get(10),
            deleted_at: row.get(11),
            workspace_id: row.get(12),
        })
    }

    fn serializer(&self) -> WorkspaceInviteSerializer<'_> {
        WorkspaceInviteSerializer {
            id: self.id,
            email: &self.email,
            role: i64::from(self.role),
            created_at: self.created_at,
            updated_at: self.updated_at,
            responded_at: self.responded_at,
            accepted: self.accepted,
        }
    }
}

/// One `list` row (the seven serializer columns).
struct ListRow {
    id: Uuid,
    email: String,
    role: i16,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    responded_at: Option<DateTime<Utc>>,
    accepted: bool,
}

impl ListRow {
    fn decode(row: &tokio_postgres::Row) -> Self {
        Self {
            id: row.get(0),
            email: row.get(1),
            role: row.get(2),
            created_at: row.get(3),
            updated_at: row.get(4),
            responded_at: row.get(5),
            accepted: row.get(6),
        }
    }

    fn serializer(&self) -> WorkspaceInviteSerializer<'_> {
        WorkspaceInviteSerializer {
            id: self.id,
            email: &self.email,
            role: i64::from(self.role),
            created_at: self.created_at,
            updated_at: self.updated_at,
            responded_at: self.responded_at,
            accepted: self.accepted,
        }
    }
}

async fn get_invite(client: &Client, slug: &str, pk: Uuid) -> Result<Option<Invite>, ViewError> {
    let row = client
        .query_opt(queries::INVITE_BY_ID.sql, &[&slug, &pk])
        .await
        .map_err(ViewError::from)?;
    row.as_ref().map(Invite::decode).transpose()
}

/// `WorkspaceInvitationsViewset.list` (`plane/api/views/invite.py:55-58`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(error) = format_error(ctx) {
            return Err(error);
        }
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let client = plane_http_app::common::client(ctx)?;

        let rows = client
            .query(queries::INVITES.sql, &[&slug])
            .await
            .map_err(ViewError::from)?;
        // Owned so the serializers below can borrow them.
        let invites = rows.iter().map(ListRow::decode).collect::<Vec<_>>();
        let serializers: Vec<WorkspaceInviteSerializer<'_>> =
            invites.iter().map(ListRow::serializer).collect();

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_http_app::common::render_list(&serializers, &RenderCtx::full(&timezone))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkspaceInvitationsViewset.retrieve` (`plane/api/views/invite.py:75-78`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(error) = format_error(ctx) {
            return Err(error);
        }
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        // `pk` is the regex route's `[^/.]+`, so a non-UUID is Django's
        // `ValidationError` → the family 400.
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let client = plane_http_app::common::client(ctx)?;

        let Some(invite) = get_invite(client, slug, pk).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&invite.serializer(), &RenderCtx::full(&timezone))
            .map_err(ViewError::from)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `WorkspaceInvitationsViewset.create` (`plane/api/views/invite.py:89-94`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(error) = format_error(ctx) {
            return Err(error);
        }
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let actor = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`invite.py:90`).
        let workspace = plane_http_app::common::lookup::workspace_by_slug(client, slug).await?;

        let mut serializer = plane_http_app::write::Serializer::new(ctx, false)?;
        plane_http_app::write::generated::workspace_invite_serializer::validate(
            &mut serializer,
            client,
        )
        .await?;
        if let Some(plane_http_app::write::Value::Text(email)) = serializer.value("email") {
            // `validate_email` (`serializers/invite.py:41-46`).
            if !plane_http_app::write::email::validate_email(email) {
                serializer.push_error(
                    "email",
                    plane_http_app::write::Detail::new(
                        "Invalid email address",
                        "INVALID_EMAIL_ADDRESS",
                    ),
                );
            } else if invite_email_exists(client, slug, email).await? {
                // `validate` (`serializers/invite.py:48-59`).
                serializer.push_error(
                    "non_field_errors",
                    plane_http_app::write::Detail::new(
                        "Email already invited",
                        "EMAIL_ALREADY_INVITED",
                    ),
                );
            }
        }
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };
        let email = values
            .text("email")
            .ok_or_else(|| ViewError::Unknown("KeyError: 'email'".to_string()))?
            .to_owned();
        let role = values
            .integer("role")
            .map(|role| i16::try_from(role).map_err(|_| ViewError::Unknown("role".to_string())))
            .transpose()?
            .unwrap_or(5);

        // `serializer.save(workspace=…, created_by=request.user)`: Django's
        // `Field.get_default()` fills the missing `token` with `""`, so the row
        // is created (see `INVITE_INSERT`).
        let now = plane_http_app::write::now();
        let invite = Invite {
            id: Uuid::new_v4(),
            email,
            role,
            created_at: now,
            updated_at: now,
            responded_at: None,
            accepted: false,
            token: String::new(),
            message: None,
            created_by: Some(actor),
            updated_by: None,
            deleted_at: None,
            workspace_id: workspace.id,
        };
        client
            .execute(
                queries::INVITE_INSERT.sql,
                &[
                    &invite.created_at,
                    &invite.updated_at,
                    &invite.created_by,
                    &invite.updated_by,
                    &invite.deleted_at,
                    &invite.id,
                    &invite.workspace_id,
                    &invite.email,
                    &invite.accepted,
                    &invite.token,
                    &invite.message,
                    &invite.responded_at,
                    &invite.role,
                ],
            )
            .await
            .map_err(db_error)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&invite.serializer(), &RenderCtx::full(&timezone))
            .map_err(ViewError::from)?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

/// `WorkspaceInvitationsViewset.partial_update` (`plane/api/views/invite.py:112-124`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(error) = format_error(ctx) {
            return Err(error);
        }
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let actor = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        let mut invite = match get_invite(client, slug, pk).await? {
            Some(invite) => invite,
            None => return Err(ViewError::ObjectDoesNotExist),
        };

        // `if request.data.get("email"):` — any truthy email, even its own.
        let body_data = ctx.data()?;
        // `request.data.get("email")` on a JSON object is the JSON value; on a
        // form/multipart body it is the (possibly empty) string.
        let email_present = match body_data {
            plane_drf::parsers::Data::Form(_) => body_data
                .get("email")
                .is_some_and(|value| !value.is_empty()),
            plane_drf::parsers::Data::Json(_) => body_data
                .json()
                .and_then(|value| value.get("email"))
                .is_some_and(truthy_json),
        };
        if email_present {
            let body = plane_query::Json::object(vec![
                (
                    "error",
                    plane_query::Json::string("Email cannot be updated after invite is created."),
                ),
                ("code", plane_query::Json::string("EMAIL_CANNOT_BE_UPDATED")),
            ]);
            return Ok(ViewResponse::raw(400, APPLICATION_JSON, body.to_drf()));
        }

        let mut serializer = plane_http_app::write::Serializer::new(ctx, true)?;
        plane_http_app::write::generated::workspace_invite_serializer::validate(
            &mut serializer,
            client,
        )
        .await?;
        validate_invite(&mut serializer, client, slug).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };
        apply_values(&mut invite, &values)?;
        save_invite(ctx, client, &mut invite, actor).await
    })
}

/// `WorkspaceInvitationsViewset.update` — DRF's `UpdateModelMixin.update`
/// (`invite.py:96-110` registers `put`, the method itself is inherited).
pub fn update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(error) = format_error(ctx) {
            return Err(error);
        }
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let client = plane_http_app::common::client(ctx)?;

        if get_invite(client, slug, pk).await?.is_none() {
            // `get_object()` runs before the serializer.
            return Err(ViewError::ObjectDoesNotExist);
        }

        // The inherited `update` builds the serializer through
        // `get_serializer`, whose context is `{request, format, view}` — it has
        // no `slug`. `WorkspaceInviteSerializer.validate` reads
        // `self.context["slug"]` (`api/serializers/invite.py:49`), so any PUT
        // whose field validation passes raises `KeyError`, and the family
        // handler answers 400 `{"error": "The required key does not exist."}`.
        // Field errors (email/role) still win, because they are collected
        // before `validate()` runs.
        let mut serializer = plane_http_app::write::Serializer::new(ctx, false)?;
        plane_http_app::write::generated::workspace_invite_serializer::validate(
            &mut serializer,
            client,
        )
        .await?;
        if let Err(errors) = serializer.finish() {
            return Ok(errors.into_response());
        }
        Err(ViewError::Key)
    })
}

/// `WorkspaceInviteSerializer`'s `validate_email` and `validate`
/// (`plane/api/serializers/invite.py:41-59`) — shared by `PUT` and `PATCH`.
async fn validate_invite(
    serializer: &mut plane_http_app::write::Serializer<'_>,
    client: &Client,
    slug: &str,
) -> Result<(), ViewError> {
    if let Some(plane_http_app::write::Value::Text(email)) = serializer.value("email") {
        if !plane_http_app::write::email::validate_email(email) {
            serializer.push_error(
                "email",
                plane_http_app::write::Detail::new(
                    "Invalid email address",
                    "INVALID_EMAIL_ADDRESS",
                ),
            );
        } else if invite_email_exists(client, slug, email).await? {
            serializer.push_error(
                "non_field_errors",
                plane_http_app::write::Detail::new(
                    "Email already invited",
                    "EMAIL_ALREADY_INVITED",
                ),
            );
        }
    }
    Ok(())
}

/// `ModelSerializer.update`'s assignment step for the two writable fields.
fn apply_values(
    invite: &mut Invite,
    values: &plane_http_app::write::Validated,
) -> Result<(), ViewError> {
    if let Some(email) = values.text("email") {
        invite.email = email.to_owned();
    }
    if let Some(role) = values.integer("role") {
        invite.role = i16::try_from(role)
            .map_err(|_| ViewError::Unknown("smallint out of range".to_string()))?;
    }
    Ok(())
}

/// `serializer.save()` → a full-row `UPDATE` plus the family's response.
async fn save_invite(
    ctx: &ViewCtx<'_>,
    client: &Client,
    invite: &mut Invite,
    actor: Uuid,
) -> Result<ViewResponse, ViewError> {
    invite.updated_at = plane_http_app::write::now();
    invite.updated_by = Some(actor);
    client
        .execute(
            queries::INVITE_UPDATE.sql,
            &[
                &invite.created_at,
                &invite.updated_at,
                &invite.created_by,
                &invite.updated_by,
                &invite.deleted_at,
                &invite.id,
                &invite.workspace_id,
                &invite.email,
                &invite.accepted,
                &invite.token,
                &invite.message,
                &invite.responded_at,
                &invite.role,
            ],
        )
        .await
        .map_err(db_error)?;
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let body = plane_serializers::render(&invite.serializer(), &RenderCtx::full(&timezone))
        .map_err(ViewError::from)?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

/// `WorkspaceInvitationsViewset.destroy` (`plane/api/views/invite.py:141-154`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(error) = format_error(ctx) {
            return Err(error);
        }
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let actor = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        let Some(invite) = get_invite(client, slug, pk).await? else {
            return Err(ViewError::ObjectDoesNotExist);
        };
        if invite.accepted {
            let body = plane_query::Json::object(vec![
                (
                    "error",
                    plane_query::Json::string("Invite already accepted"),
                ),
                ("code", plane_query::Json::string("INVITE_ALREADY_ACCEPTED")),
            ]);
            return Ok(ViewResponse::raw(400, APPLICATION_JSON, body.to_drf()));
        }
        if invite.responded_at.is_some() {
            let body = plane_query::Json::object(vec![
                (
                    "error",
                    plane_query::Json::string("Invite already responded"),
                ),
                (
                    "code",
                    plane_query::Json::string("INVITE_ALREADY_RESPONDED"),
                ),
            ]);
            return Ok(ViewResponse::raw(400, APPLICATION_JSON, body.to_drf()));
        }

        // `instance.delete()` → `SoftDeleteModel.delete` (`mixins.py:65-77`):
        // `deleted_at = now`, a full `save()`, then the cascade publish with
        // `_meta.model_name`.
        let now = plane_http_app::write::now();
        plane_http_app::write::soft_delete(
            client,
            "workspace_member_invites",
            "id",
            invite.id,
            plane_http_app::write::Audit::on_update(Some(actor)),
            now,
        )
        .await?;
        plane_http_app::write::publish_soft_delete_cascade(
            ctx,
            "db",
            "workspacememberinvite",
            invite.id,
        )
        .await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `WorkspaceMemberInvite.objects.filter(email=…, workspace__slug=slug).exists()`.
async fn invite_email_exists(client: &Client, slug: &str, email: &str) -> Result<bool, ViewError> {
    Ok(client
        .query_one(queries::INVITE_EMAIL_EXISTS.sql, &[&slug, &email])
        .await
        .map_err(ViewError::from)?
        .get(0))
}

/// Django's `IntegrityError` family is the family 400 (`plane/api/views/
/// base.py:192-197`); every other database error is the 500.
fn db_error(error: tokio_postgres::Error) -> ViewError {
    match error.as_db_error() {
        Some(db) if db.code().code().starts_with("23") => ViewError::Integrity,
        _ => ViewError::from(error),
    }
}

/// Python truthiness of a parsed JSON value (`request.data.get("email")`).
fn truthy_json(value: &plane_serde::pyjson::Value) -> bool {
    use plane_serde::pyjson::Value;
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Int(value) => value.as_i64().is_some_and(|value| value != 0),
        Value::Float(value) => *value != 0.0,
        Value::Str(value) => value.as_str().is_some_and(|value| !value.is_empty()),
        Value::List(items) => !items.is_empty(),
        Value::Dict(object) => !object.is_empty(),
    }
}
