// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! DRF's `get_object()` miss, as a response.
//!
//! `GenericAPIView.get_object()` raises
//! `Http404("No %s matches the given query." % queryset.model._meta.verbose_name)`
//! (`rest_framework/generics.py:87-89`), so the 404 body is **not** the default
//! `{"detail": "Not found."}` that `plane_core::ApiError::NotFound` renders — it
//! names the model, e.g. `{"detail": "No Label matches the given query."}`.
//!
//! `plane_core::ApiError` has no variant that carries a custom 404 detail
//! (`ApiError::PermissionDenied(String)` exists for 403; `NotFound` is a unit
//! variant), so the body is built here. It is a one-key object rendered through
//! `plane_serde`'s DRF dialect, so nothing about key order or escaping is
//! hand-rolled, and the header set is the one `finalize_response` adds to every
//! response of the view — which is why the differential test sees identical
//! headers. Reported to the orchestrator as a `plane-core` gap
//! (`ApiError::NotFoundDetail(String)` would remove this helper).

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::ViewResponse;

/// The `{"detail": …}` body DRF's 404 carries.
#[derive(serde::Serialize)]
struct Detail {
    detail: String,
}

/// `Http404("No <object_name> matches the given query.")`.
///
/// The name is the model's `_meta.object_name` — the class name, CamelCase
/// preserved — not `verbose_name`: `django/shortcuts.py:get_object_or_404` uses
/// the former, which is why Django says `No ProjectMemberInvite matches the
/// given query.`
pub fn no_match(object_name: &str) -> Result<ViewResponse, ViewError> {
    let body = Detail {
        detail: format!("No {object_name} matches the given query."),
    };
    let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &body)?;
    Ok(ViewResponse::raw(404, APPLICATION_JSON, rendered))
}
