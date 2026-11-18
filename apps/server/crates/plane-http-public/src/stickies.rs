// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Workspace stickies on `/api/v1/` (routes 346–349).
//!
//! Python: `plane/api/views/sticky.py:24-113`, a `BaseViewSet`
//! (`api.BaseViewSet`), so it keeps `AnonRateThrottle` (no headers) and the
//! "required object" 404 wording.
//!
//! | Route | Method | Python |
//! | --- | --- | --- |
//! | `ws/stickies/` | `GET` | `list` (`:66-77`) |
//! | same | `POST` | `create` (`:48-54`) |
//! | `ws/stickies/{pk}/` | `GET` | `retrieve` (`:85-87`) |
//! | same | `PUT`/`PATCH`/`DELETE` | inherited update/partial_update/destroy (`:96-113`) |
//! | `…stickies.{format}`, `…stickies/{pk}.{format}` | every bound verb | 500 |
//!
//! The format routes pass `format=` to methods that do not accept it, so the
//! family handler answers 500 (`sticky.py:48,66,85,96,110`); see
//! [`format_error`].
//!
//! ## What is shared with the app surface
//!
//! `create`, `partial_update` and `destroy` are literally the app-surface
//! handlers (`plane_http_app::stickies::write`): the two Python views run the
//! same serializer, the same H23/H01 hooks and the same `Sticky.delete()`
//! cascade, and the only difference between the views is their permission
//! class, which the runtime enforces before the handler. `list` and `retrieve`
//! need v1's own statements because the app list orders `-sort_order` and this
//! one orders `-created_at`; `PUT` (a full update) has no app counterpart.

use chrono::{DateTime, Utc};
use deadpool_postgres::Client;
use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::api_sticky::StickySerializer;
use plane_serializers::{PgJson, RenderCtx};
use uuid::Uuid;

use crate::projects::{fetch_page, page_envelope};

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.api.views.sticky.StickyViewSet")?
        .action("list", list)?
        .action("create", create)?
        .action("retrieve", retrieve)?
        .action("update", update)?
        .action("partial_update", partial_update)?
        .action("destroy", destroy)?;
    Ok(())
}

/// `paginate(default_per_page=20)` (`plane/api/views/sticky.py:72-77`).
const DEFAULT_PER_PAGE: u32 = 20;

/// `TypeError` from the missing `format` parameter, as the family 500.
fn format_error(ctx: &ViewCtx<'_>) -> Option<ViewError> {
    ctx.kwarg("format").map(|format| {
        ViewError::Unknown(format!(
            "TypeError: the view method does not accept the format kwarg ({format:?})"
        ))
    })
}

/// One `stickies` row.
struct Sticky {
    id: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
    name: Option<String>,
    description: JsonText,
    description_html: String,
    description_stripped: Option<String>,
    description_binary: Option<Vec<u8>>,
    logo_props: JsonText,
    color: Option<String>,
    background_color: Option<String>,
    sort_order: f64,
    created_by: Option<Uuid>,
    updated_by: Option<Uuid>,
    workspace: Uuid,
    owner: Uuid,
}

impl Sticky {
    /// [`crate::stickies_queries::STICKY_PAGE`]'s columns.
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.get(0),
            created_at: row.get(1),
            updated_at: row.get(2),
            deleted_at: row.get(3),
            name: row.get(4),
            description: row.get(5),
            description_html: row.get(6),
            description_stripped: row.get(7),
            description_binary: row.get(8),
            logo_props: row.get(9),
            color: row.get(10),
            background_color: row.get(11),
            sort_order: row.get(12),
            created_by: row.get(13),
            updated_by: row.get(14),
            workspace: row.get(15),
            owner: row.get(16),
        })
    }

    fn value(&self) -> StickySerializer<'_> {
        StickySerializer {
            id: self.id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deleted_at: self.deleted_at,
            name: self.name.as_deref(),
            description: Some(PgJson::from(&self.description)),
            description_html: &self.description_html,
            description_stripped: self.description_stripped.as_deref(),
            description_binary: self.description_binary.as_deref(),
            logo_props: Some(PgJson::from(&self.logo_props)),
            color: self.color.as_deref(),
            background_color: self.background_color.as_deref(),
            sort_order: self.sort_order,
            created_by: self.created_by,
            updated_by: self.updated_by,
            workspace: self.workspace,
            owner: self.owner,
        }
    }
}

/// `StickyViewSet.list` (`plane/api/views/sticky.py:66-77`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(error) = format_error(ctx) {
            return Err(error);
        }
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let owner = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = plane_http_app::common::client(ctx)?;

        // `query = request.query_params.get("query", False)` — an empty value
        // is falsy, so `?query=` does not filter (`sticky.py:67-70`).
        let needle = ctx
            .query()
            .get("query")
            .filter(|value| !value.is_empty())
            .map(|value| format!("%{}%", escape_like(value)));

        let (rows, total, window, next, prev) = match &needle {
            Some(needle) => {
                fetch_page(
                    ctx,
                    client,
                    crate::stickies_queries::STICKY_PAGE_FILTERED.sql,
                    &[&slug, &owner, needle],
                    crate::stickies_queries::STICKY_COUNT_FILTERED.sql,
                    &[&slug, &owner, needle],
                    DEFAULT_PER_PAGE,
                    Sticky::decode,
                )
                .await?
            }
            None => {
                fetch_page(
                    ctx,
                    client,
                    crate::stickies_queries::STICKY_PAGE.sql,
                    &[&slug, &owner],
                    crate::stickies_queries::STICKY_COUNT.sql,
                    &[&slug, &owner],
                    DEFAULT_PER_PAGE,
                    Sticky::decode,
                )
                .await?
            }
        };

        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let serializers: Vec<StickySerializer<'_>> = rows.iter().map(Sticky::value).collect();
        let results =
            plane_http_app::common::render_list(&serializers, &RenderCtx::full(&timezone))?;
        Ok(page_envelope(
            total,
            &window,
            &next,
            &prev,
            serializers.len(),
            results,
        ))
    })
}

/// `django/db/backends/utils.py` `prep_for_like_query`.
fn escape_like(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' | '%' | '_' => {
                escaped.push('\\');
                escaped.push(character);
            }
            _ => escaped.push(character),
        }
    }
    escaped
}

/// `StickyViewSet.retrieve` (`plane/api/views/sticky.py:85-87`).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(error) = format_error(ctx) {
            return Err(error);
        }
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let owner = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let client = plane_http_app::common::client(ctx)?;

        let row = client
            .query_opt(
                crate::stickies_queries::STICKY_BY_PK.sql,
                &[&slug, &owner, &pk],
            )
            .await
            .map_err(ViewError::from)?;
        let Some(row) = row else {
            // `get_object()` uses `get_object_or_404`, whose `Http404` message
            // is Django's `"No <model> matches the given query."`
            // (`rest_framework/generics.py`, `django/shortcuts.py:76-84`).
            return Err(ViewError::Api(ApiError::not_found_detail(
                "No Sticky matches the given query.",
            )));
        };
        let sticky = Sticky::decode(&row)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&sticky.value(), &RenderCtx::full(&timezone))
            .map_err(ViewError::from)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `StickyViewSet.create` (`plane/api/views/sticky.py:48-54`) — the app
/// surface's handler, byte for byte.
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    if let Some(error) = format_error(ctx) {
        return Box::pin(async move { Err(error) });
    }
    plane_http_app::stickies::write::create(ctx)
}

/// DRF's `UpdateModelMixin.update` (`sticky.py:89-102` registers `put`; the
/// method is inherited): a full (non-partial) serializer run.
pub fn update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        if let Some(error) = format_error(ctx) {
            return Err(error);
        }
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let owner = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        let client = plane_http_app::common::client(ctx)?;

        let mut sticky = get_object(client, slug, owner, pk).await?;
        let mut serializer = plane_http_app::write::Serializer::new(ctx, false)?;
        plane_http_app::write::generated::api_sticky_serializer::validate(&mut serializer, client)
            .await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };
        if let Some(response) = apply_body(&mut sticky, &values)? {
            return Ok(response);
        }
        let now = plane_http_app::write::now();
        sticky.updated_at = now;
        sticky.updated_by = plane_http_app::write::Audit::on_update(Some(owner));
        sticky.description_stripped =
            plane_db::hooks::sticky::before_update(&sticky.description_html);
        client
            .execute(
                crate::stickies_queries::STICKY_UPDATE.sql,
                &[
                    &sticky.created_at,
                    &sticky.updated_at,
                    &sticky.created_by,
                    &sticky.updated_by,
                    &sticky.deleted_at,
                    &sticky.name,
                    &sticky.description,
                    &sticky.description_html,
                    &sticky.description_stripped,
                    &sticky.description_binary,
                    &sticky.logo_props,
                    &sticky.color,
                    &sticky.background_color,
                    &sticky.workspace,
                    &sticky.owner,
                    &sticky.sort_order,
                    &sticky.id,
                ],
            )
            .await
            .map_err(ViewError::from)?;
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let body = plane_serializers::render(&sticky.value(), &RenderCtx::full(&timezone))
            .map_err(ViewError::from)?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `StickyViewSet.partial_update` (`plane/api/views/sticky.py:96-102`) — the
/// app surface's handler, with the v1 viewset's `get_object()` 404 message.
///
/// The app handler's own `Http404` is DRF's bare `{"detail": "Not found."}`,
/// while `BaseViewSet.get_object()` calls `get_object_or_404` and DRF renders
/// `"No Sticky matches the given query."`. The pre-check makes the miss path
/// identical without forking the write; a hit repeats the lookup once.
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    if let Some(error) = format_error(ctx) {
        return Box::pin(async move { Err(error) });
    }
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let owner = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        get_object(plane_http_app::common::client(ctx)?, slug, owner, pk).await?;
        plane_http_app::stickies::write::partial_update(ctx).await
    })
}

/// `StickyViewSet.destroy` (`plane/api/views/sticky.py:110-113`) — the app
/// surface's handler, with the same `get_object()` 404 pre-check as
/// [`partial_update`].
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    if let Some(error) = format_error(ctx) {
        return Box::pin(async move { Err(error) });
    }
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let owner = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let pk = plane_http_app::common::lookup::uuid_kwarg(
            "pk",
            ctx.kwarg("pk").ok_or(ViewError::Key)?,
        )?;
        get_object(plane_http_app::common::client(ctx)?, slug, owner, pk).await?;
        plane_http_app::stickies::write::destroy(ctx).await
    })
}

/// `get_object()` over the v1 queryset: live, in the URL workspace, owned by
/// the caller. A miss is DRF's `Http404`.
async fn get_object(
    client: &Client,
    slug: &str,
    owner: Uuid,
    pk: Uuid,
) -> Result<Sticky, ViewError> {
    let row = client
        .query_opt(
            crate::stickies_queries::STICKY_BY_PK.sql,
            &[&slug, &owner, &pk],
        )
        .await
        .map_err(ViewError::from)?;
    match row {
        Some(row) => Sticky::decode(&row),
        None => Err(ViewError::Api(ApiError::not_found_detail(
            "No Sticky matches the given query.",
        ))),
    }
}

/// `StickySerializer.validate`'s two branches (`api/serializers/sticky.py:18-32`).
fn apply_body(
    sticky: &mut Sticky,
    values: &plane_http_app::write::Validated,
) -> Result<Option<ViewResponse>, ViewError> {
    if values.contains("deleted_at") {
        sticky.deleted_at = values.datetime("deleted_at");
    }
    if let Some(name) = values.text("name") {
        sticky.name = Some(name.to_owned());
    }
    if let Some(description) = values.json("description") {
        sticky.description = description.clone();
    }
    if let Some(description_html) = values.text("description_html") {
        sticky.description_html = match sanitize_html(Some(description_html))? {
            Some(html) => html,
            None => {
                // `{"error": "html content is not valid"}` with DRF's 400
                // (`plane/app/serializers/workspace.py:353-361`).
                let body = plane_query::Json::object(vec![(
                    "error",
                    plane_query::Json::string("html content is not valid"),
                )]);
                return Ok(Some(ViewResponse::raw(
                    400,
                    APPLICATION_JSON,
                    body.to_drf(),
                )));
            }
        };
    }
    if let Some(logo_props) = values.json("logo_props") {
        sticky.logo_props = logo_props.clone();
    }
    if values.contains("color") {
        sticky.color = values.text("color").map(str::to_owned);
    }
    if values.contains("background_color") {
        sticky.background_color = values.text("background_color").map(str::to_owned);
    }
    if let Some(sort_order) = values.float("sort_order") {
        sticky.sort_order = sort_order;
    }
    if values.contains("created_by") {
        sticky.created_by = values.uuid("created_by");
    }
    Ok(None)
}

/// `validate_html_content` (`plane/utils/content_validator.py`) for a truthy
/// value: `ammonia`-style sanitisation, and a 400 with a bare-string body on
/// failure (ported from `plane_http_app::stickies::write`).
fn sanitize_html(raw: Option<&str>) -> Result<Option<String>, ViewError> {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return Ok(raw.map(str::to_owned));
    };
    if raw.len() > 10 * 1024 * 1024 {
        return Err(ViewError::Unknown("html content is not valid".to_string()));
    }
    Ok(Some(plane_http_app::write::html::sanitize(raw)))
}
