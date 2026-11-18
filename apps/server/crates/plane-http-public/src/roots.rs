// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The v1 api-roots (routes 344/345, and the shadowed 350/351).
//!
//! Python: `rest_framework.routers.APIRootView.get`
//! (`rest_framework/routers.py:314-341`) mounted by `DefaultRouter`'s root
//! view. `plane/api/urls/__init__.py:26-31` includes the invitations router
//! before the stickies router, so **both** `workspaces/<slug>/` roots exist
//! and the first match wins: the invitations one. Rows 350/351 are the
//! unreachable stickies copy (`ROUTES.md:400-407`) and dispatch to the same
//! view, which is exactly what Django does for the first registration.
//!
//! The body is `{"invitations": "<absolute url>"}`:
//!
//! * the key is the router's only registry entry, `invitations`;
//! * the value is `reverse("workspace-invitations-list", kwargs={"slug": …},
//!   request=request, format=kwargs.get("format"))`. Without a format suffix
//!   that is `http://<host>/api/v1/workspaces/<slug>/invitations/`; with
//!   `.{format}` it is the `.format` variant of the same list URL.
//!
//! The view is a `drf.APIView` with the default `SessionAuthentication`, so an
//! API key is not accepted here (the runtime's policy enforces it) and the
//! `AnonRateThrottle` no-op emits no headers.

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("rest_framework.routers.APIRootView")?
        .method(http::Method::GET, get)?;
    Ok(())
}

/// `APIRootView.get` (`rest_framework/routers.py:322-341`), with the
/// invitations router's single `api_root_dict` entry.
pub fn get<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let location = match ctx.kwarg("format") {
            Some(format) => format!("/api/v1/workspaces/{slug}/invitations.{format}"),
            None => format!("/api/v1/workspaces/{slug}/invitations/"),
        };
        let url = plane_http_app::common::request::build_absolute_uri(ctx, &location);
        let body = plane_query::Json::Object(vec![(
            "invitations".to_string(),
            plane_query::Json::string(url),
        )])
        .to_drf();
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}
