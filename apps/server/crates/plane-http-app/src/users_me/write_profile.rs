// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Set A of the `users-me` writes: the profile and the two onboarding flags
//! (`plans/USERS_ME.md` §3).
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 162 `PATCH /api/users/me/` | `partial_update` | `user/base.py:99-100` → DRF's `UpdateModelMixin` | [`partial_update`] |
//! | 167 `PATCH /api/users/me/profile/` | `patch` | `user/base.py:450-456` | [`profile_patch`] |
//! | 171 `PATCH /api/users/me/onboard/` | `patch` | `user/base.py:391-396` | [`onboard`] |
//! | 172 `PATCH /api/users/me/tour-completed/` | `patch` | `user/base.py:399-404` | [`tour_completed`] |
//!
//! ## Order, no transactions
//!
//! No view opens `transaction.atomic` and `ATOMIC_REQUESTS` is unset, so every
//! statement commits on its own (`HANDLERS.md` §4):
//!
//! * 162: `get_object()` is `request.user` (`user/base.py:69-70`, no query) →
//!   `UserSerializer(instance, data=request.data, partial=True)` →
//!   `is_valid(raise_exception=True)` (400 `serializer.errors`) →
//!   `serializer.save()` → `UserSerializer(instance).data`;
//! * 167: `Profile.objects.get(user=request.user)` (family 404) → serializer
//!   (400) → `save()` → `ProfileSerializer(instance).data`;
//! * 171/172: the profile lookup, then `request.data.get(..., False)` — the
//!   body is parsed **after** the lookup, so a missing profile 404s before a
//!   malformed body 400s → `profile.save()` → the fixed message.
//!
//! ## The two `UserSerializer` field hooks
//!
//! `validate_first_name`/`validate_last_name` reject a URL
//! (`serializers/user.py:16-24` over `plane/utils/url.py:15-53`), and
//! `validate_avatar_asset`/`validate_cover_image_asset` only accept the
//! caller's own asset of the matching entity type (`serializers/user.py:26-38`).
//! DRF runs each hook inside `to_internal_value`, immediately after its field,
//! so the errors carry the serializer's field order; the generated table's own
//! `validate` runs first here and `ValidationErrors::sort_by_field_order`
//! puts the two hooks' errors back into that order.
//!
//! `validate_avatar_asset`/`validate_cover_image_asset` read `user_id` and
//! `entity_type` off the instance `queryset.get(pk=…)` returned. The generated
//! pk lookup only selects the pk, so the port re-reads those two columns — one
//! extra statement per asset field a body carries, which is the only place it
//! diverges from Django's round-trip count (never in the response).
//!
//! ## 171/172 use no serializer
//!
//! `profile.is_onboarded = request.data.get("is_onboarded", False)` reaches
//! Django's `BooleanField.to_python` at save time, whose coercion is *not*
//! DRF's ([`django_boolean`]): `true`/`"t"`/`"True"`/`"1"`/`1` are true,
//! `"0"`/`"f"`/`false` are false, and every other value raises a
//! `ValidationError` — the family 400 `{"error": "Please provide valid
//! detail"}` (`user/base.py:394,402`; `views/base.py:312-316`). A JSON `null` is
//! different: `get_prep_value(None)` returns `None` without raising, so the
//! column's `NOT NULL` makes it the family 400 `{"error": "The payload is not
//! valid"}` instead.

use plane_core::error::APPLICATION_JSON;
use plane_db::models::user::{Profile, User};
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use plane_serializers::generated::app_user::UserSerializer;

use super::queries::USER_PROFILE;
use super::user_base;
use super::write_deactivate::profile_binds;
use super::write_queries::PROFILE_UPDATE;
use super::{write_a_queries, write_email};
use crate::common::{client, render};
use crate::views::write_support::{Body, body_get};
use crate::write::generated::user_serializer;
use crate::write::serializer::Validated;
use crate::write::{self, Detail, Serializer};

// ---------------------------------------------------------------------------
// 162 PATCH /api/users/me/
// ---------------------------------------------------------------------------

/// `UserEndpoint.partial_update` (`plane/app/views/user/base.py:99-100`), which
/// is DRF's `UpdateModelMixin.partial_update` with `get_object()` overridden to
/// `request.user`.
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `get_object()` → `request.user` (`:69-70`): no query, the row the
        // session loader already read.
        let mut user = user_base::user_row(ctx).await?.into_owned();
        let client = client(ctx)?;

        // `UserSerializer(instance, data=request.data, partial=True)`
        // (`rest_framework/mixins.py:78-86`), then `is_valid(raise_exception=True)`.
        let mut serializer = Serializer::new(ctx, true)?;
        user_serializer::validate(&mut serializer, client).await?;
        user_field_hooks(client, &mut serializer, &user).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(mut errors) => {
                // `to_internal_value` reports the keys in `self.fields` order;
                // the hooks above appended theirs, so re-sort. The writable
                // fields keep the same relative order in `_readable_fields`.
                errors.sort_by_field_order(UserSerializer::FIELDS);
                return Ok(errors.into_response());
            }
        };

        // `serializer.save()` → `ModelSerializer.update`
        // (`rest_framework/serializers.py:1260-1290`): assign every validated
        // key, then `instance.save()` with no `update_fields`.
        apply_user(&mut user, &values);
        let user = write_email::save_user(client, user).await?;

        // `Response(serializer.data)`: `UserSerializer(instance).data` from the
        // in-memory row, 200. `BaseSerializer` is not dynamic, so `?fields=`
        // and `?expand=` are ignored (`FilterMode::None` in the table).
        render_user(ctx, &user)
    })
}

/// `UserSerializer`'s four `validate_<field>` hooks, in field order.
///
/// Each runs only when its field produced a value — DRF skips
/// `validate_<field>` for a field that already failed — so the checks read the
/// validated bag rather than the body.
async fn user_field_hooks(
    client: &deadpool_postgres::Client,
    serializer: &mut Serializer<'_>,
    user: &User,
) -> Result<(), ViewError> {
    // `validate_first_name` / `validate_last_name` (`serializers/user.py:16-24`).
    for (field, message) in [
        ("first_name", "First name cannot contain a URL."),
        ("last_name", "Last name cannot contain a URL."),
    ] {
        let Some(value) = serializer.value(field).and_then(text_of) else {
            continue;
        };
        if write::email::contains_url(&value) {
            serializer.push_error(field, Detail::new(message, "invalid"));
        }
    }

    // `validate_avatar_asset` / `validate_cover_image_asset`
    // (`serializers/user.py:26-38`): `value.user_id != self.instance.id or
    // value.entity_type != entity_type`. `self.instance` is `request.user`
    // here, so the two conditions are always evaluated.
    for (field, entity_type) in [
        ("avatar_asset", "USER_AVATAR"),
        ("cover_image_asset", "USER_COVER"),
    ] {
        let Some(asset_id) = serializer.value(field).and_then(uuid_of) else {
            continue;
        };
        if !owns_asset(client, asset_id, user.id, entity_type).await? {
            serializer.push_error(field, Detail::new("Invalid asset.", "invalid"));
        }
    }

    Ok(())
}

/// `value.user_id == user_id and value.entity_type == entity_type`, from the
/// `file_assets` row DRF's `PrimaryKeyRelatedField` loaded
/// (`serializers/user.py:26-38`).
///
/// Django reads the two attributes off the instance `queryset.get(pk=…)`
/// returned; the generated pk lookup only selects the pk, so this re-reads them
/// (one extra statement per asset field the body carries, never more).
async fn owns_asset(
    client: &deadpool_postgres::Client,
    asset_id: uuid::Uuid,
    user_id: uuid::Uuid,
    entity_type: &str,
) -> Result<bool, ViewError> {
    let statement = write_a_queries::FILE_ASSET_OWNER.prepare(client).await?;
    let row = client.query_opt(&statement, &[&asset_id]).await?;
    let Some(row) = row else {
        // The id passed the field's own `FileAsset.objects` lookup, so the row
        // exists; a concurrent hard delete is the only way here, and the
        // in-memory object would have been the one already loaded.
        return Ok(false);
    };
    let owner: Option<uuid::Uuid> = row.get(0);
    let kind: Option<String> = row.get(1);
    Ok(owner == Some(user_id) && kind.as_deref() == Some(entity_type))
}

/// `text_of` for a `Value::Text`.
fn text_of(value: &crate::write::Value) -> Option<String> {
    match value {
        crate::write::Value::Text(text) => Some(text.clone()),
        _ => None,
    }
}

/// `uuid_of` for a `Value::Uuid` (an explicit `null` is accepted and skipped).
fn uuid_of(value: &crate::write::Value) -> Option<uuid::Uuid> {
    match value {
        crate::write::Value::Uuid(id) => Some(*id),
        _ => None,
    }
}

/// `ModelSerializer.update`'s assignments for `UserSerializer`, in its field
/// order.
///
/// Only keys in `validated_data` are assigned, so a `partial=True` body that
/// omits a field keeps the stored value.
fn apply_user(user: &mut User, values: &Validated) {
    // `last_login` is writable (it is not in `Meta.read_only_fields`), so a
    // body value reaches the column.
    if values.is_null("last_login") {
        user.last_login = None;
    } else if let Some(value) = values.datetime("last_login") {
        user.last_login = Some(value);
    }
    if let Some(value) = values.text("display_name") {
        user.display_name = value.to_owned();
    }
    if let Some(value) = values.text("first_name") {
        user.first_name = value.to_owned();
    }
    if let Some(value) = values.text("last_name") {
        user.last_name = value.to_owned();
    }
    if let Some(value) = values.text("avatar") {
        user.avatar = value.to_owned();
    }
    if values.is_null("avatar_asset") {
        user.avatar_asset_id = None;
    } else if let Some(value) = values.uuid("avatar_asset") {
        user.avatar_asset_id = Some(value);
    }
    if values.is_null("cover_image") {
        user.cover_image = None;
    } else if let Some(value) = values.text("cover_image") {
        user.cover_image = Some(value.to_owned());
    }
    if values.is_null("cover_image_asset") {
        user.cover_image_asset_id = None;
    } else if let Some(value) = values.uuid("cover_image_asset") {
        user.cover_image_asset_id = Some(value);
    }
    if let Some(value) = values.bool("is_password_expired") {
        user.is_password_expired = value;
    }
    if let Some(value) = values.bool("is_password_reset_required") {
        user.is_password_reset_required = value;
    }
    if values.is_null("bot_type") {
        user.bot_type = None;
    } else if let Some(value) = values.text("bot_type") {
        user.bot_type = Some(value.to_owned());
    }
    if let Some(value) = values.text("user_timezone") {
        user.user_timezone = value.to_owned();
    }
    if let Some(value) = values.bool("is_email_valid") {
        user.is_email_valid = value;
    }
    if values.is_null("masked_at") {
        user.masked_at = None;
    } else if let Some(value) = values.datetime("masked_at") {
        user.masked_at = Some(value);
    }
}

/// `UserSerializer(instance).data`, 200.
///
/// `PrimaryKeyRelatedField` renders through `Model.serializable_value`, i.e.
/// the `<field>_id` attribute (`rest_framework/relations.py:441-456`,
/// `django/db/models/base.py:590-605`), so the two asset columns cost no query.
fn render_user(ctx: &ViewCtx<'_>, user: &User) -> Result<ViewResponse, ViewError> {
    let serializer = user_serializer(user);
    let body = render(ctx, &serializer)?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

fn user_serializer(user: &User) -> UserSerializer<'_> {
    UserSerializer {
        last_login: user.last_login,
        id: user.id,
        username: &user.username,
        mobile_number: user.mobile_number.as_deref(),
        email: user.email.as_deref(),
        display_name: &user.display_name,
        first_name: &user.first_name,
        last_name: &user.last_name,
        avatar: &user.avatar,
        avatar_asset: user.avatar_asset_id,
        cover_image: user.cover_image.as_deref(),
        cover_image_asset: user.cover_image_asset_id,
        date_joined: user.date_joined,
        created_at: user.created_at,
        updated_at: user.updated_at,
        last_location: &user.last_location,
        created_location: &user.created_location,
        is_superuser: user.is_superuser,
        is_managed: user.is_managed,
        is_password_expired: user.is_password_expired,
        is_active: user.is_active,
        is_staff: user.is_staff,
        is_email_verified: user.is_email_verified,
        is_password_autoset: user.is_password_autoset,
        is_password_reset_required: user.is_password_reset_required,
        token: &user.token,
        last_active: user.last_active,
        last_login_time: user.last_login_time,
        last_logout_time: user.last_logout_time,
        last_login_ip: &user.last_login_ip,
        last_logout_ip: &user.last_logout_ip,
        last_login_medium: &user.last_login_medium,
        last_login_uagent: &user.last_login_uagent,
        token_updated_at: user.token_updated_at,
        is_bot: user.is_bot,
        bot_type: user.bot_type.as_deref(),
        user_timezone: &user.user_timezone,
        is_email_valid: user.is_email_valid,
        masked_at: user.masked_at,
    }
}

// ---------------------------------------------------------------------------
// 167 PATCH /api/users/me/profile/
// ---------------------------------------------------------------------------

/// `ProfileEndpoint.patch` (`plane/app/views/user/base.py:450-456`).
pub fn profile_patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        // `Profile.objects.get(user=request.user)` (`:451`) — before the body
        // is parsed, so a missing profile is the 404 even for a malformed body.
        let mut profile = load_profile(ctx).await?;
        let client = client(ctx)?;

        // `ProfileSerializer(profile, data=request.data, partial=True)`; the
        // view checks `is_valid()` and returns `serializer.errors` itself, so
        // an invalid body is the same 400 body as `raise_exception=True`.
        let mut serializer = Serializer::new(ctx, true)?;
        crate::write::generated::profile_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        apply_profile(&mut profile, &values);
        save_profile(client, &mut profile).await?;

        // `Response(serializer.data)`: `ProfileSerializer(instance).data`, 200.
        render_profile(ctx, &profile)
    })
}

/// `ModelSerializer.update`'s assignments for `ProfileSerializer`, in its field
/// order (`plane/app/serializers/user.py:215-226`, `fields = "__all__"`).
fn apply_profile(profile: &mut Profile, values: &Validated) {
    if let Some(value) = values.json("theme") {
        profile.theme = value.clone();
    }
    if let Some(value) = values.bool("is_app_rail_docked") {
        profile.is_app_rail_docked = value;
    }
    if let Some(value) = values.bool("is_tour_completed") {
        profile.is_tour_completed = value;
    }
    if let Some(value) = values.json("onboarding_step") {
        profile.onboarding_step = value.clone();
    }
    if values.is_null("use_case") {
        profile.use_case = None;
    } else if let Some(value) = values.text("use_case") {
        profile.use_case = Some(value.to_owned());
    }
    if values.is_null("role") {
        profile.role = None;
    } else if let Some(value) = values.text("role") {
        profile.role = Some(value.to_owned());
    }
    if let Some(value) = values.bool("is_onboarded") {
        profile.is_onboarded = value;
    }
    if values.is_null("last_workspace_id") {
        profile.last_workspace_id = None;
    } else if let Some(value) = values.uuid("last_workspace_id") {
        profile.last_workspace_id = Some(value);
    }
    if let Some(value) = values.text("billing_address_country") {
        profile.billing_address_country = value.to_owned();
    }
    if values.is_null("billing_address") {
        profile.billing_address = None;
    } else if let Some(value) = values.json("billing_address") {
        profile.billing_address = Some(value.clone());
    }
    if let Some(value) = values.bool("has_billing_address") {
        profile.has_billing_address = value;
    }
    if let Some(value) = values.text("company_name") {
        profile.company_name = value.to_owned();
    }
    if let Some(value) = values.text("notification_view_mode") {
        profile.notification_view_mode = value.to_owned();
    }
    if let Some(value) = values.bool("is_smooth_cursor_enabled") {
        profile.is_smooth_cursor_enabled = value;
    }
    if let Some(value) = values.bool("is_mobile_onboarded") {
        profile.is_mobile_onboarded = value;
    }
    if let Some(value) = values.json("mobile_onboarding_step") {
        profile.mobile_onboarding_step = value.clone();
    }
    if let Some(value) = values.bool("mobile_timezone_auto_set") {
        profile.mobile_timezone_auto_set = value;
    }
    if let Some(value) = values.text("language") {
        profile.language = value.to_owned();
    }
    // `PositiveSmallIntegerField` over a `ChoiceField`: the validated value is
    // one of 0..=6, so the cast cannot truncate.
    if let Some(value) = values.integer("start_of_the_week") {
        profile.start_of_the_week = i16::try_from(value).unwrap_or(profile.start_of_the_week);
    }
    if let Some(value) = values.json("goals") {
        profile.goals = value.clone();
    }
    if let Some(value) = values.text("background_color") {
        profile.background_color = value.to_owned();
    }
    if let Some(value) = values.bool("is_navigation_tour_completed") {
        profile.is_navigation_tour_completed = value;
    }
    if let Some(value) = values.bool("has_marketing_email_consent") {
        profile.has_marketing_email_consent = value;
    }
    if let Some(value) = values.bool("is_subscribed_to_changelog") {
        profile.is_subscribed_to_changelog = value;
    }
    if let Some(value) = values.json("product_tour") {
        profile.product_tour = value.clone();
    }
}

// ---------------------------------------------------------------------------
// 171 PATCH /api/users/me/onboard/ and 172 PATCH /api/users/me/tour-completed/
// ---------------------------------------------------------------------------

/// `UpdateUserOnBoardedEndpoint.patch` (`plane/app/views/user/base.py:391-396`).
pub fn onboard<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let mut profile = load_profile(ctx).await?;
        // `request.data.get("is_onboarded", False)` (`:394`), coerced by
        // Django's `BooleanField` at save time.
        let value = django_boolean(body_get(ctx.data()?, "is_onboarded")?)?;
        profile.is_onboarded = value;
        save_profile(client(ctx)?, &mut profile).await?;
        Ok(updated())
    })
}

/// `UpdateUserTourCompletedEndpoint.patch`
/// (`plane/app/views/user/base.py:399-404`).
pub fn tour_completed<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let mut profile = load_profile(ctx).await?;
        let value = django_boolean(body_get(ctx.data()?, "is_tour_completed")?)?;
        profile.is_tour_completed = value;
        save_profile(client(ctx)?, &mut profile).await?;
        Ok(updated())
    })
}

/// `Response({"message": "Updated successfully"})` (`:396,404`).
fn updated() -> ViewResponse {
    ViewResponse::json(
        200,
        serde_json::json!({ "message": "Updated successfully" }),
    )
}

/// Django's `BooleanField.to_python` for the value the two views assign
/// (`django/db/models/fields/__init__.py:1421-1440`, reached through
/// `get_prep_value` inside the `UPDATE`).
///
/// Note this is **not** DRF's `BooleanField`: `"true"`/`"false"` (lower case)
/// are a `ValidationError`, because only `"t"`/`"True"`/`"1"` and
/// `"f"`/`"False"`/`"0"` are accepted, and `1`/`0` are matched by Python `==`
/// against `True`/`False`. A JSON `null` is not a coercion failure at all — see
/// the `Json::Null` arm below.
fn django_boolean(value: Body<'_>) -> Result<bool, ViewError> {
    match value {
        // `request.data.get(key, False)`.
        Body::Missing => Ok(false),
        Body::Form(text) => python_boolean_text(text).ok_or(ViewError::Validation),
        Body::Json(Json::Bool(flag)) => Ok(*flag),
        // `value in (True, False)` is Python `==`: 1 and 0 match, every other
        // number falls through to the string comparisons and fails.
        Body::Json(Json::Int(number)) => match number.to_decimal_string().as_str() {
            "1" => Ok(true),
            "0" => Ok(false),
            _ => Err(ViewError::Validation),
        },
        Body::Json(Json::Float(number)) => {
            if *number == 1.0 {
                Ok(true)
            } else if *number == 0.0 {
                Ok(false)
            } else {
                Err(ViewError::Validation)
            }
        }
        Body::Json(Json::Str(text)) => text
            .as_str()
            .and_then(python_boolean_text)
            .ok_or(ViewError::Validation),
        // `BooleanField.get_prep_value(None)` returns `None` without a
        // ValidationError, so the column's `NOT NULL` constraint is what
        // rejects it: the family `IntegrityError` 400, not "Please provide
        // valid detail". Recorded from the real field (Django 5.2.17).
        Body::Json(Json::Null) => Err(ViewError::Integrity),
        Body::Json(_) => Err(ViewError::Validation),
    }
}

fn python_boolean_text(text: &str) -> Option<bool> {
    match text {
        "t" | "True" | "1" => Some(true),
        "f" | "False" | "0" => Some(false),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Shared profile helpers
// ---------------------------------------------------------------------------

/// `Profile.objects.get(user=request.user)` / `.get(user_id=request.user.id)`
/// (`user/base.py:345,393,401,451`) — the family 404 on a miss.
async fn load_profile(ctx: &ViewCtx<'_>) -> Result<Profile, ViewError> {
    let user_id = super::support::user_id(ctx)?;
    let client = client(ctx)?;
    let statement = USER_PROFILE.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&user_id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    Ok(Profile::from_row(&row, 0)?)
}

/// `profile.save()` with no `update_fields`.
///
/// `Profile` is a `TimeAuditModel` with no `save()` override, so this is
/// `Model.save()`: `auto_now` moves `updated_at` (`django/db/models/fields/
/// __init__.py:864-877`) and every column is rewritten. No hook applies.
///
/// `Field.pre_save` assigns the timestamp **on the instance**, so the caller's
/// row carries it too — 167's response renders the in-memory object, and Django
/// answers the new `updated_at`.
pub(super) async fn save_profile(
    client: &deadpool_postgres::Client,
    profile: &mut Profile,
) -> Result<(), ViewError> {
    profile.updated_at = write::now();
    let statement = PROFILE_UPDATE.prepare(client).await?;
    client.execute(&statement, &profile_binds(profile)).await?;
    Ok(())
}

/// `ProfileSerializer(instance).data`, 200.
fn render_profile(ctx: &ViewCtx<'_>, profile: &Profile) -> Result<ViewResponse, ViewError> {
    let serializer = user_base::profile_serializer(profile);
    let body = crate::common::render(ctx, &serializer)?;
    Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn django_booleans_match_boolean_field_to_python() {
        // Recorded from the real `models.BooleanField().get_prep_value` on
        // CPython 3.12.13 / Django 5.2.17.
        for (text, expected) in [
            ("t", Some(true)),
            ("True", Some(true)),
            ("1", Some(true)),
            ("f", Some(false)),
            ("False", Some(false)),
            ("0", Some(false)),
            ("true", None),
            ("false", None),
            ("yes", None),
            ("", None),
            ("T", None),
            ("2", None),
        ] {
            assert_eq!(python_boolean_text(text), expected, "{text:?}");
        }
    }

    #[test]
    fn a_missing_boolean_is_false() {
        use plane_serde::pyjson::{PyInt, PyStr};
        let null = Json::Null;
        let one = Json::Int(PyInt::Small(1));
        let zero = Json::Int(PyInt::Small(0));
        let two = Json::Int(PyInt::Small(2));
        let float_one = Json::Float(1.0);
        let float_zero = Json::Float(-0.0);
        let float_two = Json::Float(2.0);
        let empty = Json::List(Vec::new());
        let text = Json::Str(PyStr::new("true"));
        assert!(!django_boolean(Body::Missing).expect("missing"));
        assert!(!django_boolean(Body::Form("0")).expect("form"));
        assert!(django_boolean(Body::Json(&Json::Bool(true))).expect("bool"));
        assert!(django_boolean(Body::Json(&one)).expect("int 1"));
        assert!(!django_boolean(Body::Json(&zero)).expect("int 0"));
        assert!(django_boolean(Body::Json(&float_one)).expect("float 1"));
        assert!(!django_boolean(Body::Json(&float_zero)).expect("float -0"));
        // `null` is the `NOT NULL` violation, not the boolean coercion error.
        assert_eq!(
            django_boolean(Body::Json(&null)).unwrap_err(),
            ViewError::Integrity
        );
        for bad in [
            Body::Json(&two),
            Body::Json(&float_two),
            Body::Json(&empty),
            Body::Json(&text),
        ] {
            assert_eq!(django_boolean(bad).unwrap_err(), ViewError::Validation);
        }
    }
}
