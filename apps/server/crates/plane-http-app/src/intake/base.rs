// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane.app.views.intake.base.IntakeViewSet.list` — the single read action.
//!
//! `GET /api/workspaces/{slug}/projects/{project_id}/intakes/` and
//! `GET …/inboxes/` are the same route object under two paths and both answer
//! `IntakeSerializer(self.get_queryset().first())`
//! (`plane/app/views/intake/base.py:74-77`).
//!
//! Three details are behaviour:
//!
//! * `.first()` is ordered by `Intake.Meta.ordering = ("name",)`
//!   (`plane/db/models/intake.py:33`), not by creation time;
//! * a project with no intake renders an object of **initial values**, not
//!   `{}` — see [`super::render::empty_intake`];
//! * `pending_issue_count` counts soft-deleted pending intake issues too, since
//!   the `Count` filter only names `status` (`base.py:70`).
//!
//! The `GET …/{pk}/` route on the same view is refused by `BaseViewSet`'s
//! guard (`routes.json` `refused_methods`), so it registers
//! [`super::render::guard_only`] and never reaches a serializer.

use plane_db::models::intake::Intake;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serializers::FieldValue;
use plane_serializers::PgJson;
use plane_serializers::generated::app_intake::IntakeSerializer;
use plane_serializers::generated::app_project::ProjectLiteSerializer;
use uuid::Uuid;

use super::asset;
use super::queries;
use super::render::{empty_intake, plain_context};
use crate::issues::lists::list_common::{client, kwarg_text, kwarg_uuid};

/// Register the view's two read actions (the second is the guard's 405).
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.intake.base.IntakeViewSet")?
        .action("list", list)?
        .action("retrieve", super::render::guard_only)?;
    Ok(())
}

/// `IntakeViewSet.list` (`plane/app/views/intake/base.py:74-77`).
pub fn list<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let client = client(ctx)?;

        let statement = client
            .prepare_cached(queries::INTAKE_BY_PROJECT.sql)
            .await
            .map_err(ViewError::from)?;
        let Some(row) = client
            .query_opt(&statement, &[&project_id, &slug])
            .await
            .map_err(ViewError::from)?
        else {
            return empty_intake();
        };

        let intake = Intake::from_row(&row, 0)?;
        let pending_issue_count: i64 = row.get(Intake::COLUMN_COUNT);

        // `ProjectLiteSerializer(project)` (`plane/app/serializers/project.py:113`)
        // and its `cover_image_url` property.
        let project_id: Uuid = row.get(Intake::COLUMN_COUNT + 1);
        let cover_image: Option<String> = row.get(Intake::COLUMN_COUNT + 4);
        let cover_image_asset_id: Option<Uuid> = row.get(Intake::COLUMN_COUNT + 5);
        let mut asset_ids = std::collections::HashSet::new();
        if let Some(id) = cover_image_asset_id {
            asset_ids.insert(id);
        }
        let asset_urls = asset::load_asset_urls(client, &asset_ids).await?;
        let cover_image_url = asset::asset_or_text(
            cover_image_asset_id,
            cover_image.clone().unwrap_or_default(),
            &asset_urls,
        )?;

        let identifier: String = row.get(Intake::COLUMN_COUNT + 2);
        let name: String = row.get(Intake::COLUMN_COUNT + 3);
        let logo_props: plane_db::JsonText = row.get(Intake::COLUMN_COUNT + 6);
        let description: String = row.get(Intake::COLUMN_COUNT + 7);

        let project_detail = ProjectLiteSerializer {
            id: project_id,
            identifier: &identifier,
            name: &name,
            cover_image: cover_image.as_deref(),
            cover_image_url: cover_image_url.as_deref(),
            logo_props: Some(PgJson::from(&logo_props)),
            description: &description,
        };

        let serializer = IntakeSerializer {
            id: intake.id,
            project_detail: Some(&project_detail),
            // The annotation exists: `Count(..., filter=Q(status=-2))`.
            pending_issue_count: FieldValue::value(pending_issue_count),
            created_at: intake.created_at,
            updated_at: intake.updated_at,
            deleted_at: intake.deleted_at,
            name: &intake.name,
            description: &intake.description,
            is_default: intake.is_default,
            view_props: Some(PgJson::from(&intake.view_props)),
            logo_props: Some(PgJson::from(&intake.logo_props)),
            created_by: intake.created_by_id,
            updated_by: intake.updated_by_id,
            project: intake.project_id,
            workspace: intake.workspace_id,
        };
        let context = plain_context(ctx);
        Ok(ViewResponse::raw(
            200,
            plane_core::error::APPLICATION_JSON,
            plane_serializers::render(&serializer, &context)?,
        ))
    })
}
