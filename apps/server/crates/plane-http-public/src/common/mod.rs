// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The pieces every `/api/v1/` handler needs and `/api/` does not.
//!
//! Nothing here may grow a domain rule: a shape only one v1 set needs belongs
//! in that set's module. The two modules that exist are the ones the plan's
//! §3a named — the plain-page envelope ([`page`]) and the `BaseSerializer`-mode
//! render helper ([`render`]) — and each is used by every set.
//!
//! | Module | What it removes from every handler |
//! | --- | --- |
//! | [`mod@page`] | `per_page`/`cursor` parsing and the `OffsetPaginator` envelope |
//! | [`mod@render`] | building the v1 `RenderCtx`, including the `_id` fallback |
//!
//! The rest of what a v1 handler needs is imported, not copied:
//! `plane_http_app::common` (`RequestRender`, `render`, `render_list`, the
//! lookup helpers) and `plane_http_app::write` (the generated serializer
//! tables) behave identically on this surface. `plane_http_app::AppState` is
//! the shared per-request handle.

pub mod html;
pub(crate) mod html_tables;
pub mod page;
pub mod render;
