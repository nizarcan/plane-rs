// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The issue *read* routes, registered from `super::register`.
//!
//! This module is the join point for the handlers whose files start with
//! `list_` (the list/detail read paths) so they can be ported independently of
//! the sub-resource files. Every handler here answers a `GET`; the write verbs
//! of the same views stay on Django.
//!
//! The handler files live next to this one and are declared here with
//! `#[path]`, so `src/issues/mod.rs` only ever gains one `mod lists;` line and
//! the two issue branches merge without touching the same lines.

use plane_drf::registry::{Registry, RegistryError};

#[path = "list_common.rs"]
pub mod list_common;
#[path = "list_detail_endpoint.rs"]
mod list_detail_endpoint;
#[path = "list_drafts.rs"]
mod list_drafts;
#[path = "list_export.rs"]
mod list_export;
#[path = "list_list_endpoint.rs"]
mod list_list_endpoint;
#[path = "list_paginated.rs"]
mod list_paginated;
#[path = "list_pyset.rs"]
mod list_pyset;
#[path = "list_retrieve.rs"]
mod list_retrieve;
#[path = "list_small.rs"]
mod list_small;
#[path = "list_viewset.rs"]
mod list_viewset;

/// Register the issue read handlers, in route order.
pub fn register_lists(registry: &mut Registry) -> Result<(), RegistryError> {
    list_list_endpoint::register(registry)?;
    list_paginated::register(registry)?;
    list_small::register(registry)?;
    list_retrieve::register(registry)?;
    list_detail_endpoint::register(registry)?;
    list_export::register(registry)?;
    list_drafts::register(registry)?;
    list_viewset::register(registry)?;
    Ok(())
}
