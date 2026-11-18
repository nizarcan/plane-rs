// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-http-public`: every handler for the `/api/v1/` surface.
//!
//! The mount is `plane/api/urls/*.py`: 79 resolver rows
//! (`ROUTES.md:325-407`, `routes.json` group `v1`), 151 endpoint×method pairs.
//! It is a different API from `/api/`, not a thin wrapper around it: key
//! authentication only, `BaseAPIView`/`BaseViewSet` semantics, the v1
//! serializer base, and one plain-paginator envelope for every list. What the
//! two surfaces share is imported from `plane_http_app` (the write pipeline,
//! `AppState`, `common::*`) rather than copied, so a fix to the app surface
//! cannot drift from v1.
//!
//! ## Layout
//!
//! ```text
//! src/lib.rs            `register_all` (orchestrator-owned)
//! src/common/mod.rs     the pieces only the v1 surface needs
//! src/<domain>.rs       one module per domain set; added by the sets, not by P0
//! src/<domain>_queries.rs  that domain's static SQL catalog
//! ```
//!
//! `P0 — shared` (this crate's first commit) owns no v1 handler: it creates the
//! crate, registers nothing, and lands `common/` plus the shared fixes in
//! `plane-drf`, `plane-serializers` and `plane-query` that every set depends on.
//! `register_all` therefore returns `Ok(())` without touching the registry, and
//! every v1 route keeps answering the pipeline's 501 — which is what the
//! coexistence proxy expects while Django serves `/api/v1/`.
//!
//! ## What the sets add
//!
//! | set | routes | modules |
//! | --- | --- | --- |
//! | A — workspace/project administration | 292–298, 307–311, 314, 340–351 | `projects`, `members`, `invitations`, `stickies`, `users`, `roots` |
//! | B — assets, labels, states, intake | 273–278, 288–291, 312–313 | `assets`, `labels`, `states`, `intake` |
//! | C — cycles and modules | 279–287, 299–306 | `cycles`, `modules` |
//! | D — work items and sub-resources | 315–326, 327–339 | `issues`, `issue_subresources`, `search` |
//!
//! Each set adds its own `register` call here, its own scenario under
//! `scenarios/read|write/v1-*.yaml`, and flips its own `ROUTES.md` cells.
//!
//! ## Differences from `/api/` a handler must not lose
//!
//! * **Auth.** `authentication_classes = [APIKeyAuthentication]` only
//!   (`plane/api/views/base.py:53,165`): a session cookie is not accepted on
//!   273–349. The two `drf.APIView` api-roots keep the default
//!   `SessionAuthentication`, so they take a session and **not** an API key.
//!   The token lookup and its `last_used` write run on the primary pool
//!   (`plane-drf`'s `Conn::primary_client`).
//! * **Throttles.** `BaseAPIView.get_throttles` is `[ApiKeyRateThrottle]` and
//!   `finalize_response` copies `X-RateLimit-*` onto every response
//!   (`plane/api/views/base.py:69-70,123-136`). `BaseViewSet` (invitations,
//!   stickies) and the api-roots keep `AnonRateThrottle` and emit no headers.
//! * **Errors.** The 404 wording differs (`"The requested resource does not
//!   exist."`) and a POST/PUT/PATCH body that parses to a scalar is a 400
//!   `Request body must be a JSON object or array.` before the handler
//!   (`plane/api/views/base.py:35,59-62,95,171-174`).
//! * **Serializers.** `plane_serializers::v1`: `fields=` removes, `expand=`
//!   only replaces, the map has 14 names, and a non-mapper name becomes
//!   `getattr(instance, "<name>_id", None)`.
//! * **Pagination.** Every paginated list uses
//!   `BasePaginator.paginate` + `OffsetPaginator`
//!   (`plane/utils/paginator.py:94-180,635-755`); [`common::page`] builds that
//!   envelope.
//!
//! See `apps/server/plans/V1_API.md` for the route-by-route inventory and the
//! upstream bugs that are ported as they are.

pub mod assets;
pub mod assets_queries;
pub mod common;
pub mod cycles;
pub mod cycles_queries;
pub mod intake;
pub mod intake_queries;
pub mod invitations;
pub mod invitations_queries;
pub mod issue_subresources;
pub mod issue_subresources_queries;
pub mod issue_subresources_write;
pub mod issues;
pub mod issues_queries;
pub mod labels;
pub mod labels_queries;
pub mod members;
pub mod members_queries;
pub mod modules;
pub mod modules_queries;
pub mod projects;
pub mod projects_queries;
mod pyvalue;
pub mod roots;
pub mod search;
pub mod states;
pub mod states_queries;
pub mod stickies;
pub mod stickies_queries;
pub mod users;
pub mod users_queries;

use plane_drf::registry::{Registry, RegistryError};

/// Register every ported `/api/v1/` handler.
///
/// The call site exists so `bins/plane-api` needs no edit when a set lands, and
/// so the ordering rule (after `/api/`, before `finish`) is stated once. Sets
/// add one line each, alphabetically by module.
pub fn register_all(registry: &mut Registry) -> Result<(), RegistryError> {
    assets::register(registry)?;
    cycles::register(registry)?;
    invitations::register(registry)?;
    intake::register(registry)?;
    labels::register(registry)?;
    issue_subresources::register(registry)?;
    issue_subresources_write::register(registry)?;
    issues::register(registry)?;
    members::register(registry)?;
    modules::register(registry)?;
    projects::register(registry)?;
    roots::register(registry)?;
    search::register(registry)?;
    states::register(registry)?;
    stickies::register(registry)?;
    users::register(registry)?;
    Ok(())
}
