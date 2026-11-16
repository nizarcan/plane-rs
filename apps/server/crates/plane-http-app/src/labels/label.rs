// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.issue.label.LabelViewSet` — the two read actions.
//!
//! Python: `apps/api/plane/app/views/issue/label.py:20-92`.
//!
//! `permission_classes = [ProjectBasePermission]` is the only authorization on
//! both actions (no `@allow_permission` on `list`/`retrieve`), so a project
//! member or guest may read labels; the runtime enforces it.
//!
//! `LabelSerializer` is a plain `BaseSerializer`, not a `DynamicBaseSerializer`
//! (`plane/app/serializers/issue.py:371-396`), so `fields=` and `expand=` are
//! not accepted at all (DRF would raise `TypeError`) and the response is the
//! full field list in declaration order: `parent`, `name`, `color`, `id`,
//! `project_id`, `workspace_id`, `sort_order`.

use plane_core::error::{APPLICATION_JSON, ApiError};
use plane_db::models::label::Label;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::generated::app_issue::LabelSerializer;

use crate::common::{client, lookup, render, render_list};
use crate::labels::queries;

/// `LabelViewSet.list` (`plane/app/views/issue/label.py:24-36`, DRF's mixin).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let member_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;

        let rows = client(ctx)?
            .query(
                queries::LABELS_BY_PROJECT.sql,
                &[&slug, &project_id, &member_id],
            )
            .await?;
        let mut labels = Vec::with_capacity(rows.len());
        for row in &rows {
            labels.push(Label::from_row(row, 0)?);
        }

        let serializers: Vec<LabelSerializer<'_>> = labels.iter().map(serializer_for).collect();
        let body = render_list(
            &serializers,
            &crate::common::render::RequestRender::from_ctx(ctx).render_ctx(),
        )?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `LabelViewSet.retrieve` (`plane/app/views/issue/label.py:26-36`, DRF's
/// `get_object()` over the same queryset).
pub fn retrieve<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let label_id = lookup::uuid_kwarg("pk", ctx.kwarg("pk").ok_or(ViewError::Key)?)?;
        let member_id = ctx
            .user
            .id()
            .ok_or(ViewError::Api(ApiError::NotAuthenticated))?;

        let row = client(ctx)?
            .query_opt(
                queries::LABEL_IN_PROJECT.sql,
                &[&slug, &project_id, &member_id, &label_id],
            )
            .await?;
        let Some(row) = row else {
            // `get_object()`: `Http404("No Label matches the given query.")`.
            return crate::labels::not_found::no_match("Label");
        };
        let label = Label::from_row(&row, 0)?;

        let body = render(ctx, &serializer_for(&label))?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// `LabelSerializer(label)`.
fn serializer_for(label: &Label) -> LabelSerializer<'_> {
    LabelSerializer {
        parent: label.parent_id,
        name: &label.name,
        color: &label.color,
        id: label.id,
        project_id: label.project_id,
        workspace_id: label.workspace_id,
        sort_order: label.sort_order,
    }
}
