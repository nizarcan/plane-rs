// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/api/views/admin.py`: the instance-admin list, detail,
//! `me` and `session` routes (260-262 and 264's three methods).
//!
//! | Route | View | Python |
//! | --- | --- | --- |
//! | `GET/POST/DELETE /api/instances/admins/` | `InstanceAdminEndpoint` | `admin.py:39-76` |
//! | `GET /api/instances/admins/me/` | `InstanceAdminUserMeEndpoint` | `admin.py:357-361` |
//! | `GET /api/instances/admins/session/` | `InstanceAdminUserSessionEndpoint` | `admin.py:364-375` |
//! | `GET/POST/DELETE /api/instances/admins/{pk}/` | `InstanceAdminEndpoint` | `admin.py:39-76` |
//!
//! ## Bugs ported as-is
//!
//! * **260 DELETE** and **264 GET/POST** raise `TypeError`: the collection
//!   route passes no `pk` to `delete(self, request, pk)` and the detail route
//!   passes one to `get(self, request)`/`post(self, request)`
//!   (`admin.py:51,73`, `license/urls.py:37`). `handle_exception` does not map
//!   `TypeError`, so the family answers its 500 body
//!   (`license/api/views/base.py:62-88`).
//! * `POST /admins/` looks the user up with `User.objects.get(email=email)`
//!   and never catches `DoesNotExist`: the family's 404 body
//!   (`admin.py:53`). `role` defaults to `20` but is not validated against the
//!   choices, and a duplicate `(instance, user)` pair is the unique constraint's
//!   `IntegrityError` → 400 `{"error": "The payload is not valid"}`
//!   (`plane/db/models/base.py`, `license/models/instance.py:96`).
//! * `DELETE` is `InstanceAdmin.objects.filter(…).delete()`, i.e.
//!   `QuerySet.delete` = `update(deleted_at=now)` (`plane/db/mixins.py:48-53`).
//!   It does not 404 on a missing row and writes no other column.
//! * The list's `@cache_response(60 * 60 * 2, user=False)` keys on
//!   `/api/instances/admins/`, and `POST`/`DELETE` invalidate
//!   `/api/instances/` instead — the list can stay stale for two hours
//!   (`admin.py:48,55,71`).
//! * `GET`'s `user_detail` is a nested `UserAdminLiteSerializer` per row with
//!   no `select_related`, so Django issues one query per admin plus one per
//!   avatar asset. [`USERS_ADMIN_LITE_BY_IDS`] folds both into one.

use chrono::Utc;
use plane_cache::PyValue;
use plane_core::error::APPLICATION_JSON;
use plane_core::json::Object;
use plane_db::models::license::instance::InstanceAdmin;
use plane_db::models::user::User;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::Dialect;
use plane_serializers::generated::app_user::UserAdminLiteSerializer;
use plane_serializers::generated::license_api_admin::{
    InstanceAdminMeSerializer, InstanceAdminSerializer,
};
use uuid::Uuid;

use super::instance::{error_body, instance_first};
use super::queries;
use crate::common::cache;
use crate::common::client;
use crate::write;

/// Register the routes this module owns.
///
/// Both `/admins/` and `/admins/{pk}/` resolve to the same view class, and the
/// registry maps one handler per HTTP method, so the three handlers branch on
/// `pk` exactly where Python's signatures fail (`admin.py:51,71,73`).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.license.api.views.admin.InstanceAdminEndpoint")?
        .method(http::Method::GET, get)?
        .method(http::Method::POST, create)?
        .method(http::Method::DELETE, delete)?;
    registry
        .api_view("plane.license.api.views.admin.InstanceAdminUserMeEndpoint")?
        .method(http::Method::GET, me)?;
    registry
        .api_view("plane.license.api.views.admin.InstanceAdminUserSessionEndpoint")?
        .method(http::Method::GET, session)?;
    Ok(())
}

/// `InstanceAdminEndpoint.get(self, request)` (`admin.py:68-76`) — and the
/// detail route's `TypeError` (`admin.py:73`).
fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("pk").is_some() {
            return Err(ViewError::Unknown(
                "TypeError: InstanceAdminEndpoint.get() got an unexpected keyword argument 'pk'"
                    .to_string(),
            ));
        }
        list_cached(ctx).await
    })
}

/// `GET /api/instances/admins/` (`admin.py:54-66`), with
/// `@cache_response(60 * 60 * 2, user=False)`.
///
/// The cached `serializer.data` is a `ReturnList` of plain dicts (a nested
/// serializer's `to_representation` returns `dict`), which is why Django's
/// pickled value stays decodable; the Build side mirrors that shape.
async fn list_cached(ctx: &ViewCtx<'_>) -> Result<ViewResponse, ViewError> {
    cache::cache_response(ctx, 60 * 60 * 2, None, false, || async move {
        let client = client(ctx)?;
        let Some(instance) = instance_first(client).await? else {
            return Ok((403, dict_error("Instance is not registered yet")));
        };
        let statement = client.prepare_cached(queries::INSTANCE_ADMINS.sql).await?;
        let rows = client.query(&statement, &[&instance.id]).await?;
        let admins = rows
            .iter()
            .map(|row| InstanceAdmin::from_row(row, 0))
            .collect::<Result<Vec<_>, _>>()?;

        let user_ids: Vec<Uuid> = admins.iter().filter_map(|admin| admin.user_id).collect();
        let users = load_admin_users(client, &user_ids).await?;
        let entries = admins
            .iter()
            .map(|admin| {
                let user = admin.user_id.and_then(|user_id| users.get(&user_id));
                admin_value(admin, user)
            })
            .collect();
        Ok((200, PyValue::return_list(entries)))
    })
    .await
}

/// One element of the cached list: `InstanceAdminSerializer(admin).data`.
fn admin_value(admin: &InstanceAdmin, user: Option<&AdminUser>) -> PyValue {
    PyValue::dict(vec![
        ("id", PyValue::uuid(admin.id)),
        (
            "user_detail",
            user.map_or(PyValue::None, |user| PyValue::dict(user.value_fields())),
        ),
        ("created_at", PyValue::string(drf_string(admin.created_at))),
        ("updated_at", PyValue::string(drf_string(admin.updated_at))),
        (
            "deleted_at",
            admin
                .deleted_at
                .map_or(PyValue::None, |value| PyValue::string(drf_string(value))),
        ),
        ("role", PyValue::int(i64::from(admin.role))),
        ("is_verified", PyValue::bool(admin.is_verified)),
        (
            "created_by",
            admin.created_by_id.map_or(PyValue::None, PyValue::uuid),
        ),
        (
            "updated_by",
            admin.updated_by_id.map_or(PyValue::None, PyValue::uuid),
        ),
        ("user", admin.user_id.map_or(PyValue::None, PyValue::uuid)),
        ("instance", PyValue::uuid(admin.instance_id)),
    ])
}

fn dict_error(message: &str) -> PyValue {
    PyValue::dict(vec![("error", PyValue::string(message))])
}

/// A `users` row joined with its avatar asset's entity type.
#[derive(Debug)]
pub(crate) struct AdminUser {
    pub user: User,
    /// `User.avatar_url` resolved once (`plane/db/models/user.py:143-151`).
    pub avatar_url: Option<String>,
}

impl AdminUser {
    /// `UserAdminLiteSerializer`'s nine fields, in order.
    fn value_fields(&self) -> Vec<(&'static str, PyValue)> {
        vec![
            ("id", PyValue::uuid(self.user.id)),
            ("first_name", PyValue::string(&self.user.first_name)),
            ("last_name", PyValue::string(&self.user.last_name)),
            ("avatar", PyValue::string(&self.user.avatar)),
            (
                "avatar_url",
                self.avatar_url
                    .clone()
                    .map_or(PyValue::None, PyValue::string),
            ),
            ("is_bot", PyValue::bool(self.user.is_bot)),
            ("display_name", PyValue::string(&self.user.display_name)),
            (
                "email",
                self.user
                    .email
                    .clone()
                    .map_or(PyValue::None, PyValue::string),
            ),
            (
                "last_login_medium",
                PyValue::string(&self.user.last_login_medium),
            ),
        ]
    }

    /// The same nine fields as the generated serializer.
    pub(crate) fn user_admin_lite(&self) -> UserAdminLiteSerializer<'_> {
        UserAdminLiteSerializer {
            id: self.user.id,
            first_name: &self.user.first_name,
            last_name: &self.user.last_name,
            avatar: &self.user.avatar,
            avatar_url: self.avatar_url.as_deref(),
            is_bot: self.user.is_bot,
            display_name: &self.user.display_name,
            email: self.user.email.as_deref(),
            last_login_medium: &self.user.last_login_medium,
        }
    }

    /// `UserLiteSerializer` (`plane/license/api/serializers/user.py:7-13`).
    pub(crate) fn user_lite(
        &self,
    ) -> plane_serializers::generated::license_api_user::UserLiteSerializer<'_> {
        plane_serializers::generated::license_api_user::UserLiteSerializer {
            id: self.user.id,
            email: self.user.email.as_deref(),
            first_name: &self.user.first_name,
            last_name: &self.user.last_name,
        }
    }

    /// `InstanceAdminMeSerializer`'s fifteen fields (`SERIALIZERS.md`), from
    /// the same joined row.
    pub(crate) fn instance_admin_me(&self) -> InstanceAdminMeSerializer<'_> {
        InstanceAdminMeSerializer {
            id: self.user.id,
            avatar: &self.user.avatar,
            avatar_url: self.avatar_url.as_deref(),
            cover_image: self.user.cover_image.as_deref(),
            date_joined: self.user.date_joined,
            display_name: &self.user.display_name,
            email: self.user.email.as_deref(),
            first_name: &self.user.first_name,
            last_name: &self.user.last_name,
            is_active: self.user.is_active,
            is_bot: self.user.is_bot,
            is_email_verified: self.user.is_email_verified,
            user_timezone: &self.user.user_timezone,
            username: &self.user.username,
            is_password_autoset: self.user.is_password_autoset,
        }
    }
}

/// `InstanceAdminSerializer`'s `user_detail` values for `users`.
pub(crate) async fn load_admin_users(
    client: &deadpool_postgres::Client,
    user_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, AdminUser>, ViewError> {
    if user_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let statement = client
        .prepare_cached(queries::USERS_ADMIN_LITE_BY_IDS.sql)
        .await?;
    let rows = client.query(&statement, &[&user_ids]).await?;
    let mut users = std::collections::HashMap::with_capacity(rows.len());
    for row in &rows {
        let id: Uuid = row.try_get(0)?;
        let user = User {
            id,
            first_name: row.try_get(1)?,
            last_name: row.try_get(2)?,
            avatar: row.try_get(3)?,
            is_bot: row.try_get(4)?,
            display_name: row.try_get(5)?,
            email: row.try_get(6)?,
            last_login_medium: row.try_get(7)?,
            avatar_asset_id: row.try_get(8)?,
            ..blank_user()
        };
        let avatar_entity_type: Option<String> = row.try_get(9)?;
        let avatar_url = crate::workspaces::rows::user_avatar_url(
            user.avatar_asset_id,
            avatar_entity_type.as_deref(),
            &user.avatar,
        );
        users.insert(id, AdminUser { avatar_url, user });
    }
    Ok(users)
}

/// A `User` whose unselected columns are never read by these serializers.
fn blank_user() -> User {
    let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap_or_default();
    User {
        password: String::new(),
        last_login: None,
        id: Uuid::nil(),
        username: String::new(),
        mobile_number: None,
        email: None,
        display_name: String::new(),
        first_name: String::new(),
        last_name: String::new(),
        avatar: String::new(),
        avatar_asset_id: None,
        cover_image: None,
        cover_image_asset_id: None,
        date_joined: epoch,
        created_at: epoch,
        updated_at: epoch,
        last_location: String::new(),
        created_location: String::new(),
        is_superuser: false,
        is_managed: false,
        is_password_expired: false,
        is_active: false,
        is_staff: false,
        is_email_verified: false,
        is_password_autoset: false,
        is_password_reset_required: false,
        token: String::new(),
        last_active: None,
        last_login_time: None,
        last_logout_time: None,
        last_login_ip: String::new(),
        last_logout_ip: String::new(),
        last_login_medium: String::new(),
        last_login_uagent: String::new(),
        token_updated_at: None,
        is_bot: false,
        bot_type: None,
        user_timezone: String::new(),
        is_email_valid: false,
        masked_at: None,
    }
}

/// `POST /api/instances/admins/` (`admin.py:41-61`) — and the detail route's
/// `TypeError` (`admin.py:51`).
fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if ctx.kwarg("pk").is_some() {
            return Err(ViewError::Unknown(
                "TypeError: InstanceAdminEndpoint.post() got an unexpected keyword argument 'pk'"
                    .to_string(),
            ));
        }
        cache::invalidate_cache(ctx, Some("/api/instances/"), false, false, false).await?;
        let client = client(ctx)?;
        let email = crate::instances::body_string(ctx, "email")?.unwrap_or_default();
        if email.is_empty() {
            return Ok(error_body(400, "Email is required"));
        }
        let Some(instance) = instance_first(client).await? else {
            return Ok(error_body(403, "Instance is not registered yet"));
        };
        let role = crate::instances::body_int(ctx, "role")?.unwrap_or(20);
        let user = load_user_by_email(client, &email).await?;
        let Some(user) = user else {
            // `User.objects.get` → `DoesNotExist` → the family's 404.
            return Err(ViewError::ObjectDoesNotExist);
        };
        let now = write::now();
        let id = Uuid::new_v4();
        let admin = InstanceAdmin {
            created_at: now,
            updated_at: now,
            created_by_id: ctx.user.id(),
            updated_by_id: None,
            deleted_at: None,
            id,
            user_id: Some(user.id),
            instance_id: instance.id,
            role: role as i32,
            is_verified: false,
        };
        let statement = client
            .prepare_cached(queries::INSTANCE_ADMIN_CREATE.sql)
            .await?;
        client
            .execute(
                &statement,
                &[
                    &admin.created_at,
                    &admin.updated_at,
                    &admin.created_by_id,
                    &admin.updated_by_id,
                    &admin.deleted_at,
                    &admin.id,
                    &admin.user_id,
                    &admin.instance_id,
                    &admin.role,
                    &admin.is_verified,
                ],
            )
            .await
            .map_err(|error| match error.code() {
                Some(code) if code.code().starts_with("23") => ViewError::Integrity,
                _ => ViewError::from(error),
            })?;
        let avatar_entity_type = load_asset_entity_type(client, user.avatar_asset_id).await?;
        let admin_user = AdminUser {
            avatar_url: crate::workspaces::rows::user_avatar_url(
                user.avatar_asset_id,
                avatar_entity_type.as_deref(),
                &user.avatar,
            ),
            user,
        };
        let body = crate::common::render(
            ctx,
            &InstanceAdminSerializer {
                id: admin.id,
                user_detail: Some(&admin_user.user_admin_lite()),
                created_at: admin.created_at,
                updated_at: admin.updated_at,
                deleted_at: admin.deleted_at,
                role: i64::from(admin.role),
                is_verified: admin.is_verified,
                created_by: admin.created_by_id,
                updated_by: admin.updated_by_id,
                user: admin.user_id,
                instance: admin.instance_id,
            },
        )?;
        Ok(ViewResponse::raw(201, APPLICATION_JSON, body))
    })
}

async fn load_user_by_email(
    client: &deadpool_postgres::Client,
    email: &str,
) -> Result<Option<User>, ViewError> {
    let statement = client
        .prepare_cached(crate::auth::queries::AUTH_USER_BY_EMAIL.sql)
        .await?;
    match client.query_opt(&statement, &[&email]).await? {
        Some(row) => Ok(Some(User::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

pub(crate) async fn load_asset_entity_type(
    client: &deadpool_postgres::Client,
    id: Option<Uuid>,
) -> Result<Option<String>, ViewError> {
    let Some(id) = id else {
        return Ok(None);
    };
    let statement = client
        .prepare_cached(queries::ASSET_ENTITY_TYPE.sql)
        .await?;
    match client.query_opt(&statement, &[&id]).await? {
        Some(row) => Ok(row.try_get(0)?),
        None => Ok(None),
    }
}

/// `InstanceAdminEndpoint.delete(self, request, pk)` (`admin.py:71-76`).
///
/// The collection route passes no `pk`, so Python raises `TypeError` before
/// the body and the family answers its 500 (`admin.py:71`).
fn delete<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let Some(pk) = ctx.kwarg("pk") else {
            return Err(ViewError::Unknown(
                "TypeError: InstanceAdminEndpoint.delete() missing 1 required positional \
                 argument: 'pk'"
                    .to_string(),
            ));
        };
        let pk = Uuid::parse_str(pk).map_err(|_| {
            ViewError::Unknown("ValueError: badly formed hexadecimal UUID string".to_string())
        })?;
        cache::invalidate_cache(ctx, Some("/api/instances/"), false, false, false).await?;
        let client = client(ctx)?;
        let instance = instance_first(client).await?;
        if let Some(instance) = instance {
            let statement = client
                .prepare_cached(queries::INSTANCE_ADMIN_SOFT_DELETE.sql)
                .await?;
            client
                .execute(&statement, &[&write::now(), &instance.id, &pk])
                .await?;
        }
        Ok(ViewResponse::empty(204))
    })
}

/// `GET /api/instances/admins/me/` (`admin.py:357-361`).
fn me<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let user = current_user(ctx).await?;
        let avatar_entity_type = load_asset_entity_type(client, user.avatar_asset_id).await?;
        let admin_user = AdminUser {
            avatar_url: crate::workspaces::rows::user_avatar_url(
                user.avatar_asset_id,
                avatar_entity_type.as_deref(),
                &user.avatar,
            ),
            user,
        };
        let body = crate::common::render(ctx, &admin_user.instance_admin_me())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `GET /api/instances/admins/session/` (`admin.py:364-375`).
fn session<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let authenticated = if let Some(user) = ctx.user.row() {
            if user.id.is_nil() {
                false
            } else {
                has_admin_row(client, user.id).await?
            }
        } else {
            false
        };
        if !authenticated {
            let body = Object::new().field("is_authenticated", false);
            let bytes = plane_serde::to_vec(Dialect::Drf, &body)?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, bytes));
        }
        let user = current_user(ctx).await?;
        let avatar_entity_type = load_asset_entity_type(client, user.avatar_asset_id).await?;
        let admin_user = AdminUser {
            avatar_url: crate::workspaces::rows::user_avatar_url(
                user.avatar_asset_id,
                avatar_entity_type.as_deref(),
                &user.avatar,
            ),
            user,
        };
        // `{"is_authenticated": True, "user": serializer.data}` — the dict is
        // built in that order (`admin.py:370-372`).
        let user_body = crate::common::render(ctx, &admin_user.instance_admin_me())?;
        let mut body = Vec::with_capacity(user_body.len() + 32);
        body.extend_from_slice(b"{\"is_authenticated\":true,\"user\":");
        body.extend_from_slice(&user_body);
        body.push(b'}');
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

async fn has_admin_row(
    client: &deadpool_postgres::Client,
    user_id: Uuid,
) -> Result<bool, ViewError> {
    let statement = client
        .prepare_cached(queries::INSTANCE_ADMIN_EXISTS_FOR_USER.sql)
        .await?;
    let row = client.query_one(&statement, &[&user_id]).await?;
    Ok(row.get(0))
}

/// `request.user` as a full row: the session loader already read it, so this
/// only falls back to a query for a caller without one.
pub(crate) async fn current_user(ctx: &ViewCtx<'_>) -> Result<User, ViewError> {
    if let Some(user) = ctx.user.row() {
        return Ok(user.clone());
    }
    let user_id = ctx
        .user
        .id()
        .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
    let client = client(ctx)?;
    let statement = client
        .prepare_cached(crate::auth::queries::AUTH_USER_BY_ID.sql)
        .await?;
    let row = client
        .query_opt(&statement, &[&user_id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(User::from_row(&row, 0)?)
}

/// `datetime.isoformat()` for a UTC datetime: DRF's `DateTimeField` text.
pub(crate) fn drf_string(value: chrono::DateTime<Utc>) -> String {
    plane_serializers::fields::drf_datetime(value, &chrono_tz::UTC).to_drf_string()
}

/// The two instance-independent 500s of the detail route.
#[allow(dead_code)]
pub(crate) fn detail_500(method: &str) -> ViewError {
    ViewError::Unknown(format!(
        "TypeError: InstanceAdminEndpoint.{method}() got an unexpected keyword argument 'pk'"
    ))
}
