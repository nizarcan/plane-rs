// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Registration for the issue sub-resource routes.
//!
//! The issue list/detail routes are ported by a second agent in the same
//! directory; `issues::register` calls this function and that agent's, so the
//! two branches only ever add one line each to `mod.rs`. Everything else this
//! half of the group needs is declared here.
//!
//! Routes registered here (orders from `routes.json`):
//!
//! | Order | Route |
//! | --- | --- |
//! | 74/75 | `issue-links/` list, retrieve |
//! | 76/77 | `issue-attachments/` list, detail (a ported 500 — see below) |
//! | 80 | `history/` |
//! | 81/82 | `comments/` list, retrieve |
//! | 83/85 | `issue-subscribers/` list, `subscribe/` status |
//! | 86 | `reactions/` list |
//! | 88 | `comments/<comment_id>/reactions/` list |
//! | 93 | `issue-relation/` list |
//! | 97/98 | `versions/` list, detail |
//! | 99/100 | `work-items/<work_item_id>/description-versions/` list, detail |
//!
//! Route 77 (`issue-attachments/<pk>/`) is registered too, as the 500 Django
//! answers: `IssueAttachmentEndpoint.get` has no `pk` parameter. The
//! presigned-redirect branch lives in `IssueAttachmentV2Endpoint`, which is a
//! different route group and is not ported here.

use plane_drf::registry::{Registry, RegistryError};

// The handler files sit next to this one in `src/issues/`; `#[path]` keeps the
// declarations here (so `issues/mod.rs` only gains the one `mod sub_resources;`
// line the parallel agent's branch does not touch) without a subdirectory.
#[path = "sub_activity.rs"]
mod sub_activity;
#[path = "sub_attachments.rs"]
mod sub_attachments;
#[path = "sub_comments.rs"]
mod sub_comments;
#[path = "sub_common.rs"]
pub(crate) mod sub_common;
#[path = "sub_links.rs"]
mod sub_links;
#[path = "sub_queries.rs"]
pub(crate) mod sub_queries;
#[path = "sub_reactions.rs"]
mod sub_reactions;
#[path = "sub_relation.rs"]
mod sub_relation;
#[path = "sub_subscribers.rs"]
mod sub_subscribers;
#[path = "sub_versions.rs"]
mod sub_versions;

pub(super) fn register_sub_resources(registry: &mut Registry) -> Result<(), RegistryError> {
    sub_links::register(registry)?;
    sub_attachments::register(registry)?;
    sub_activity::register(registry)?;
    sub_comments::register(registry)?;
    sub_subscribers::register(registry)?;
    sub_reactions::register(registry)?;
    sub_relation::register(registry)?;
    sub_versions::register(registry)?;
    Ok(())
}
