// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The static SQL catalog of the `ai` group.
//!
//! Every statement this group needs that has a fixed shape goes here as a
//! [`plane_query!`](plane_db::plane_query) with the result columns it expects,
//! and is listed in [`ALL_QUERIES`] so
//! `crates/plane-http-app/tests/schema_check.rs` prepares it against a
//! Django-migrated database (`DECISIONS.md` D1). Dynamic SQL — `fields=`,
//! filters, ordering, grouping — does **not** belong here; compose it with
//! `plane-query` and send it as an unnamed statement (`DECISIONS.md` D1
//! amendment). `HANDLERS.md` has both examples.
//!
//! Empty today: the group has no ported handler, so it has no statements.

use plane_db::Query;

/// Every catalogued statement of this group, in declaration order.
pub static ALL_QUERIES: &[&Query] = &[];
