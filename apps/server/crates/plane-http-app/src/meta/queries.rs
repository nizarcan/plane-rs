// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `meta` group.
//!
//! Empty: `GET /api/timezones/` reads nothing, and the group's other route
//! (`/api/unsplash/`) is not ported. A group that grows a statement declares it
//! here with [`plane_query!`](plane_db::plane_query) and lists it in
//! [`ALL_QUERIES`]; `crates/plane-http-app/tests/schema_check.rs` prepares it
//! against a Django-migrated database.

use plane_db::Query;

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[];
