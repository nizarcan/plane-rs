// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Route group `public` (`deploy/route-groups.yaml`): the anonymous project
//! board under `/api/public/`.
//!
//! Python: `apps/api/plane/space/` — 25 routes, mounted by `plane/urls.py` at
//! `/api/public/` and wired in `plane/space/urls/{issue,intake,project,asset}.py`.
//!
//! This module holds the pieces every route of the group shares; the handlers
//! land with the porting sets:
//!
//! * [`board`] — the deploy-board resolution every view starts with, its 404s
//!   and the write-scope predicates (`PUBLIC_INSTANCES.md` §2a);
//! * [`queries`] — the statements [`board`] prepares.
//!
//! The routes themselves are not registered yet: `register_all`
//! (`crate::register_all`) does not call a `public::register` until the first
//! handler exists, which is why every `/api/public/` path still answers the
//! pipeline's 501.
//!
//! Two shared behaviours live in the runtime, not here:
//!
//! * `DeployBoardAnchorMixin.initial` (`plane/space/views/base.py:47-59`) —
//!   the 404 for a board whose `entity_identifier` is not its `project_id` —
//!   is `plane_drf::dispatch`'s `InitialExtra::DeployBoardAnchor`
//!   (`plane-drf/src/dispatch.rs:472-477,740-753`), and it runs **after**
//!   `check_permissions`, so an anonymous request on an `IsAuthenticated` route
//!   is a 401 before the anchor is read;
//! * the 500 body of the group's `handle_exception`
//!   (`plane/space/views/base.py:91-124`) is the family error type the
//!   dispatcher already renders (`ApiError::FamilyServerError`).

pub mod board;
pub mod queries;
