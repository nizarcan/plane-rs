// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WorkspaceStickyViewSet`'s write actions (`plane/app/views/workspace/
//! sticky.py:32-45`), the Phase 5 pilot.
//!
//! | Route | Python | Handler |
//! | --- | --- | --- |
//! | `POST /api/workspaces/{slug}/stickies/` | `create` (`sticky.py:33-40`) | [`create`] |
//! | `PATCH …/stickies/{pk}/` | `BaseViewSet.partial_update` → `UpdateModelMixin` | [`partial_update`] |
//! | `DELETE …/stickies/{pk}/` | `BaseViewSet.destroy` → `DestroyModelMixin` | [`destroy`] |
//!
//! ## What Django does per action
//!
//! **create** (`WorkspaceStickyViewSet.create`, `sticky.py:33-40`):
//!
//! 1. `allow_permission([ADMIN, MEMBER, GUEST], WORKSPACE)` — the runtime's
//!    `plane-permissions` already enforced it before this handler ran;
//! 2. `Workspace.objects.get(slug=slug)` (404 `{"error": "The required object
//!    does not exist."}` when it misses);
//! 3. `StickySerializer(data=request.data)`; `is_valid()` → 400 with
//!    `serializer.errors` in the field order;
//! 4. `serializer.save(workspace_id=…, owner_id=request.user.id)` →
//!    `Sticky.objects.create` → `Sticky.save()` (H23: strip the HTML; on create
//!    `sort_order = MAX(sort_order in the workspace) + 10000`) and
//!    `BaseModel.save()` (H01: `created_by = user`, `updated_by = NULL`) plus the
//!    `auto_now_add`/`auto_now` timestamps;
//! 5. `Response(serializer.data, 201)` from the in-memory instance.
//!
//! **partial_update**, after `allow_permission(creator=True, model=Sticky)`
//! (again the runtime's check — it fetches the workspace membership and the
//! `created_by` row, and answers 403 for someone else's sticky or a missing
//! one):
//!
//! 1. `get_object()` over `get_queryset()` (workspace slug, `owner = user`,
//!    `deleted_at IS NULL`) → `Http404` → 404 `{"detail": "Not found."}`;
//! 2. `StickySerializer(instance, data, partial=True)` → 400 with the errors;
//! 3. `serializer.save()` → `Sticky.save()` (H23 recomputes
//!    `description_stripped` on every save; `sort_order` is untouched because
//!    `_state.adding` is false) and `BaseModel.save()` (`updated_by = user`,
//!    `updated_at = now`);
//! 4. `Response(serializer.data)` from the in-memory instance.
//!
//! **destroy**, after the same permission check and `get_object()`:
//! `perform_destroy` → `instance.delete()` → `SoftDeleteModel.delete`
//! (`mixins.py:65-77`): `deleted_at = now`, a full `save()` (so H23 and H01 run
//! again), then `soft_delete_related_objects.delay("db", "sticky", pk,
//! using=None)`. `Response(status=204)`.
//!
//! ## Statements
//!
//! | Action | Django | Here |
//! | --- | ---: | ---: |
//! | create | 3 (workspace, `MAX(sort_order)`, INSERT) | 3 |
//! | partial_update | 4 (permission ×2, get_object, UPDATE) | 2 (runtime permission, get_object, UPDATE) |
//! | destroy | 4 + task | 2 + task |

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::hooks::audit::Audit;
use plane_db::{JsonText, hooks};
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_workspace::StickySerializer;
use plane_serializers::{PgJson, RenderCtx};
use uuid::Uuid;

use crate::common::{client, lookup};
use crate::stickies::queries;
use crate::write::{self, Serializer, html};

/// A stored `stickies` row (the columns the serializer and the UPDATE need).
#[derive(Debug, Clone)]
struct Sticky {
    id: Uuid,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    deleted_at: Option<chrono::DateTime<chrono::Utc>>,
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
    fn decode(row: &tokio_postgres::Row) -> Result<Self, ViewError> {
        Ok(Self {
            id: row.try_get(0).map_err(ViewError::from)?,
            created_at: row.try_get(1).map_err(ViewError::from)?,
            updated_at: row.try_get(2).map_err(ViewError::from)?,
            deleted_at: row.try_get(3).map_err(ViewError::from)?,
            name: row.try_get(4).map_err(ViewError::from)?,
            description: row.try_get(5).map_err(ViewError::from)?,
            description_html: row.try_get(6).map_err(ViewError::from)?,
            description_stripped: row.try_get(7).map_err(ViewError::from)?,
            description_binary: row.try_get(8).map_err(ViewError::from)?,
            logo_props: row.try_get(9).map_err(ViewError::from)?,
            color: row.try_get(10).map_err(ViewError::from)?,
            background_color: row.try_get(11).map_err(ViewError::from)?,
            sort_order: row.try_get(12).map_err(ViewError::from)?,
            created_by: row.try_get(13).map_err(ViewError::from)?,
            updated_by: row.try_get(14).map_err(ViewError::from)?,
            workspace: row.try_get(15).map_err(ViewError::from)?,
            owner: row.try_get(16).map_err(ViewError::from)?,
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

/// A body from one instance, in the request's timezone (`serializer.data`).
fn render(ctx: &ViewCtx<'_>, sticky: &Sticky, status: u16) -> Result<ViewResponse, ViewError> {
    let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
    let body = plane_serializers::render(&sticky.value(), &RenderCtx::full(&timezone))?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

/// `StickySerializer`'s writable fields, in DRF's validation order, from the
/// generated table (`plane/app/serializers/workspace.py:346-368` ->
/// `write/generated/sticky_serializer.rs`).
///
/// The table carries the `description_html` null rule and the
/// `created_by`/`updated_by` `PrimaryKeyRelatedField` checks that the old
/// hand-written list missed; `write::fields::primary_key` runs the queryset
/// lookups. Regenerate it after changing `apps/api` (see `HANDLERS.md`).
async fn apply_body(
    serializer: &mut Serializer<'_>,
    client: &deadpool_postgres::Client,
) -> Result<(), ViewError> {
    write::generated::sticky_serializer::validate(serializer, client).await
}

/// `StickySerializer.validate`'s HTML branch
/// (`plane/app/serializers/workspace.py:353-361`): a truthy `description_html`
/// is sanitised with `validate_html_content` and **replaced**; a failure is
/// `ValidationError({"error": "html content is not valid"})`, whose body is a
/// bare string (not a list), and a payload over 10 MB is refused with the same
/// body.
fn sanitized_html(raw: Option<&str>) -> Result<Option<String>, ViewResponse> {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return Ok(raw.map(str::to_owned));
    };
    if raw.len() > 10 * 1024 * 1024 {
        return Err(html_invalid());
    }
    Ok(Some(html::sanitize(raw)))
}

/// `{"error": "html content is not valid"}` with DRF's 400.
fn html_invalid() -> ViewResponse {
    ViewResponse::raw(
        400,
        APPLICATION_JSON,
        plane_query::json::Json::object(vec![(
            "error",
            plane_query::json::Json::string("html content is not valid"),
        )])
        .to_drf(),
    )
}

/// `WorkspaceStickyViewSet.create` (`plane/app/views/workspace/sticky.py:33-40`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let owner = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let client = client(ctx)?;

        // `Workspace.objects.get(slug=slug)` (`sticky.py:34`).
        let workspace = lookup::workspace_by_slug(client, slug)
            .await
            .map_err(ViewError::from)?;

        let mut serializer = Serializer::new(ctx, false)?;
        apply_body(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // Model defaults, exactly the model field defaults
        // (`plane/db/models/sticky.py:16-36`). `description_stripped` is not a
        // default: H23 derives it.
        let description = values
            .json("description")
            .cloned()
            .unwrap_or_else(|| JsonText::new("{}"));
        let description_html = match sanitized_html(values.text("description_html")) {
            Ok(Some(html)) => html,
            Ok(None) => "<p></p>".to_owned(),
            Err(response) => return Ok(response),
        };
        let logo_props = values
            .json("logo_props")
            .cloned()
            .unwrap_or_else(|| JsonText::new("{}"));
        let default_sort_order = values.float("sort_order").unwrap_or(65535.0);

        // H23: `description_stripped` and the create-only `sort_order`.
        let prepared = hooks::sticky::before_create(
            client,
            workspace.id,
            &description_html,
            default_sort_order,
        )
        .await
        .map_err(ViewError::from)?;

        let now = write::now();
        let audit = Audit::on_create(Some(owner));
        let sticky = Sticky {
            id: Uuid::new_v4(),
            created_at: now,
            updated_at: now,
            deleted_at: values.datetime("deleted_at"),
            name: values.text("name").map(str::to_owned),
            description,
            description_html,
            description_stripped: prepared.description_stripped,
            description_binary: None,
            logo_props,
            color: values.text("color").map(str::to_owned),
            background_color: values.text("background_color").map(str::to_owned),
            sort_order: prepared.sort_order,
            created_by: audit.created_by_id,
            updated_by: audit.updated_by_id,
            workspace: workspace.id,
            owner,
        };

        let statement = client
            .prepare_cached(queries::STICKY_INSERT.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &sticky.created_at,
                    &sticky.updated_at,
                    &sticky.created_by,
                    &sticky.updated_by,
                    &sticky.deleted_at,
                    &sticky.id,
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
                ],
            )
            .await
            .map_err(ViewError::from)?;

        render(ctx, &sticky, 201)
    })
}

/// `PATCH …/stickies/{pk}/` (DRF's `UpdateModelMixin.partial_update`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let owner = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)
            .map_err(ViewError::from)?;
        let client = client(ctx)?;

        let mut sticky = get_object(client, slug, owner, pk).await?;

        let mut serializer = Serializer::new(ctx, true)?;
        apply_body(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `ModelSerializer.update`: assign every validated field on the
        // instance, then `save()`.
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
            sticky.description_html = match sanitized_html(Some(description_html)) {
                Ok(Some(html)) => html,
                Ok(None) => String::new(),
                Err(response) => return Ok(response),
            };
        }
        // `description_stripped` is writable on the serializer, but H23
        // recomputes it from `description_html` on every save, so the body's
        // value is overwritten.
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

        // H23 on update: only `description_stripped` changes.
        sticky.description_stripped = hooks::sticky::before_update(&sticky.description_html);
        // H01 on update.
        let now = write::now();
        sticky.updated_at = now;
        sticky.updated_by = Audit::on_update(Some(owner));

        let statement = client
            .prepare_cached(queries::STICKY_UPDATE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
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

        render(ctx, &sticky, 200)
    })
}

/// `DELETE …/stickies/{pk}/` (DRF's `DestroyModelMixin.destroy`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let owner = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)
            .map_err(ViewError::from)?;
        let client = client(ctx)?;

        let sticky = get_object(client, slug, owner, pk).await?;
        let now = write::now();
        write::soft_delete(
            client,
            "stickies",
            "id",
            sticky.id,
            Audit::on_update(Some(owner)),
            now,
        )
        .await?;
        // `soft_delete_related_objects.delay("db", "sticky", pk, using=None)`.
        write::publish_soft_delete_cascade(ctx, "db", "sticky", sticky.id).await?;
        Ok(ViewResponse::empty(204))
    })
}

/// `get_object()`: the row must be live, in the URL workspace and owned by the
/// caller (the queryset's own filters). A miss is DRF's `Http404`.
async fn get_object(
    client: &deadpool_postgres::Client,
    slug: &str,
    owner: Uuid,
    pk: Uuid,
) -> Result<Sticky, ViewError> {
    let statement = client
        .prepare_cached(queries::STICKY_BY_PK.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&slug, &owner, &pk])
        .await
        .map_err(ViewError::from)?;
    match row {
        Some(row) => Sticky::decode(&row),
        None => Err(ViewError::Api(ApiError::NotFound)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The body's 201 keeps the same serializer bytes as a GET.
    #[test]
    fn a_created_row_renders_like_the_read_route() {
        let sticky = Sticky {
            id: Uuid::nil(),
            created_at: chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .expect("date")
                .with_timezone(&chrono::Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .expect("date")
                .with_timezone(&chrono::Utc),
            deleted_at: None,
            name: Some("x".to_owned()),
            description: JsonText::new("{}"),
            description_html: "<p>x</p>".to_owned(),
            description_stripped: Some("x".to_owned()),
            description_binary: None,
            logo_props: JsonText::new("{}"),
            color: None,
            background_color: None,
            sort_order: 65535.0,
            created_by: None,
            updated_by: None,
            workspace: Uuid::nil(),
            owner: Uuid::nil(),
        };
        let body = plane_serializers::render(&sticky.value(), &RenderCtx::full(&chrono_tz::UTC))
            .expect("render");
        let text = String::from_utf8(body).expect("utf-8");
        assert!(text.starts_with(r#"{"id":"00000000-0000-0000-0000-000000000000","#));
        assert!(text.contains(r#""description":"#));
    }
}
