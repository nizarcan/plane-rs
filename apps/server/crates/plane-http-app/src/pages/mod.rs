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
//! | [`base`] | `page/base.py` — `PageViewSet`, `PagesDescriptionViewSet` | `pages-summary/`, `pages/`, `pages/{page_id}/`, `pages/{page_id}/description/` |
//! | [`version`] | `page/version.py` — `PageVersionEndpoint` | `pages/{page_id}/versions/`, `pages/{page_id}/versions/{pk}/` |
//! | [`live`] | `page/live.py` — `PageLiveAccessEndpoint` | `pages/{page_id}/live-access/` |
//!
//! The description route is the one response in `/api/` that is a
//! `StreamingHttpResponse`. `ViewResponse::streaming_raw` carries
//! `HttpResponseBase.streaming` through `plane-drf`; `bins/plane-api` then skips
//! `Content-Length` and frames the body with `Transfer-Encoding: chunked`, and
//! `GZipMiddleware` compresses it regardless of length, exactly as Django does
//! (`django/middleware/gzip.py:27-30,52-67`). `crates/plane-testkit` decodes the
//! gzip body before comparing, so the random `FNAME` bytes Django writes
//! (`django/utils/text.py:347-363`) are not part of the contract.

use http::Method;
use plane_drf::registry::{Registry, RegistryError};

pub mod base;
pub mod live;
pub mod queries;
pub mod version;
pub mod write_duplicate;
pub mod write_duplicate_queries;

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
        .viewset("plane.app.views.page.base.PagesDescriptionViewSet")?
        .action("retrieve", base::description)?;
    registry
        .api_view("plane.app.views.page.version.PageVersionEndpoint")?
        .method(Method::GET, version::get)?;
    registry
        .api_view("plane.app.views.page.live.PageLiveAccessEndpoint")?
        .method(Method::GET, live::get)?;
    // Set D: duplicate (route 133).
    registry
        .api_view("plane.app.views.page.base.PageDuplicateEndpoint")?
        .method(Method::POST, write_duplicate::post)?;
    Ok(())
}
