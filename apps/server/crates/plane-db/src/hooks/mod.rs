// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Model `save()`/signal overrides as explicit functions (Phase 5).
//!
//! Django's write paths do not write what the serializer validated: a model
//! `save()` override assigns sequence numbers, sort orders and stripped HTML,
//! `BaseModel.save()` stamps the audit columns, and `SoftDeleteModel.delete()`
//! sets `deleted_at` and enqueues the cascade. In Rust there is no ORM and no
//! implicit hook, so `MODEL_HOOKS.md` lists every one of them (H01–H36) and
//! assigns a function here. **A write path that does not call the hooks its
//! model needs writes a different row than Django.** The differential harness
//! compares the database diff, so a missed hook fails the scenario.
//!
//! ## Ported today
//!
//! | Hook | Python | Here |
//! | --- | --- | --- |
//! | H01 | `BaseModel.save` (`plane/db/models/base.py:23-45`) | [`audit::stamp`] |
//! | H03 | `SoftDeleteModel.delete` (`plane/db/mixins.py:65-77`) | [`soft_delete::delete_instance`] |
//! | H23 | `Sticky.save` (`plane/db/models/sticky.py:38-54`) | [`sticky::before_save`] |
//! | H08/H09 | `Project.save` (`plane/db/models/project.py:193-227`) | [`project::before_insert`] / [`project::before_update`] / [`project::retime_intake_snoozes`] |
//! | H20 | `Label.save` (`plane/db/models/label.py:46-54`) | [`label::before_insert`] |
//! | H25 | `UserFavorite.save` (`plane/db/models/favorite.py:52-72`) | [`favorite::before_insert`] |
//! | H32 | `ProjectMember.save` (`plane/db/models/project.py:276-293`) | [`project_member::before_insert`] |
//!
//! Every other row of `MODEL_HOOKS.md` is `not-ported`; a domain ports its own
//! hooks here before its write routes go to Rust. The module names above are
//! the ones `MODEL_HOOKS.md`'s "Rust" column already promises, so a domain adds
//! a sibling module (`hooks::issue`, `hooks::state`, …) with the same shape:
//! one function per mutation, taking the values the SQL will bind and returning
//! the final values, plus a doc comment that cites the Python line.
//!
//! ## Contract
//!
//! * A hook **returns the values to write**; it does not write them itself
//!   (except where the Python hook issues its own statement — H23's
//!   `MAX(sort_order)` does). The handler then binds them in one statement, so
//!   the row and the response share the same in-memory values, as Django's
//!   instance does.
//! * A hook runs inside the caller's transaction/autocommit boundary; it never
//!   opens its own.
//! * Timestamps come from the caller (`django.utils.timezone.now`); a hook
//!   never calls the clock twice for one save.

pub mod audit;
pub mod favorite;
pub mod html;
/// Generated data tables for [`html`] (CPython's `html.entities.html5` and its
/// invalid-reference tables). Private: only `html::strip_tags` needs them.
mod html_entities;
pub mod issue;
pub mod issue_m2m;
pub mod json_merge;
pub mod label;
pub mod project;
pub mod project_member;
pub mod soft_delete;
pub mod state;
pub mod sticky;
