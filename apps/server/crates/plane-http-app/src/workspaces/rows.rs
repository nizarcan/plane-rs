// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Row decoding and response helpers shared by the group's handlers.
//!
//! Three pieces of Python are repeated by almost every view of this group:
//!
//! * `FileAsset.asset_url` for the "static" entity types
//!   (`plane/db/models/asset.py:83-104`), which is what `Workspace.logo_url`
//!   and `User.avatar_url`/`cover_image_url` return;
//! * `UserLiteSerializer` / `UserAdminLiteSerializer`
//!   (`plane/app/serializers/user.py:155-184`) built from a joined `users` row;
//! * the family error bodies a view returns directly instead of raising:
//!   `{"error": …}` (`plane/app/views/base.py` family handler) and
//!   `{"detail": …}` for the DRF `NotFound` that `get_object_or_404` raises
//!   with Django's message.

use plane_core::error::APPLICATION_JSON;
use plane_core::json::Object;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::view::ViewResponse;
use plane_serde::Dialect;
use plane_serializers::generated::app_user::{UserAdminLiteSerializer, UserLiteSerializer};
use tokio_postgres::Row;
use uuid::Uuid;

/// The static entity types `FileAsset.asset_url` serves as
/// `/api/assets/v2/static/<id>/` (`plane/db/models/asset.py:84-91`).
const STATIC_ENTITY_TYPES: [&str; 4] = [
    "WORKSPACE_LOGO",
    "USER_AVATAR",
    "USER_COVER",
    "PROJECT_COVER",
];

/// `FileAsset.asset_url` for an asset the handler did not otherwise load.
///
/// The three non-static branches need the asset's workspace, project and issue
/// rows; they cannot appear for a workspace logo, a user avatar or a user
/// cover, which are the only assets this group serialises. An asset of one of
/// those types and one of the non-static types renders as `None`, which is the
/// Python's last line.
pub fn static_asset_url(entity_type: Option<&str>, id: Option<Uuid>) -> Option<String> {
    let (Some(entity_type), Some(id)) = (entity_type, id) else {
        return None;
    };
    STATIC_ENTITY_TYPES
        .contains(&entity_type)
        .then(|| format!("/api/assets/v2/static/{id}/"))
}

/// `Workspace.logo_url` (`plane/db/models/workspace.py:145-154`):
/// the logo asset when there is one, else `logo` when it is truthy, else `None`.
pub fn logo_url(
    logo_asset_id: Option<Uuid>,
    logo_entity_type: Option<&str>,
    logo: Option<&str>,
) -> Option<String> {
    if let Some(url) = static_asset_url(logo_entity_type, logo_asset_id) {
        return Some(url);
    }
    logo.filter(|value| !value.is_empty()).map(str::to_owned)
}

/// `User.avatar_url` (`plane/db/models/user.py:143-151`).
pub fn user_avatar_url(
    avatar_asset_id: Option<Uuid>,
    entity_type: Option<&str>,
    avatar: &str,
) -> Option<String> {
    if let Some(url) = static_asset_url(entity_type, avatar_asset_id) {
        return Some(url);
    }
    (!avatar.is_empty()).then(|| avatar.to_owned())
}

/// The fields `WorkspaceLiteSerializer` renders
/// (`plane/app/serializers/workspace.py:72-77`), read off a joined `workspaces`
/// row plus the lazy `logo_asset` Django would have fetched separately.
#[derive(Debug)]
pub struct WorkspaceLite {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub logo_url: Option<String>,
}

impl WorkspaceLite {
    pub fn new(
        id: Uuid,
        name: String,
        slug: String,
        logo_asset_id: Option<Uuid>,
        logo_entity_type: Option<&str>,
        logo: Option<&str>,
    ) -> Self {
        Self {
            id,
            name,
            slug,
            logo_url: logo_url(logo_asset_id, logo_entity_type, logo),
        }
    }

    pub fn serializer(
        &self,
    ) -> plane_serializers::generated::app_workspace::WorkspaceLiteSerializer<'_> {
        plane_serializers::generated::app_workspace::WorkspaceLiteSerializer {
            name: &self.name,
            slug: &self.slug,
            id: self.id,
            logo_url: self.logo_url.as_deref(),
        }
    }
}

/// The columns [`UserRow::from_row`] reads, in order.
///
/// `avatar_entity_type` is the joined `file_assets.entity_type` of the nullable
/// `avatar_asset`; it is `None` both when the FK is null and when the row's
/// entity type is not one `asset_url` serves.
#[derive(Debug)]
pub struct UserRow {
    pub id: Uuid,
    pub email: Option<String>,
    pub first_name: String,
    pub last_name: String,
    pub display_name: String,
    pub avatar: String,
    pub avatar_asset_id: Option<Uuid>,
    pub avatar_url: Option<String>,
    pub is_bot: bool,
    pub last_login_medium: String,
}

impl UserRow {
    /// Decode the nine `users` columns plus the avatar asset's entity type, as
    /// every statement in this group selects them.
    pub fn from_row(row: &Row, start: usize) -> Result<Self, plane_db::models::Error> {
        let id: Uuid = row.try_get(start)?;
        let email: Option<String> = row.try_get(start + 1)?;
        let first_name: String = row.try_get(start + 2)?;
        let last_name: String = row.try_get(start + 3)?;
        let avatar: String = row.try_get(start + 4)?;
        let avatar_asset_id: Option<Uuid> = row.try_get(start + 5)?;
        let is_bot: bool = row.try_get(start + 6)?;
        let display_name: String = row.try_get(start + 7)?;
        let last_login_medium: String = row.try_get(start + 8)?;
        let entity_type: Option<String> = row.try_get(start + 9)?;
        let avatar_url = user_avatar_url(avatar_asset_id, entity_type.as_deref(), &avatar);
        Ok(Self {
            id,
            email,
            first_name,
            last_name,
            display_name,
            avatar,
            avatar_asset_id,
            avatar_url,
            is_bot,
            last_login_medium,
        })
    }

    /// `UserLiteSerializer` (`plane/app/serializers/user.py:155-167`).
    pub fn lite(&self) -> UserLiteSerializer<'_> {
        UserLiteSerializer {
            id: self.id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            avatar: &self.avatar,
            avatar_url: self.avatar_url.as_deref(),
            is_bot: self.is_bot,
            display_name: &self.display_name,
        }
    }

    /// `UserAdminLiteSerializer` (`plane/app/serializers/user.py:170-184`).
    pub fn admin_lite(&self) -> UserAdminLiteSerializer<'_> {
        UserAdminLiteSerializer {
            id: self.id,
            first_name: &self.first_name,
            last_name: &self.last_name,
            avatar: &self.avatar,
            avatar_url: self.avatar_url.as_deref(),
            is_bot: self.is_bot,
            display_name: &self.display_name,
            email: self.email.as_deref(),
            last_login_medium: &self.last_login_medium,
        }
    }
}

/// `json`/`jsonb` column to the renderer's borrowed form.
pub fn json(value: Option<&JsonText>) -> Option<plane_serializers::fields::PgJson<'_>> {
    value.map(plane_serializers::fields::PgJson::from)
}

/// The active timezone: `TimezoneMixin`'s user timezone, or `settings.TIME_ZONE`
/// (`UTC`) after `deactivate()` (`plane/app/views/base.py:40-45`).
pub fn timezone(ctx: &plane_drf::view::ViewCtx<'_>) -> chrono_tz::Tz {
    ctx.timezone.copied().unwrap_or(chrono_tz::UTC)
}

/// The render context for **every** serializer of this group.
///
/// None of these views forwards the request's `?fields=` or `?expand=` to its
/// serializer: `fields=` is `DynamicBaseSerializer.__init__`'s keyword argument
/// (`plane/app/serializers/base.py:93-114`) and only a view that passes it —
/// `member.py:72,74,91,93` is the one in this group — has an effect. Each view
/// calls the serializer directly (`UserFavoriteSerializer(favorites, many=True)`)
/// or through `get_serializer()`, which forwards neither. So the mask is `None`
/// and the expansion list is empty, whatever the query string says.
pub fn plain_ctx(tz: &chrono_tz::Tz) -> plane_serializers::RenderCtx<'_> {
    plane_serializers::RenderCtx::new(tz, None, &[])
}

/// Render one object with [`plain_ctx`].
pub fn render_plain<T: plane_serializers::DrfSerialize>(
    ctx: &plane_drf::view::ViewCtx<'_>,
    value: &T,
) -> Result<Vec<u8>, ViewError> {
    let tz = timezone(ctx);
    plane_serializers::render(value, &plain_ctx(&tz)).map_err(ViewError::from)
}

/// Render `many=True` with [`plain_ctx`].
pub fn render_plain_list<T: plane_serializers::DrfSerialize>(
    ctx: &plane_drf::view::ViewCtx<'_>,
    items: &[T],
) -> Result<Vec<u8>, ViewError> {
    let tz = timezone(ctx);
    plane_serializers::render_list(items, &plain_ctx(&tz)).map_err(ViewError::from)
}

/// A family `{"error": …}` body (`plane/app/views/base.py:210-239`).
///
/// These are bodies a view returns itself — `Response({"error": …}, status=…)` —
/// not exceptions, so they are not `ViewError`s. The single-key object is
/// rendered through `Dialect::Drf` like every other DRF response.
pub fn error_body(status: u16, message: &str) -> Result<ViewResponse, ViewError> {
    let body = plane_serde::to_vec(
        Dialect::Drf,
        &Object::new().field("error", message.to_owned()),
    )?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// The 404 `get_object_or_404` renders for a model whose row does not exist:
/// Django raises `Http404("No <Model> matches the given query.")` and DRF's
/// exception handler turns it into `NotFound` with that detail
/// (`django/shortcuts.py:get_object_or_404`,
/// `rest_framework/views.py:exception_handler`).
///
/// `ApiError` has no "404 with a detail" variant — its `NotFound` is DRF's
/// default `"Not found."` — so the body is rendered here, with `Dialect::Drf`
/// and `Object` so the key order and the encoding are DRF's.
pub fn not_found_detail(model: &str) -> Result<ViewResponse, ViewError> {
    let body = plane_serde::to_vec(
        Dialect::Drf,
        &Object::new().field("detail", format!("No {model} matches the given query.")),
    )?;
    Ok(ViewResponse::raw(404, APPLICATION_JSON, body))
}

/// DRF's bare `NotFound` (`raise Http404` with no message), which
/// `get_object_or_404` uses when the lookup value cannot be coerced.
pub fn not_found() -> ViewError {
    ViewError::Api(plane_core::ApiError::NotFound)
}

/// `Model.DoesNotExist` escaping a view: the family handler's 404
/// (`plane/app/views/base.py:222-226`).
pub const OBJECT_DOES_NOT_EXIST: &str = "The required object does not exist.";

/// `MultipleObjectsReturned` from `.get()`, which the Python lets escape as a
/// 500 (`plane/app/views/base.py:235-239`).
pub fn multiple_objects(model: &str) -> ViewError {
    ViewError::Unknown(format!(
        "{model}.MultipleObjectsReturned: get() returned more than one {model}"
    ))
}
