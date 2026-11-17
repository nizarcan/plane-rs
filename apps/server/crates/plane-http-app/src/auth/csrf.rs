// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/authentication/views/common.py:29-36` — route 358
//! `GET /auth/get-csrf-token/`.
//!
//! `django.middleware.csrf.get_token(request)` returns the **masked**
//! 64-character token, adopts the request's existing 32-character cookie secret
//! when it is valid, and marks the cookie for refresh (sliding expiry). The
//! `CsrfViewMiddleware.process_response` phase writes the `csrftoken` cookie and
//! `Vary: Cookie`; the handler only updates the shared state, so a malformed
//! request cookie is replaced **once** by the middleware rather than by two
//! writers.

use http::Method;
use plane_core::Object;
use plane_drf::error::ViewError;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};

use super::support;

/// Register route 358.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .api_view("plane.authentication.views.common.CSRFTokenEndpoint")?
        .method(Method::GET, get_csrf_token)?;
    Ok(())
}

/// `CSRFTokenEndpoint.get`: 200 `{"csrf_token": "<64 chars>"}`.
pub fn get_csrf_token<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let csrf = ctx.csrf()?;
        let token = csrf.lock().await.get_token();
        support::json_object(200, &Object::new().field("csrf_token", token))
    })
}
