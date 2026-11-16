// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! DRF's `get_object()` miss, as a response.
//!
//! `GenericAPIView.get_object()` resolves through `get_object_or_404`, which
//! raises `Http404("No %s matches the given query." % queryset.model._meta.object_name)`
//! (`django/shortcuts.py:get_object_or_404`). The name is the model's
//! `object_name` — the **class name**, CamelCase preserved, e.g.
//! `"No ProjectMemberInvite matches the given query."` — not `verbose_name`.
//!
//! `plane_core::ApiError` has no variant carrying a custom 404 detail (403 has
//! `PermissionDenied(String)`, 404 is a unit variant), so the body is built
//! here. It is a one-key object rendered through `plane_serde`'s DRF dialect,
//! so key order and escaping are not hand-rolled, and the response goes through
//! the same `finalize_response` a returned value always does — which is why the
//! differential test sees identical headers. Reported to the orchestrator as a
//! `plane-core` gap (`ApiError::NotFoundDetail(String)` would remove this
//! helper and its copies in `labels` and `projects`).

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::ViewResponse;

/// The `{"detail": …}` body DRF's 404 carries.
#[derive(serde::Serialize)]
struct Detail {
    detail: String,
}

/// `Http404("No <object_name> matches the given query.")`.
pub fn no_match(object_name: &str) -> Result<ViewResponse, ViewError> {
    let body = Detail {
        detail: format!("No {object_name} matches the given query."),
    };
    let rendered = plane_serde::to_vec(plane_serde::Dialect::Drf, &body)?;
    Ok(ViewResponse::raw(404, APPLICATION_JSON, rendered))
}
