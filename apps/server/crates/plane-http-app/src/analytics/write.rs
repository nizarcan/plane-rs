// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The stored-view writes (`plane/app/views/analytic/base.py`), routes 1 and 2.
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/analytic-view/` | 1 [`create`] | `AnalyticViewViewset.perform_create` (`base.py:188-190`) |
//! | `PATCH …/analytic-view/{pk}/` | 2 [`partial_update`] | DRF `UpdateModelMixin.partial_update` + `AnalyticViewSerializer.update` |
//! | `DELETE …/analytic-view/{pk}/` | 2 [`destroy`] | DRF `DestroyModelMixin.destroy` + H03 |
//!
//! `AnalyticViewViewset` is `WorkSpaceAdminPermission` — ADMIN **or MEMBER**
//! despite the name (`plane/app/permissions/workspace.py:61-71`) — and the
//! runtime's permission layer already ran it, so a member may create, patch
//! and delete a stored view while a guest gets the 403 for any pk, including a
//! missing one.
//!
//! ## Statement order (plan §4)
//!
//! * create: validate (400) → `Workspace.objects.get(slug=…)` (404) →
//!   `issue_filters(query_dict, "POST")` → INSERT → 201. Validation runs
//!   **before** the workspace lookup, unlike the webhook routes.
//! * partial_update: `get_object()` (404) → validate (400) → `update()` → full
//!   save → 200.
//! * destroy: `get_object()` (404) → H03's soft delete → the cascade publish →
//!   204.
//!
//! No transaction: `ATOMIC_REQUESTS` is unset and none of the three actions
//! opens one (`HANDLERS.md` §4).
//!
//! ## Upstream quirks kept
//!
//! * `deleted_at` is writable (`plane/app/serializers/analytic.py:12-14` omits
//!   it from `read_only_fields`), so a create can insert an already-soft-deleted
//!   view that this response echoes and no read ever returns.
//! * `update()` reads `validated_data.get("query_data", {})` — a typo for
//!   `query_dict` (`serializers/analytic.py:25`), so a PATCH **always** stores
//!   `query = issue_filters({}, "PATCH") == {}` and the double assignment at
//!   `:27,:30` is dead code. A `query_dict` in the PATCH body is stored
//!   verbatim and never recompiled.
//! * `created_by` is not read-only, so a PATCH body may change it (including to
//!   null); H01 then overwrites `updated_by` with the caller
//!   (`plane/db/models/base.py:23-45`).
//! * `created_at`/`updated_at` are two `write::now()` calls on create:
//!   Django's `auto_now_add` and `auto_now` call `timezone.now()` per field.
//!
//! The `get_object()` 404 is DRF's `Http404` → `{"detail": "No AnalyticView
//! matches the given query."}`, the same body `retrieve` answers
//! (`analytics/base.rs`'s `not_found`); the create's missing workspace is a
//! `DoesNotExist` → `{"error": "The required object does not exist."}`
//! (`plane/app/views/base.py:202-208`).

use chrono_tz::Tz;
use deadpool_postgres::Client;
use plane_core::error::APPLICATION_JSON;
use plane_db::hooks::audit::Audit;
use plane_db::models::analytic::AnalyticView;
use plane_db::{JsonText, hooks};
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;
use uuid::Uuid;

use crate::analytics::base::{not_found, serializer_for};
use crate::analytics::write_queries as queries;
use crate::common::{client, jobs, lookup};
use crate::write;
use crate::write::Serializer;

/// The model default of `query`/`query_dict` when the body carries no value:
/// `{}`.
fn empty_object() -> JsonText {
    JsonText::new("{}")
}

/// `json.loads(self.query_dict)` so `issue_filters` can walk it as the Python
/// object the serializer validated. The text keeps the body's key order, which
/// is what the create response renders (the column is `jsonb` and re-orders,
/// the in-memory value is not).
fn query_dict_json(text: &JsonText) -> Result<Json, ViewError> {
    plane_serde::pyjson::loads(text.as_str().as_bytes(), "utf-8")
        .map_err(|error| ViewError::Unknown(format!("json.loads({text:?}): {error}")))
}

/// `AnalyticViewSerializer.create`'s `query` (`plane/app/serializers/
/// analytic.py:16-22`): `issue_filters(query_dict, "POST")` when the dict is
/// truthy, else `{}`.
///
/// `issue_filters` is [`hooks::issue_view::query_from_filters`]; a shape it
/// raises on (`{"state": 5}` → `len(int)`, a string body → `.get`) is the
/// family 500, as in Python (`hooks/issue_view.rs:53-63`).
fn query_from(query_dict: &Json) -> Result<JsonText, ViewError> {
    if !hooks::issue_view::truthy(query_dict) {
        return Ok(empty_object());
    }
    hooks::issue_view::query_from_filters(query_dict, chrono::Utc::now().date_naive())
        .map_err(|error| ViewError::Unknown(error.to_string()))
}

/// `Response(serializer.data, status=…)` from the **in-memory** instance.
fn render(view: &AnalyticView, timezone: Tz, status: u16) -> Result<ViewResponse, ViewError> {
    let body = plane_serializers::render(
        &serializer_for(view),
        &plane_serializers::RenderCtx::full(&timezone),
    )?;
    Ok(ViewResponse::raw(status, APPLICATION_JSON, body))
}

fn timezone(ctx: &ViewCtx<'_>) -> Tz {
    ctx.timezone.copied().unwrap_or(chrono_tz::UTC)
}

/// `get_object()`: `AnalyticView.objects.get(pk=pk, workspace__slug=slug)`.
///
/// `None` is DRF's `Http404("No AnalyticView matches the given query.")`, which
/// [`not_found`] renders.
async fn get_object(
    client: &Client,
    slug: &str,
    pk: Uuid,
) -> Result<Option<AnalyticView>, ViewError> {
    let statement = client
        .prepare_cached(queries::ANALYTIC_VIEW_BY_PK.sql)
        .await
        .map_err(ViewError::from)?;
    let row = client
        .query_opt(&statement, &[&slug, &pk])
        .await
        .map_err(ViewError::from)?;
    match row {
        Some(row) => Ok(Some(AnalyticView::from_row(&row, 0)?)),
        None => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Route 1 — create
// ---------------------------------------------------------------------------

/// `POST …/analytic-view/` (DRF's `CreateModelMixin.create` →
/// `AnalyticViewViewset.perform_create`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = jobs::user_id(ctx)?;
        let client = client(ctx)?;
        let timezone = timezone(ctx);

        // 1. `AnalyticViewSerializer(data=request.data).is_valid()`.
        let mut serializer = Serializer::new(ctx, false)?;
        write::generated::analytic_view_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // 2. `perform_create`: `Workspace.objects.get(slug=slug)`.
        let workspace = lookup::workspace_by_slug(client, slug)
            .await
            .map_err(ViewError::from)?;

        // 3. `AnalyticViewSerializer.create`'s `query`.
        let query_dict = values
            .json("query_dict")
            .cloned()
            .unwrap_or_else(empty_object);
        let query = query_from(&query_dict_json(&query_dict)?)?;

        // 4. `AnalyticView.objects.create(**validated_data, workspace_id=…)`.
        // `auto_now_add` then `auto_now` — two `timezone.now()` calls.
        let created_at = write::now();
        let updated_at = write::now();
        let audit = Audit::on_create(Some(user));
        let view = AnalyticView {
            id: Uuid::new_v4(),
            created_at,
            updated_at,
            deleted_at: values.datetime("deleted_at"),
            name: values
                .text("name")
                .ok_or_else(|| ViewError::Unknown("validated name vanished".to_owned()))?
                .to_owned(),
            description: values.text("description").unwrap_or_default().to_owned(),
            query,
            query_dict,
            created_by_id: audit.created_by_id,
            updated_by_id: audit.updated_by_id,
            workspace_id: workspace.id,
        };

        let statement = client
            .prepare_cached(queries::ANALYTIC_VIEW_INSERT.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &view.created_at,
                    &view.updated_at,
                    &view.created_by_id,
                    &view.updated_by_id,
                    &view.deleted_at,
                    &view.id,
                    &view.workspace_id,
                    &view.name,
                    &view.description,
                    &view.query,
                    &view.query_dict,
                ],
            )
            .await
            .map_err(ViewError::from)?;

        render(&view, timezone, 201)
    })
}

// ---------------------------------------------------------------------------
// Route 2 — partial_update
// ---------------------------------------------------------------------------

/// `PATCH …/analytic-view/{pk}/` (DRF's `UpdateModelMixin.partial_update`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)
            .map_err(ViewError::from)?;
        let user = jobs::user_id(ctx)?;
        let client = client(ctx)?;
        let timezone = timezone(ctx);

        // `get_object()` runs before the body is parsed and validated.
        let Some(mut view) = get_object(client, slug, pk).await? else {
            return not_found("AnalyticView");
        };

        let mut serializer = Serializer::new(ctx, true)?;
        write::generated::analytic_view_serializer::validate(&mut serializer, client).await?;
        let values = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `AnalyticViewSerializer.update`: the `query_data` typo means
        // `query_params` is always `{}`, so `query` is always `{}` (and the
        // `if` above it at `:27` never runs).
        view.query = empty_object();

        // `ModelSerializer.update` assigns every validated field on the
        // instance, then `instance.save()` writes all of them.
        if values.contains("deleted_at") {
            view.deleted_at = values.datetime("deleted_at");
        }
        if let Some(name) = values.text("name") {
            view.name = name.to_owned();
        }
        if let Some(description) = values.text("description") {
            view.description = description.to_owned();
        }
        if let Some(query_dict) = values.json("query_dict") {
            view.query_dict = query_dict.clone();
        }
        // `created_by` is writable, so a body value (or null) is applied; H01
        // only overwrites `updated_by` on an update.
        if values.contains("created_by") {
            view.created_by_id = values.uuid("created_by");
        }

        // H01's update half plus `auto_now`'s `updated_at`.
        view.updated_at = write::now();
        view.updated_by_id = Audit::on_update(Some(user));

        let statement = client
            .prepare_cached(queries::ANALYTIC_VIEW_UPDATE.sql)
            .await
            .map_err(ViewError::from)?;
        client
            .execute(
                &statement,
                &[
                    &view.created_at,
                    &view.updated_at,
                    &view.created_by_id,
                    &view.updated_by_id,
                    &view.deleted_at,
                    &view.workspace_id,
                    &view.name,
                    &view.description,
                    &view.query,
                    &view.query_dict,
                    &view.id,
                ],
            )
            .await
            .map_err(ViewError::from)?;

        render(&view, timezone, 200)
    })
}

// ---------------------------------------------------------------------------
// Route 2 — destroy
// ---------------------------------------------------------------------------

/// `DELETE …/analytic-view/{pk}/` (DRF's `DestroyModelMixin.destroy`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let pk = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)
            .map_err(ViewError::from)?;
        let user = jobs::user_id(ctx)?;
        let client = client(ctx)?;

        let Some(view) = get_object(client, slug, pk).await? else {
            return not_found("AnalyticView");
        };

        // H03: `deleted_at = now`, then `self.save()`'s `updated_at`/`updated_by`.
        let now = write::now();
        write::soft_delete(
            client,
            AnalyticView::TABLE,
            "id",
            view.id,
            Audit::on_update(Some(user)),
            now,
        )
        .await?;
        // `soft_delete_related_objects.delay("db", "analyticview", pk, using=None)`.
        write::publish_soft_delete_cascade(ctx, "db", "analyticview", view.id).await?;
        Ok(ViewResponse::empty(204))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> Json {
        plane_serde::pyjson::loads(raw.as_bytes(), "utf-8").expect("json")
    }

    #[test]
    fn a_truthy_query_dict_is_compiled_and_an_empty_one_is_not() {
        // `issue_filters({"priority": "urgent"}, "POST")`.
        let compiled = query_from(&parse(r#"{"priority": "urgent"}"#)).expect("query");
        assert_eq!(compiled.as_str(), r#"{"priority__in": "urgent"}"#);
        // The model's `{}` for every falsy shape.
        for raw in ["{}", "[]", "false", "0", "\"\"", "null"] {
            let query = query_from(&parse(raw)).expect("query");
            assert_eq!(query.as_str(), "{}", "{raw} is falsy in Python");
        }
    }

    #[test]
    fn a_query_dict_python_chokes_on_is_an_error() {
        // `filter_state` runs `len(params.get("state"))` → `len(int)`.
        assert!(query_from(&parse(r#"{"state": 5}"#)).is_err());
    }
}
