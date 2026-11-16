// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The intake writes — set A of `plans/INTAKE.md` §3 (routes 53, 54, 57, 58).
//!
//! | Route | Action | Python |
//! | --- | --- | --- |
//! | `POST …/intakes/` (53) | [`create`] | `plane/app/views/intake/base.py:79-81` + `mixins.CreateModelMixin` |
//! | `POST …/inboxes/` (57) | [`create`] | the same view |
//! | `DELETE …/intakes/{pk}/` (54) | [`destroy`] | `plane/app/views/intake/base.py:83-93` |
//! | `DELETE …/inboxes/{pk}/` (58) | [`destroy`] | the same view |
//! | `PATCH` on 54/58 | [`super::render::guard_only`] | `plane/app/views/base.py:103-198` (the guard's 405) |
//!
//! The view never looks at the path, so one handler serves both aliases.
//!
//! ## 53/57 create never writes
//!
//! `@allow_permission` is on `perform_create` (`base.py:79-81`), not on
//! `create`, so DRF's `CreateModelMixin.create` runs its own
//! `is_valid(raise_exception=True)` and *then* calls the decorated
//! `perform_create(serializer)`. The decorator is `_wrapped_view(instance,
//! request, *args, **kwargs)` (`plane/app/permissions/base.py:22`), and DRF
//! passes the serializer positionally (`rest_framework/mixins.py:17-21`), so
//! `request` is an `IntakeSerializer` and the first role query's
//! `request.user` raises `AttributeError`. The family handler answers
//! `500 {"error": "Something went wrong please try again later"}`
//! (`plane/app/views/base.py:231-239`): no row, no task — and **no role
//! check**, because the decorator died before it evaluated one.
//!
//! Before that failure the serializer's own validation runs, so an invalid
//! body is DRF's 400 with `serializer.errors`, and a body whose only faults
//! are valid DRF values (for example a `created_by` that exists) reaches the
//! 500. `IntakeSerializer` is a plain `ModelSerializer`, so the generated
//! table [`crate::write::generated::intake_serializer`] is the whole
//! validation: field order, `required` and the two `created_by`/`updated_by`
//! primary-key lookups.
//!
//! `PATCH` on 54/58 is the `BaseViewSet` guard's 405: `partial_update` is
//! served by `rest_framework.mixins.UpdateModelMixin` and nothing authorizes
//! it (`routes.json` `viewset_guard.refused_methods`), so the handler is never
//! reached. Registering [`super::render::guard_only`] is what makes the action
//! exist for dispatch.
//!
//! ## 54/58 destroy
//!
//! ```python
//! intake = Intake.objects.filter(workspace__slug=slug, project_id=project_id, pk=pk).first()
//! if intake.is_default:                       # `.first()` → None → AttributeError → 500
//!     return Response({"error": "You cannot delete the default intake"}, 400)
//! intake.delete()                             # H03 + the cascade task
//! return Response(status=204)
//! ```
//!
//! `Intake.objects` is the default `SoftDeletionManager`
//! (`plane/db/mixins.py:56-58`), so a soft-deleted intake is `None` like a
//! wrong pk — the 500, not a 404. The delete is H03
//! (`plane/db/mixins.py:72-78`): `deleted_at = now`, a full save (H01's
//! `updated_by`), then `soft_delete_related_objects.delay("db", "intake", pk)`.
//! `Intake` has no `save()` override; `ProjectBaseModel.save`'s
//! `workspace = project.workspace` cannot change a stored value.

use deadpool_postgres::Client;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use uuid::Uuid;

use super::write_intake_queries as queries_write;
use crate::issues::lists::list_common::{client, kwarg_text, kwarg_uuid, user_id};
use crate::write;
use crate::write::Serializer;
use crate::write::generated::intake_serializer;

/// Register set A's handlers on `IntakeViewSet`, in route order.
///
/// `list`/`retrieve` are registered by [`super::base::register`]; the registry
/// merges both blocks into one view.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.intake.base.IntakeViewSet")?
        // 53/57.
        .action("create", create)?
        // 54/58's PATCH, refused by the `BaseViewSet` guard before dispatch.
        .action("partial_update", super::render::guard_only)?
        // 54/58.
        .action("destroy", destroy)?;
    Ok(())
}

/// `IntakeViewSet.create` — the `CreateModelMixin` run that always ends in the
/// decorator's `AttributeError` (`plane/app/views/intake/base.py:79-81`).
pub fn create<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;

        // `serializer = self.get_serializer(data=request.data)` then
        // `serializer.is_valid(raise_exception=True)`
        // (`rest_framework/mixins.py:13-16`). `IntakeSerializer` has no
        // `validate`/`validate_*`, so the generated table is its whole
        // validation.
        let mut serializer = Serializer::new(ctx, false)?;
        intake_serializer::validate(&mut serializer, client).await?;
        match serializer.finish() {
            Err(errors) => Ok(errors.into_response()),
            // A valid body reaches `self.perform_create(serializer)`. The
            // decorated `perform_create` (`base.py:79-81`) receives the
            // serializer as `request` and dies reading `request.user` in
            // `ProjectMember.objects.filter(member=request.user, …)`
            // (`plane/app/permissions/base.py:68-74`).
            Ok(_) => Err(ViewError::Unknown(
                "AttributeError: 'IntakeSerializer' object has no attribute 'user'".to_owned(),
            )),
        }
    })
}

/// `IntakeViewSet.destroy` (`plane/app/views/intake/base.py:83-93`).
pub fn destroy<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = kwarg_text(ctx, "slug")?;
        let project_id = kwarg_uuid(ctx, "project_id")?;
        let pk = kwarg_uuid(ctx, "pk")?;
        let user = user_id(ctx)?;
        let client = client(ctx)?;

        // `Intake.objects.filter(...).first()`: the soft-deletion manager,
        // ordered by `Intake.Meta.ordering = ("name",)`
        // (`plane/db/models/intake.py:39`).
        let is_default = load_intake(client, slug, project_id, pk).await?;
        let Some(is_default) = is_default else {
            // `intake.is_default` on `None`.
            return Err(ViewError::Unknown(
                "AttributeError: 'NoneType' object has no attribute 'is_default'".to_owned(),
            ));
        };

        if is_default {
            return Ok(write::error_body(
                400,
                "You cannot delete the default intake",
            ));
        }

        // `intake.delete()` → H03 (`plane/db/mixins.py:72-78`).
        let now = write::now();
        write::soft_delete(client, "intakes", "id", pk, Some(user), now).await?;
        write::publish_soft_delete_cascade(ctx, "db", "intake", pk).await?;

        Ok(ViewResponse::empty(204))
    })
}

/// `Intake.objects.filter(workspace__slug=…, project_id=…, pk=…).first()` —
/// `Ok(None)` is `.first()` on no match; more than one row is
/// `MultipleObjectsReturned` (`pk` is unique, so unreachable).
async fn load_intake(
    client: &Client,
    slug: &str,
    project_id: Uuid,
    pk: Uuid,
) -> Result<Option<bool>, ViewError> {
    let statement = client
        .prepare_cached(queries_write::INTAKE_FOR_DESTROY.sql)
        .await
        .map_err(ViewError::from)?;
    let rows = client
        .query(&statement, &[&slug, &project_id, &pk])
        .await
        .map_err(ViewError::from)?;
    match rows.as_slice() {
        [] => Ok(None),
        [row] => Ok(Some(row.try_get(0).map_err(ViewError::from)?)),
        _ => Err(ViewError::Unknown(
            "Intake.MultipleObjectsReturned: the first() matched more than one row".to_owned(),
        )),
    }
}
