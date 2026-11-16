// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `pages` (`deploy/route-groups.yaml`): Pages, page versions and
//! the live-collaboration access check.
//!
//! Routes, from the group's selectors in `deploy/route-groups.yaml`:
//!
//! * `/api/workspaces/*/pages/**`
//! * `/api/workspaces/*/projects/*/pages/**`
//! * `/api/workspaces/*/projects/*/pages-summary/`
//! * `/api/workspaces/*/projects/*/favorite-pages/**`
//!
//! Registered here, one file per Python module under
//! `plane/app/views/page/`:
//!
//! | Rust | Python | `GET` routes |
//! | --- | --- | --- |
//! | [`base`] | `page/base.py` — `PageViewSet` | `pages-summary/`, `pages/`, `pages/{page_id}/` |
//! | [`version`] | `page/version.py` — `PageVersionEndpoint` | `pages/{page_id}/versions/`, `pages/{page_id}/versions/{pk}/` |
//! | [`live`] | `page/live.py` — `PageLiveAccessEndpoint` | `pages/{page_id}/live-access/` |
//!
//! **Not registered:** `PagesDescriptionViewSet.retrieve`
//! (`GET …/pages/{page_id}/description/`, `page/base.py:498-516`). It returns a
//! `StreamingHttpResponse` with `Content-Type: application/octet-stream` and
//! `Content-Disposition: attachment; filename="page_description.bin"`, and
//! Django frames a streaming response with `Transfer-Encoding: chunked` and no
//! `Content-Length` (`django/http/response.py`, `HttpResponseBase`), and
//! `GZipMiddleware` compresses a *streaming* response whatever its length
//! (`django/middleware/gzip.py:52-56`: the `len(content) < 200` shortcut is
//! guarded by `not response.streaming`). `plane-http-app` has no way to say
//! "this response is streaming": `CommonMiddleware`'s phase calls
//! `Resp::set_content_length` unconditionally (`bins/plane-api/src/middleware/
//! common.rs:51`, `bins/plane-api/src/response.rs:139-144`), so the body always
//! carries a `Content-Length` and never `Transfer-Encoding: chunked`, and the
//! gzip phase skips bodies under 200 bytes (`bins/plane-api/src/middleware/
//! gzip.rs:37`) where Django compresses them. The route therefore stays on
//! Django (501) until the pipeline grows a `streaming` flag on
//! `ViewResponse::Raw`/`DrfResponse` that (a) suppresses `Content-Length` so
//! `Resp::into_response` uses its existing chunked body, and (b) makes the gzip
//! phase compress regardless of length. `flate2`, which the handler would need
//! to compress the bytes itself, is not a dependency of this crate either.
//! Everything the route *reads* is one statement
//! (`PagesDescriptionViewSet.retrieve`, `page/base.py:499-505`); only the
//! response framing blocks it.

use http::Method;
use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod live;
pub mod queries;
pub mod version;

/// Register this group's handlers, in the order they were ported.
///
/// `apps/server/HANDLERS.md` has the step list. Return the first
/// [`RegistryError`]: a route that cannot be registered must fail startup
/// rather than fall back to a 501 nobody notices.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry
        .viewset("plane.app.views.page.base.PageViewSet")?
        .action("list", base::list)?
        .action("retrieve", base::retrieve)?
        .action("summary", base::summary)?;
    registry
        .api_view("plane.app.views.page.version.PageVersionEndpoint")?
        .method(Method::GET, version::get)?;
    registry
        .api_view("plane.app.views.page.live.PageLiveAccessEndpoint")?
        .method(Method::GET, live::get)?;
    Ok(())
}
