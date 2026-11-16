// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Helpers every `/api/` domain repeats.
//!
//! Nothing here may grow a domain-specific rule: a shape that only one group
//! needs belongs in that group's directory. The rule for adding to this module
//! is the one `apps/server/HANDLERS.md` states — a second copy of it in a
//! second domain is the trigger, not an anticipated need.
//!
//! | Module | What it removes from every handler |
//! | --- | --- |
//! | [`mod@render`] | building a `RenderCtx` from `fields=`, `expand=` and the user's timezone |
//! | [`lookup`] | `Workspace.objects.get(slug=…)` / `Project.objects.get(pk=…, workspace__slug=…)` |
//! | [`queries`] | the catalog entries those lookups need (`schema_check`ed) |
//! | [`cache`] | `@cache_response` / `@invalidate_cache`: the request half of `plane/utils/cache.py` |
//! | [`jobs`] | publishing a Celery task (`task.delay`) and the `recent_visited_task` call-site shapes |
//! | [`request`] | `request.build_absolute_uri()`, the value a page-cache key is built from |

pub mod cache;
pub mod jobs;
pub mod lookup;
pub mod queries;
pub mod render;
pub mod request;

use deadpool_postgres::Client;
use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;

pub use render::{RequestRender, render, render_list};

/// The connection Django would have used for this request.
///
/// For a DRF route the runtime has already checked one out with the
/// read-replica rule applied (`ViewCtx::db`), so this is a borrow, not a query.
/// A handler that needs a *second* connection — because it writes while the
/// request is a read, or pipelines independent queries — checks out
/// `ctx.pools.write_pool()` itself.
///
/// Django opens a connection on the first query and keeps it for the request
/// (`django/db/backends/base/base.py`, `ensure_connection`); one connection per
/// request is therefore the shape to preserve.
pub fn client<'a>(ctx: &'a ViewCtx<'a>) -> Result<&'a Client, ViewError> {
    ctx.db.ok_or_else(|| {
        // Unreachable for a registered DRF route: `dispatch` fills `db` before
        // calling the handler. A plain Django view gets none.
        ViewError::Unknown("no database connection for this request".to_string())
    })
}
