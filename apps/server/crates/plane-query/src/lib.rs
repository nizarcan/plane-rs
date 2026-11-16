// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-query`: the SQL composition, filters, ordering, grouping and
//! pagination of Plane's issue read path.
//!
//! The Python is the specification. Each module names the file it ports and
//! the lines that decide behaviour; the test corpus under `tests/golden/` is
//! generated from Django on a deterministic fixture and replayed by
//! `tests/golden.rs`.
//!
//! | Module | Python |
//! | --- | --- |
//! | [`sql`] | — (the composer; see its docs for why it is hand-written) |
//! | [`issue::filter`] | `plane/utils/issue_filters.py` |
//! | [`issue::rich`] | `plane/utils/filters/{filterset,filter_backend}.py` |
//! | [`issue::order`] | `plane/utils/order_queryset.py` |
//! | [`issue::model`] | the tables/columns, `ISSUE_GROUP_BY_ALLOWLIST`, `issue_on_results` |
//! | [`issue::values`] | `issue_on_results`' projection and row decoding |
//! | [`issue::group_values`] | `issue_group_values` |
//! | [`paginator`] | `plane/utils/paginator.py` |
//! | [`global`] | `plane/utils/global_paginator.py` |
//!
//! The converters module (`plane/utils/filters/converters.py`) is ported in
//! [`converters`]; it turns a saved view's legacy `filters` dict into the JSON
//! tree `ComplexFilterBackend` accepts.
//!
//! ## Example
//!
//! ```ignore
//! use plane_query::issue::{run, ListRequest};
//!
//! let body = run(client, &ListRequest {
//!     query: ctx.query(),
//!     workspace_slug: slug,
//!     project_id,
//!     timezone,
//!     now: plane_query::global::now(),
//!     user_id: Some(user.id),
//!     is_guest: false,
//!     guest_view_all_features: project.guest_view_all_features,
//! }).await?;
//! Ok(ViewResponse::json(200, body.to_serde()))
//! ```
//!
//! ## Round trips and latency
//!
//! `crates/plane-query/README.md` records the measurement: a grouped issue
//! list (`group_by=state_id`, 300 issues, `per_page=100`) costs the Python
//! paginator four to six queries per page and this port three. The p50 numbers
//! are in the same file; re-run `apps/server/tools/query/bench.py` to refresh
//! them.

/// A process-wide count of statements executed by this crate.
///
/// One relaxed atomic increment per statement (nanoseconds against a query that
/// costs microseconds). `tests/bench.rs` uses it to report round trips; a
/// handler can ignore it.
pub mod stats {
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Statements sent to Postgres since process start.
    pub static STATEMENTS: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn record() {
        STATEMENTS.fetch_add(1, Ordering::Relaxed);
    }

    /// The current count.
    pub fn statements() -> u64 {
        STATEMENTS.load(Ordering::Relaxed)
    }
}

pub mod converters;
pub mod error;
pub mod global;
pub mod issue;
pub mod json;
pub mod paginator;
pub mod pyfloat;
pub mod sql;

pub use error::{ApiError, QueryError};
pub use issue::{ListRequest, run};
pub use json::Json;
pub use paginator::{Cursor, PageQuery, issue_list_page, per_page};
