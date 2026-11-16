// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/app/views/user/base.py` — the signed-in user's own record, its
//! settings, sessions, activity feed, profile and linked accounts.
//!
//! Routes ported here:
//!
//! * `GET /api/users/me/` — `UserEndpoint.retrieve`
//! * `GET /api/users/me/settings/` — `UserEndpoint.retrieve_user_settings`
//! * `GET /api/users/me/instance-admin/` — `UserEndpoint.retrieve_instance_admin`
//! * `GET /api/users/session/` — `UserSessionEndpoint.get`
//! * `GET /api/users/me/profile/` — `ProfileEndpoint.get`
//! * `GET /api/users/me/accounts/` and `.../{pk}/` — `AccountEndpoint.get`
//! * `GET /api/users/me/activities/` — `UserActivityEndpoint.get`
//!
//! ## Side effects
//!
//! None of these reads writes a row, invalidates a cache key or enqueues a
//! task. `retrieve`, `retrieve_user_settings` and `ProfileEndpoint.get` carry
//! `@cache_control(private=True, max_age=12)` / `@vary_on_cookie`
//! (`plane/app/views/user/base.py:77,88,441`); the runtime already emits
//! `Vary: Cookie, origin`, so only `Cache-Control` is added here.
//!
//! ## Round trips
//!
//! Django's auth layer loads the session and the whole `users` row, and
//! `GET /api/users/me/` then serialises that object, touching one
//! `file_assets` row per non-null avatar/cover asset. This port does the same:
//! the auth layer keeps the full row (`plane_auth::SessionUser`) and the
//! handler issues the asset queries the model properties do, no more and no
//! fewer. `GET /api/users/session/` re-reads the row, as its Python does. The
//! SQL docs in `queries.rs` name each statement.

use std::borrow::Cow;

use chrono::NaiveDate;
use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::user::{Account, Profile, User};
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_issue::{IssueActivitySerializer, IssueFlatSerializer};
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use plane_serializers::generated::app_user::{
    AccountSerializer, ProfileSerializer, UserLiteSerializer, UserMeSerializer,
    UserMeSettingsSerializer,
};
use plane_serializers::generated::app_workspace::WorkspaceLiteSerializer;
use plane_serializers::value::DrfValue;
use plane_serializers::writer::DrfWriter;
use plane_serializers::{PgJson, RenderCtx};
use uuid::Uuid;

use super::queries;
use super::support;
use crate::common::asset_url::{self, AssetRef};

/// Register every handler this Python module owns.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.user.base.UserEndpoint")?
        .action("retrieve", retrieve)?
        .action("retrieve_user_settings", retrieve_user_settings)?
        .action("retrieve_instance_admin", retrieve_instance_admin)?;
    registry
        .api_view("plane.app.views.user.base.UserSessionEndpoint")?
        .method(Method::GET, session)?;
    registry
        .api_view("plane.app.views.user.base.ProfileEndpoint")?
        .method(Method::GET, profile)?;
    registry
        .api_view("plane.app.views.user.base.AccountEndpoint")?
        .method(Method::GET, accounts)?;
    registry
        .api_view("plane.app.views.user.base.UserActivityEndpoint")?
        .method(Method::GET, activities)?;
    Ok(())
}

/// `@cache_control(private=True, max_age=12)` plus `@vary_on_cookie`
/// (`plane/app/views/user/base.py:77-78,440-441`).
///
/// The runtime would add `Vary: Cookie` later in the stack; `vary_on_cookie`
/// adds it *inside* the handler, before `GZipMiddleware`, so the order Django
/// sends is `Cookie, Accept-Encoding, origin`. Setting it here keeps that
/// order (the later patches are idempotent for `Cookie`).
fn with_private_cache(response: ViewResponse) -> ViewResponse {
    response
        .with_header("Vary", "Cookie")
        .with_header("Cache-Control", "private, max-age=12")
}

/// The full `users` row plus its avatar/cover assets, loaded once per request.
///
/// `user` is a `Cow` because `GET /api/users/me/` borrows the row the auth
/// layer loaded (Django's `request.user`), while `GET /api/users/session/`
/// re-reads it the way its Python does. Cloning the row would allocate its ~15
/// strings again for nothing.
struct LoadedUser<'a> {
    user: Cow<'a, User>,
    avatar: AssetRef,
    cover: AssetRef,
}

/// The `file_assets` row a `users.avatar_asset_id`/`cover_image_asset_id`
/// points at.
///
/// Django issues one query per non-null foreign key (`request.user` is not
/// `select_related`), so this is one query per set id, not a join on the user
/// row. A missing row cannot happen under the foreign key; it is treated as no
/// asset.
async fn load_asset(ctx: &ViewCtx<'_>, id: Option<Uuid>) -> Result<AssetRef, ViewError> {
    let Some(id) = id else {
        return Ok(AssetRef::default());
    };
    let client = crate::common::client(ctx)?;
    let statement = queries::ASSET_BY_ID.prepare(client).await?;
    match client.query_opt(&statement, &[&id]).await? {
        Some(row) => Ok(AssetRef::from_row(&row, 0)?),
        None => Ok(AssetRef::default()),
    }
}

/// `request.user` plus the two asset properties `UserMeSerializer` reads.
///
/// `UserEndpoint.retrieve` serialises `request.user` itself
/// (`plane/app/views/user/base.py:80-83`), so nothing here queries `users`
/// again: the row comes from the session loader (DECISIONS.md D1's
/// "one query per round trip").
async fn load_request_user<'a>(ctx: &'a ViewCtx<'a>) -> Result<LoadedUser<'a>, ViewError> {
    if let Some(user) = ctx.user.row() {
        return loaded_user(ctx, Cow::Borrowed(user)).await;
    }
    // A caller authenticated without the session loader (none of this group's
    // routes has another authenticator) falls back to Django's `get_object`.
    let user_id = support::user_id(ctx)?;
    load_user(ctx, user_id).await
}

/// `User.objects.get(pk=…)` plus the two asset properties.
///
/// `UserSessionEndpoint.get` re-reads the user even though the auth layer just
/// loaded it (`plane/app/views/user/base.py:380-382`), so this is a query.
async fn load_user<'a>(ctx: &ViewCtx<'a>, user_id: Uuid) -> Result<LoadedUser<'a>, ViewError> {
    let client = crate::common::client(ctx)?;
    let statement = queries::USER_BY_ID.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&user_id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    let user = User::from_row(&row, 0)?;
    loaded_user(ctx, Cow::Owned(user)).await
}

async fn loaded_user<'a>(
    ctx: &ViewCtx<'_>,
    user: Cow<'a, User>,
) -> Result<LoadedUser<'a>, ViewError> {
    let avatar = load_asset(ctx, user.avatar_asset_id).await?;
    let cover = load_asset(ctx, user.cover_image_asset_id).await?;
    Ok(LoadedUser {
        user,
        avatar,
        cover,
    })
}

/// The two strings `UserMeSerializer` computes from the assets.
struct UserMeParts {
    avatar_url: Option<String>,
    cover_image_url: Option<String>,
}

fn user_me_parts(loaded: &LoadedUser) -> Result<UserMeParts, ViewError> {
    Ok(UserMeParts {
        avatar_url: asset_url::user_avatar_url(&loaded.user.avatar, &loaded.avatar)?,
        cover_image_url: asset_url::user_cover_image_url(
            loaded.user.cover_image.as_deref(),
            &loaded.cover,
        )?,
    })
}

fn user_me_serializer<'a>(loaded: &'a LoadedUser, parts: &'a UserMeParts) -> UserMeSerializer<'a> {
    let user = &loaded.user;
    UserMeSerializer {
        id: user.id,
        avatar: &user.avatar,
        cover_image: user.cover_image.as_deref(),
        avatar_url: parts.avatar_url.as_deref(),
        cover_image_url: parts.cover_image_url.as_deref(),
        date_joined: user.date_joined,
        display_name: &user.display_name,
        email: user.email.as_deref(),
        first_name: &user.first_name,
        last_name: &user.last_name,
        is_active: user.is_active,
        is_bot: user.is_bot,
        is_email_verified: user.is_email_verified,
        user_timezone: &user.user_timezone,
        username: &user.username,
        is_password_autoset: user.is_password_autoset,
        last_login_medium: &user.last_login_medium,
        last_login_time: user.last_login_time,
    }
}

/// `UserEndpoint.retrieve` (`plane/app/views/user/base.py:77-85`).
fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let loaded = load_request_user(ctx).await?;
        let parts = user_me_parts(&loaded)?;
        let body = crate::common::render(ctx, &user_me_serializer(&loaded, &parts))?;
        Ok(with_private_cache(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            body,
        )))
    })
}

/// The `workspace` value `UserMeSettingsSerializer.get_workspace` returns
/// (`plane/app/serializers/user.py:112-143`), resolved before it is turned
/// into a [`DrfValue`] so the strings outlive the serializer.
enum SettingsWorkspace {
    LastVisited {
        id: Uuid,
        slug: String,
        name: String,
        /// `None`: the workspace has no logo asset, and Django stores `""`.
        /// `Some(None)`: the asset exists but `asset_url` is `None` (an
        /// unknown entity type), which Django stores as JSON `null`.
        logo: Option<Option<String>>,
        invites: i64,
    },
    Fallback {
        id: Option<Uuid>,
        slug: Option<String>,
        invites: i64,
    },
}

async fn settings_workspace(
    ctx: &ViewCtx<'_>,
    user_id: Uuid,
    email: &str,
) -> Result<SettingsWorkspace, ViewError> {
    let client = crate::common::client(ctx)?;

    // 1. `WorkspaceMemberInvite.objects.filter(email=obj.email).count()`.
    let statement = queries::USER_INVITE_COUNT.prepare(client).await?;
    let invites: i64 = client.query_one(&statement, &[&email]).await?.get(0);

    // 2. `Profile.objects.get(user=obj)`.
    let statement = queries::USER_PROFILE.prepare(client).await?;
    let row = client
        .query_opt(&statement, &[&user_id])
        .await?
        .ok_or(ViewError::ObjectDoesNotExist)?;
    let profile = Profile::from_row(&row, 0)?;

    // 3. `profile.last_workspace_id is not None and Workspace.objects.filter(
    //    pk=…, workspace_member__member=…, is_active=True).exists()`, then
    //    `.first()`. Django runs the same filter twice; one statement here.
    if let Some(last_workspace_id) = profile.last_workspace_id {
        let statement = queries::WORKSPACE_FOR_USER_BY_ID.prepare(client).await?;
        if let Some(row) = client
            .query_opt(&statement, &[&last_workspace_id, &user_id])
            .await?
        {
            let workspace = plane_db::models::workspace::Workspace::from_row(&row, 0)?;
            let logo =
                AssetRef::from_row(&row, plane_db::models::workspace::Workspace::COLUMN_COUNT)?;
            // `workspace.logo_asset.asset_url if workspace.logo_asset is not
            // None else ""` (`plane/app/serializers/user.py:130`): the logo
            // column is not a fallback in this branch.
            let logo_url = if logo.is_some() {
                Some(logo.asset_url()?)
            } else {
                None
            };
            return Ok(SettingsWorkspace::LastVisited {
                id: last_workspace_id,
                slug: workspace.slug,
                name: workspace.name,
                logo: logo_url,
                invites,
            });
        }
    }

    // 4. The fallback branch.
    let statement = queries::FALLBACK_WORKSPACE_FOR_USER.prepare(client).await?;
    let fallback = client.query_opt(&statement, &[&user_id]).await?;
    Ok(match &fallback {
        Some(row) => {
            let workspace = plane_db::models::workspace::Workspace::from_row(row, 0)?;
            SettingsWorkspace::Fallback {
                id: Some(workspace.id),
                slug: Some(workspace.slug),
                invites,
            }
        }
        None => SettingsWorkspace::Fallback {
            id: None,
            slug: None,
            invites,
        },
    })
}

fn settings_workspace_value(workspace: &SettingsWorkspace) -> DrfValue<'_> {
    match workspace {
        SettingsWorkspace::LastVisited {
            id,
            slug,
            name,
            logo,
            invites,
        } => DrfValue::Object(vec![
            ("last_workspace_id", DrfValue::Uuid(*id)),
            ("last_workspace_slug", DrfValue::Str(slug)),
            ("last_workspace_name", DrfValue::Str(name)),
            (
                "last_workspace_logo",
                match logo {
                    None => DrfValue::Str(""),
                    Some(Some(url)) => DrfValue::Str(url),
                    Some(None) => DrfValue::Null,
                },
            ),
            ("fallback_workspace_id", DrfValue::Uuid(*id)),
            ("fallback_workspace_slug", DrfValue::Str(slug)),
            ("invites", DrfValue::Int(*invites)),
        ]),
        SettingsWorkspace::Fallback { id, slug, invites } => DrfValue::Object(vec![
            ("last_workspace_id", DrfValue::Null),
            ("last_workspace_slug", DrfValue::Null),
            (
                "fallback_workspace_id",
                id.map_or(DrfValue::Null, DrfValue::Uuid),
            ),
            (
                "fallback_workspace_slug",
                slug.as_deref().map_or(DrfValue::Null, DrfValue::Str),
            ),
            ("invites", DrfValue::Int(*invites)),
        ]),
    }
}

/// `UserEndpoint.retrieve_user_settings` (`plane/app/views/user/base.py:88-91`).
fn retrieve_user_settings<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user_id = support::user_id(ctx)?;
        let email = support::user_email(ctx)?;
        let auth_user = ctx.user.user().ok_or(ViewError::Key)?;
        let workspace = settings_workspace(ctx, user_id, &email).await?;
        let serializer = UserMeSettingsSerializer {
            id: auth_user.id,
            email: Some(auth_user.email.as_str()),
            workspace: settings_workspace_value(&workspace),
        };
        let body = crate::common::render(ctx, &serializer)?;
        Ok(with_private_cache(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            body,
        )))
    })
}

/// `UserEndpoint.retrieve_instance_admin`
/// (`plane/app/views/user/base.py:93-97`).
fn retrieve_instance_admin<'a>(
    ctx: &'a ViewCtx<'a>,
) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user_id = support::user_id(ctx)?;
        let client = crate::common::client(ctx)?;
        let statement = queries::INSTANCE_ADMIN.prepare(client).await?;
        let is_admin = match client.query_opt(&statement, &[&user_id]).await? {
            Some(row) => row.get::<_, bool>(1),
            None => false,
        };
        Ok(ViewResponse::json(
            200,
            serde_json::json!({ "is_instance_admin": is_admin }),
        ))
    })
}

/// `UserSessionEndpoint.get` (`plane/app/views/user/base.py:377-388`).
///
/// `AllowAny`: an anonymous caller gets `{"is_authenticated": false}` without
/// a second database read.
fn session<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if !ctx.user.is_authenticated() {
            return Ok(ViewResponse::json(
                200,
                serde_json::json!({ "is_authenticated": false }),
            ));
        }
        let user_id = support::user_id(ctx)?;
        let loaded = load_user(ctx, user_id).await?;
        let parts = user_me_parts(&loaded)?;
        let serializer = user_me_serializer(&loaded, &parts);

        // `data = {"is_authenticated": True}; data["user"] = serializer.data`
        // — insertion order is visible on the wire.
        let prepared = crate::common::RequestRender::from_ctx(ctx);
        let render_ctx = prepared.render_ctx();
        let mut writer = DrfWriter::with_capacity(1024);
        {
            let mut object = writer.begin_object();
            object.field("is_authenticated", &true)?;
            object.field_nested("user", &render_ctx, Some(&serializer))?;
            object.end();
        }
        Ok(ViewResponse::raw(200, APPLICATION_JSON, writer.into_vec()))
    })
}

/// `ProfileEndpoint.get` (`plane/app/views/user/base.py:441-448`).
fn profile<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user_id = support::user_id(ctx)?;
        let client = crate::common::client(ctx)?;
        let statement = queries::USER_PROFILE.prepare(client).await?;
        let row = client
            .query_opt(&statement, &[&user_id])
            .await?
            .ok_or(ViewError::ObjectDoesNotExist)?;
        let profile = Profile::from_row(&row, 0)?;
        let serializer = profile_serializer(&profile);
        let body = crate::common::render(ctx, &serializer)?;
        Ok(with_private_cache(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            body,
        )))
    })
}

fn profile_serializer(profile: &Profile) -> ProfileSerializer<'_> {
    ProfileSerializer {
        id: profile.id,
        created_at: profile.created_at,
        updated_at: profile.updated_at,
        theme: Some(PgJson::from_json_text(&profile.theme)),
        is_app_rail_docked: profile.is_app_rail_docked,
        is_tour_completed: profile.is_tour_completed,
        onboarding_step: Some(PgJson::from_json_text(&profile.onboarding_step)),
        use_case: profile.use_case.as_deref(),
        role: profile.role.as_deref(),
        is_onboarded: profile.is_onboarded,
        last_workspace_id: profile.last_workspace_id,
        billing_address_country: &profile.billing_address_country,
        billing_address: profile.billing_address.as_ref().map(PgJson::from_json_text),
        has_billing_address: profile.has_billing_address,
        company_name: &profile.company_name,
        notification_view_mode: &profile.notification_view_mode,
        is_smooth_cursor_enabled: profile.is_smooth_cursor_enabled,
        is_mobile_onboarded: profile.is_mobile_onboarded,
        mobile_onboarding_step: Some(PgJson::from_json_text(&profile.mobile_onboarding_step)),
        mobile_timezone_auto_set: profile.mobile_timezone_auto_set,
        language: &profile.language,
        start_of_the_week: i64::from(profile.start_of_the_week),
        goals: Some(PgJson::from_json_text(&profile.goals)),
        background_color: &profile.background_color,
        is_navigation_tour_completed: profile.is_navigation_tour_completed,
        has_marketing_email_consent: profile.has_marketing_email_consent,
        is_subscribed_to_changelog: profile.is_subscribed_to_changelog,
        product_tour: Some(PgJson::from_json_text(&profile.product_tour)),
        user: profile.user_id,
    }
}

/// `AccountEndpoint.get` (`plane/app/views/user/base.py:425-436`).
fn accounts<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user_id = support::user_id(ctx)?;
        let client = crate::common::client(ctx)?;
        if let Some(pk) = ctx.kwarg("pk") {
            // The route's `<uuid:pk>` converter already matched, so a parse
            // failure cannot happen; Django would answer 404 before the view.
            let pk =
                Uuid::parse_str(pk).map_err(|_| ViewError::Api(plane_core::ApiError::NotFound))?;
            let statement = queries::USER_ACCOUNT_BY_ID.prepare(client).await?;
            let row = client
                .query_opt(&statement, &[&pk, &user_id])
                .await?
                .ok_or(ViewError::ObjectDoesNotExist)?;
            let account = Account::from_row(&row, 0)?;
            let body = crate::common::render(ctx, &account_serializer(&account))?;
            return Ok(ViewResponse::raw(200, APPLICATION_JSON, body));
        }

        let statement = queries::USER_ACCOUNTS.prepare(client).await?;
        let rows = client.query(&statement, &[&user_id]).await?;
        let mut accounts = Vec::with_capacity(rows.len());
        for row in &rows {
            accounts.push(Account::from_row(row, 0)?);
        }
        let serializers: Vec<AccountSerializer> = accounts.iter().map(account_serializer).collect();
        let prepared = crate::common::RequestRender::from_ctx(ctx);
        let body = crate::common::render_list(&serializers, &prepared.render_ctx())?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

fn account_serializer(account: &Account) -> AccountSerializer<'_> {
    AccountSerializer {
        id: account.id,
        created_at: account.created_at,
        updated_at: account.updated_at,
        provider_account_id: &account.provider_account_id,
        provider: &account.provider,
        access_token: &account.access_token,
        access_token_expired_at: account.access_token_expired_at,
        refresh_token: account.refresh_token.as_deref(),
        refresh_token_expired_at: account.refresh_token_expired_at,
        last_connected_at: account.last_connected_at,
        id_token: &account.id_token,
        metadata: Some(PgJson::from_json_text(&account.metadata)),
        user: account.user_id,
    }
}

/// `sanitize_order_by(request.GET.get("order_by", "-created_at"),
/// ACTIVITY_ORDER_BY_ALLOWLIST, "-created_at")`
/// (`plane/utils/order_queryset.py:135-156`).
///
/// Returns `(field, descending)`; a value outside the allowlist, or with a
/// malformed `--` prefix, becomes `("-created_at")`.
fn activity_order(value: Option<&str>) -> (&'static str, bool) {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return ("created_at", true);
    };
    let desc = value.starts_with('-');
    let bare = if desc { &value[1..] } else { value };
    if bare.starts_with('-') {
        return ("created_at", true);
    }
    match bare {
        "created_at" => ("created_at", desc),
        "updated_at" => ("updated_at", desc),
        _ => ("created_at", true),
    }
}

/// The `ORDER BY` clause `OffsetPaginator` builds (`paginator.py:116-122`).
///
/// Django's `order_by(F(key).desc(nulls_last=True), "-created_at")`
/// de-duplicates `created_at`: the captured SQL for `created_at` has one term,
/// `updated_at` two.
fn activity_order_sql(key: &str, desc: bool) -> String {
    let direction = if desc { "DESC" } else { "ASC" };
    if key == "created_at" {
        format!("a.\"created_at\" {direction} NULLS LAST")
    } else {
        format!("a.\"{key}\" {direction} NULLS LAST, a.\"created_at\" DESC")
    }
}

/// The activity page's `SELECT`/`FROM`/`WHERE`, in column order
/// [`ActivityRow::from_row`] reads.
const ACTIVITY_SELECT: &str = "\
SELECT a.\"created_at\" AS a_created_at, a.\"updated_at\" AS a_updated_at, \
 a.\"created_by_id\" AS a_created_by_id, a.\"updated_by_id\" AS a_updated_by_id, \
 a.\"deleted_at\" AS a_deleted_at, a.\"id\" AS a_id, a.\"project_id\" AS a_project_id, \
 a.\"workspace_id\" AS a_workspace_id, a.\"issue_id\" AS a_issue_id, a.\"verb\" AS a_verb, \
 a.\"field\" AS a_field, a.\"old_value\" AS a_old_value, a.\"new_value\" AS a_new_value, \
 a.\"comment\" AS a_comment, a.\"attachments\" AS a_attachments, \
 a.\"issue_comment_id\" AS a_issue_comment_id, a.\"actor_id\" AS a_actor_id, \
 a.\"old_identifier\" AS a_old_identifier, a.\"new_identifier\" AS a_new_identifier, \
 a.\"epoch\" AS a_epoch, \
 p.\"id\" AS p_id, p.\"identifier\" AS p_identifier, p.\"name\" AS p_name, \
 p.\"cover_image\" AS p_cover_image, p.\"cover_image_asset_id\" AS p_cover_image_asset_id, \
 p.\"logo_props\" AS p_logo_props, p.\"description\" AS p_description, \
 w.\"id\" AS w_id, w.\"name\" AS w_name, w.\"slug\" AS w_slug, w.\"logo\" AS w_logo, \
 w.\"logo_asset_id\" AS w_logo_asset_id, \
 u.\"id\" AS u_id, u.\"first_name\" AS u_first_name, u.\"last_name\" AS u_last_name, \
 u.\"avatar\" AS u_avatar, u.\"is_bot\" AS u_is_bot, u.\"display_name\" AS u_display_name, \
 i.\"id\" AS i_id, i.\"name\" AS i_name, i.\"description_json\" AS i_description_json, \
 i.\"description_html\" AS i_description_html, i.\"priority\" AS i_priority, \
 i.\"start_date\" AS i_start_date, i.\"target_date\" AS i_target_date, \
 i.\"sequence_id\" AS i_sequence_id, i.\"sort_order\" AS i_sort_order, \
 i.\"is_draft\" AS i_is_draft, \
 av.\"id\" AS av_id, av.\"entity_type\" AS av_entity_type, \
 avw.\"slug\" AS av_workspace_slug, av.\"project_id\" AS av_project_id, \
 av.\"issue_id\" AS av_issue_id, \
 cv.\"id\" AS cv_id, cv.\"entity_type\" AS cv_entity_type, \
 cvw.\"slug\" AS cv_workspace_slug, cv.\"project_id\" AS cv_project_id, \
 cv.\"issue_id\" AS cv_issue_id, \
 lv.\"id\" AS lv_id, lv.\"entity_type\" AS lv_entity_type, \
 lvw.\"slug\" AS lv_workspace_slug, lv.\"project_id\" AS lv_project_id, \
 lv.\"issue_id\" AS lv_issue_id \
 FROM issue_activities a \
 INNER JOIN users u ON a.\"actor_id\" = u.\"id\" \
 INNER JOIN projects p ON a.\"project_id\" = p.\"id\" \
 INNER JOIN workspaces w ON a.\"workspace_id\" = w.\"id\" \
 LEFT OUTER JOIN issues i ON a.\"issue_id\" = i.\"id\" \
 LEFT JOIN file_assets av ON av.\"id\" = u.\"avatar_asset_id\" \
 LEFT JOIN workspaces avw ON avw.\"id\" = av.\"workspace_id\" \
 LEFT JOIN file_assets cv ON cv.\"id\" = p.\"cover_image_asset_id\" \
 LEFT JOIN workspaces cvw ON cvw.\"id\" = cv.\"workspace_id\" \
 LEFT JOIN file_assets lv ON lv.\"id\" = w.\"logo_asset_id\" \
 LEFT JOIN workspaces lvw ON lvw.\"id\" = lv.\"workspace_id\" \
 WHERE a.\"deleted_at\" IS NULL AND a.\"actor_id\" = $1";

/// One decoded activity row, owning the strings its nested serializers borrow.
#[derive(Debug)]
struct ActivityRow {
    activity: plane_db::models::issue::IssueActivity,
    project: ProjectParts,
    workspace: WorkspaceParts,
    actor: ActorParts,
    issue: Option<IssueParts>,
    project_cover_image_url: Option<String>,
    workspace_logo_url: Option<String>,
    actor_avatar_url: Option<String>,
}

#[derive(Debug)]
struct ProjectParts {
    id: Uuid,
    identifier: String,
    name: String,
    cover_image: Option<String>,
    logo_props: plane_db::JsonText,
    description: String,
}

#[derive(Debug)]
struct WorkspaceParts {
    id: Uuid,
    name: String,
    slug: String,
    logo: Option<String>,
}

#[derive(Debug)]
struct ActorParts {
    id: Uuid,
    first_name: String,
    last_name: String,
    avatar: String,
    is_bot: bool,
    display_name: String,
}

#[derive(Debug)]
struct IssueParts {
    id: Uuid,
    name: String,
    description_json: plane_db::JsonText,
    description_html: String,
    priority: String,
    start_date: Option<NaiveDate>,
    target_date: Option<NaiveDate>,
    sequence_id: i32,
    sort_order: f64,
    is_draft: bool,
}

impl ActivityRow {
    fn from_row(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        // Indexes follow `ACTIVITY_SELECT`'s column order.
        let activity = plane_db::models::issue::IssueActivity {
            created_at: row.try_get(0)?,
            updated_at: row.try_get(1)?,
            created_by_id: row.try_get(2)?,
            updated_by_id: row.try_get(3)?,
            deleted_at: row.try_get(4)?,
            id: row.try_get(5)?,
            project_id: row.try_get(6)?,
            workspace_id: row.try_get(7)?,
            issue_id: row.try_get(8)?,
            verb: row.try_get(9)?,
            field: row.try_get(10)?,
            old_value: row.try_get(11)?,
            new_value: row.try_get(12)?,
            comment: row.try_get(13)?,
            attachments: row.try_get(14)?,
            issue_comment_id: row.try_get(15)?,
            actor_id: row.try_get(16)?,
            old_identifier: row.try_get(17)?,
            new_identifier: row.try_get(18)?,
            epoch: row.try_get(19)?,
        };
        let project = ProjectParts {
            id: row.try_get(20)?,
            identifier: row.try_get(21)?,
            name: row.try_get(22)?,
            cover_image: row.try_get(23)?,
            logo_props: row.try_get(25)?,
            description: row.try_get(26)?,
        };
        let workspace = WorkspaceParts {
            id: row.try_get(27)?,
            name: row.try_get(28)?,
            slug: row.try_get(29)?,
            logo: row.try_get(30)?,
        };
        let actor = ActorParts {
            id: row.try_get(32)?,
            first_name: row.try_get(33)?,
            last_name: row.try_get(34)?,
            avatar: row.try_get(35)?,
            is_bot: row.try_get(36)?,
            display_name: row.try_get(37)?,
        };
        let issue = if row.try_get::<_, Option<Uuid>>(38)?.is_some() {
            Some(IssueParts {
                id: row.try_get(38)?,
                name: row.try_get(39)?,
                description_json: row.try_get(40)?,
                description_html: row.try_get(41)?,
                priority: row.try_get(42)?,
                start_date: row.try_get(43)?,
                target_date: row.try_get(44)?,
                sequence_id: row.try_get(45)?,
                sort_order: row.try_get(46)?,
                is_draft: row.try_get(47)?,
            })
        } else {
            None
        };
        let actor_asset = AssetRef::from_row(row, 48)?;
        let project_asset = AssetRef::from_row(row, 53)?;
        let workspace_asset = AssetRef::from_row(row, 58)?;
        let project_cover_image_url =
            asset_url::project_cover_image_url(project.cover_image.as_deref(), &project_asset)?;
        let workspace_logo_url =
            asset_url::workspace_logo_url(workspace.logo.as_deref(), &workspace_asset)?;
        let actor_avatar_url = asset_url::user_avatar_url(&actor.avatar, &actor_asset)?;
        Ok(Self {
            activity,
            project,
            workspace,
            actor,
            issue,
            project_cover_image_url,
            workspace_logo_url,
            actor_avatar_url,
        })
    }
}

/// `IssueActivityEndpoint.get` (`plane/app/views/user/base.py:414-422`).
fn activities<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let user_id = support::user_id(ctx)?;
        let page = support::page_query(ctx.query(), 1000, 1000)?;
        let (key, desc) = activity_order(ctx.query().get("order_by"));
        let limit = i64::from(page.per_page);
        let offset = page.cursor.offset.saturating_mul(limit);

        // `results = results[-(limit + 1):]` with a negative slice start:
        // Django's QuerySet raises `ValueError: Negative indexing is not
        // supported.` — a 500, before any query runs.
        if page.cursor.is_prev && page.cursor.value != page.per_page as f64 {
            return Err(ViewError::Unknown(
                "ValueError: Negative indexing is not supported.".to_string(),
            ));
        }

        let client = crate::common::client(ctx)?;
        let sql = format!(
            "{ACTIVITY_SELECT} ORDER BY {} LIMIT $2 OFFSET $3",
            activity_order_sql(key, desc)
        );
        let fetch = limit.saturating_add(1);
        let statement = client.prepare(&sql).await?;
        let rows = client
            .query(&statement, &[&user_id, &fetch, &offset])
            .await?;
        let mut activities = Vec::with_capacity(rows.len());
        for row in &rows {
            activities.push(ActivityRow::from_row(row)?);
        }

        let count_statement = queries::USER_ACTIVITY_COUNT.prepare(client).await?;
        let total: i64 = client
            .query_one(&count_statement, &[&user_id])
            .await?
            .get(0);

        let visible = activities.len().min(limit as usize);
        let next_has_results = activities.len() as i64 > limit;
        let page_index = page.cursor.offset;
        let total_pages = (total + limit - 1) / limit;

        let prepared = crate::common::RequestRender::from_ctx(ctx);
        let render_ctx = prepared.render_ctx();
        let mut writer = DrfWriter::with_capacity(rows.len() * 512 + 256);
        {
            let mut object = writer.begin_object();
            object.null_field("grouped_by")?;
            object.null_field("sub_grouped_by")?;
            object.field("total_count", &total)?;
            object.field("next_cursor", &format!("{limit}:{}:0", page_index + 1))?;
            object.field("prev_cursor", &format!("{limit}:{}:1", page_index - 1))?;
            object.field("next_page_results", &next_has_results)?;
            object.field("prev_page_results", &(page_index > 0))?;
            object.field("count", &(visible as i64))?;
            object.field("total_pages", &total_pages)?;
            object.field("total_results", &total)?;
            object.null_field("extra_stats")?;
            let items: Vec<ActivityItem> = activities[..visible].iter().map(ActivityItem).collect();
            object.field_nested_list("results", &render_ctx, &items)?;
            object.end();
        }
        Ok(ViewResponse::raw(200, APPLICATION_JSON, writer.into_vec()))
    })
}

/// One `IssueActivitySerializer` as an element of the envelope's `results`.
#[derive(Debug)]
struct ActivityItem<'a>(&'a ActivityRow);

impl plane_serializers::DrfSerialize for ActivityItem<'_> {
    fn drf_write(
        &self,
        writer: &mut DrfWriter,
        ctx: &RenderCtx<'_>,
    ) -> Result<(), plane_serializers::DrfError> {
        let row = self.0;
        let project = ProjectLiteSerializer {
            id: row.project.id,
            identifier: &row.project.identifier,
            name: &row.project.name,
            cover_image: row.project.cover_image.as_deref(),
            cover_image_url: row.project_cover_image_url.as_deref(),
            logo_props: Some(PgJson::from_json_text(&row.project.logo_props)),
            description: &row.project.description,
        };
        let workspace = WorkspaceLiteSerializer {
            name: &row.workspace.name,
            slug: &row.workspace.slug,
            id: row.workspace.id,
            logo_url: row.workspace_logo_url.as_deref(),
        };
        let actor = UserLiteSerializer {
            id: row.actor.id,
            first_name: &row.actor.first_name,
            last_name: &row.actor.last_name,
            avatar: &row.actor.avatar,
            avatar_url: row.actor_avatar_url.as_deref(),
            is_bot: row.actor.is_bot,
            display_name: &row.actor.display_name,
        };
        let issue = row.issue.as_ref().map(|parts| IssueFlatSerializer {
            id: parts.id,
            name: &parts.name,
            description_json: Some(PgJson::from_json_text(&parts.description_json)),
            description_html: &parts.description_html,
            priority: &parts.priority,
            start_date: parts.start_date,
            target_date: parts.target_date,
            sequence_id: i64::from(parts.sequence_id),
            sort_order: parts.sort_order,
            is_draft: parts.is_draft,
        });
        let attachments: Vec<&str> = row
            .activity
            .attachments
            .iter()
            .map(String::as_str)
            .collect();

        let serializer = IssueActivitySerializer {
            id: row.activity.id,
            actor_detail: Some(&actor),
            issue_detail: issue.as_ref(),
            project_detail: Some(&project),
            workspace_detail: Some(&workspace),
            // `get_source_data` is `hasattr(obj.issue, "source_data")`; the
            // model has no such attribute in this tree, so the field is always
            // null (`plane/app/serializers/issue.py:350-357`).
            source_data: DrfValue::Null,
            created_at: row.activity.created_at,
            updated_at: row.activity.updated_at,
            deleted_at: row.activity.deleted_at,
            verb: &row.activity.verb,
            field: row.activity.field.as_deref(),
            old_value: row.activity.old_value.as_deref(),
            new_value: row.activity.new_value.as_deref(),
            comment: &row.activity.comment,
            attachments: &attachments,
            old_identifier: row.activity.old_identifier,
            new_identifier: row.activity.new_identifier,
            epoch: row.activity.epoch,
            created_by: row.activity.created_by_id,
            updated_by: row.activity.updated_by_id,
            project: row.activity.project_id,
            workspace: row.activity.workspace_id,
            issue: row.activity.issue_id,
            issue_comment: row.activity.issue_comment_id,
            actor: row.activity.actor_id,
        };
        serializer.drf_write(writer, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_by_follows_the_allowlist() {
        assert_eq!(activity_order(None), ("created_at", true));
        assert_eq!(activity_order(Some("")), ("created_at", true));
        assert_eq!(activity_order(Some("created_at")), ("created_at", false));
        assert_eq!(activity_order(Some("-created_at")), ("created_at", true));
        assert_eq!(activity_order(Some("updated_at")), ("updated_at", false));
        assert_eq!(activity_order(Some("-updated_at")), ("updated_at", true));
        assert_eq!(activity_order(Some("--created_at")), ("created_at", true));
        assert_eq!(activity_order(Some("verb")), ("created_at", true));
    }

    #[test]
    fn order_sql_matches_the_captured_django_statements() {
        assert_eq!(
            activity_order_sql("created_at", true),
            "a.\"created_at\" DESC NULLS LAST"
        );
        assert_eq!(
            activity_order_sql("created_at", false),
            "a.\"created_at\" ASC NULLS LAST"
        );
        assert_eq!(
            activity_order_sql("updated_at", false),
            "a.\"updated_at\" ASC NULLS LAST, a.\"created_at\" DESC"
        );
    }
}
