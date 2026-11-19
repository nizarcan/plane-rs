// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-drf`: Django REST Framework's view lifecycle, rebuilt from Django's
//! own introspection.
//!
//! Every Rust HTTP handler from Phase 3 on runs inside this crate. Its job is
//! that authentication, permission classes, `@allow_permission`, throttles, the
//! `BaseViewSet` 405 guard, method dispatch, `Allow` headers, content
//! negotiation, timezone activation, exception mapping and read-replica routing
//! behave exactly as they do in `apps/api`, and that **a handler cannot be
//! registered without that policy**.
//!
//! ## Pieces
//!
//! | Module | Python |
//! | --- | --- |
//! | [`policy`] | the per-route table, read from `routes.json` |
//! | [`registry`] | the only registration path; refuses anything the policy does not cover |
//! | [`dispatch`] | `APIView.dispatch`, `initial`, `handle_exception`, `finalize_response` |
//! | [`negotiate`] | `DefaultContentNegotiation`, `?format=`, format suffixes |
//! | [`parsers`] | `JSONParser`/`FormParser`/`MultiPartParser` selection and errors |
//! | [`auth`] | `BaseSessionAuthentication`, `APIKeyAuthentication`, lazy users |
//! | [`metadata`] | `SimpleMetadata` for `OPTIONS` |
//! | [`request`] | `request.query_params`/`request.data`, `QueryDict` |
//! | [`view`] | the handler signature: `ViewCtx` in, `ViewResponse` out |
//! | [`token_log`] | `APITokenLogMiddleware`'s row and the Celery message it publishes |
//!
//! ## Using it
//!
//! ```ignore
//! use plane_drf::dispatch::Runtime;
//! use plane_drf::registry::Registry;
//! use plane_drf::policy::PolicyTable;
//!
//! let policy = PolicyTable::load()?;
//! let mut registry = Registry::new(policy)?;
//! let mut states = registry.viewset("plane.app.views.state.base.StateViewSet")?;
//! states.action("list", handlers::states::list)?;
//! states.action("create", handlers::states::create)?;
//! let runtime = Runtime::new(registry.finish()?, redis)?;
//! ```
//!
//! The pipeline (`bins/plane-api`) resolves the path with Phase 1's route
//! table, asks [`dispatch::Runtime::is_registered`], and either dispatches or
//! answers the existing 501.
//!
//! ## Side effects a handler can reach
//!
//! [`view::ViewCtx::cache`] is `django.core.cache.cache`: the one django-redis
//! handle the process built from `Settings` (shared with the throttles). Its
//! connection is opened on the first command, so a request that never touches
//! the cache never connects. [`view::ViewCtx::celery`] is the coexistence Celery
//! publisher (DECISIONS.md D7), also built once and connected on the first
//! publish. Both are `Option` only because a test build has neither; in the API
//! role both are always present, and a failure to reach them maps to the 500
//! Django produces (`crate::error`).
//!
//! ## What the table is generated from
//!
//! `apps/server/tools/inventory/routes.py` walks Django's URL resolver and
//! records, per route: the base family from the MRO, `use_read_replica`,
//! `TimezoneMixin`, `http_method_names`, parser/renderer/metadata classes,
//! `get_view_name()`/`get_view_description()`, the lifecycle hooks a concrete
//! view overrides, the `allow_permission` decorators per method, the DRF
//! security classes, and the `OPTIONS` action field maps. `routes.json` is
//! embedded with `include_str!`; nothing is typed twice.
//!
//! ## Gaps (also listed in `ROUTES.md` and the Phase 2 report)
//!
//! * Multipart parses **field values only**; files, the exact
//!   `MultiPartParserError` wording and the part-count limit are not ported.
//! * `OPTIONS` `actions` are recorded for a permitted caller; Django's
//!   `get_object()` check for `PUT` and CPython's random set ordering of
//!   `{'PUT','POST'}` are not reproduced.
//! * `plane_serde::pyjson` decodes the common text encodings (UTF-8, ASCII,
//!   latin-1, cp1252, UTF-16/32). A JSON body in another text codec Python
//!   would decode (for example `cp437`) is refused; see [`parsers`].

pub mod auth;
pub mod db;
pub mod dispatch;
pub mod error;
pub mod metadata;
pub mod negotiate;
pub mod parsers;
pub mod policy;
pub mod registry;
pub mod request;
pub mod token_log;
pub mod view;
pub mod yaml;
pub mod yaml_error;

pub use dispatch::{DispatchOutcome, DispatchRequest, Runtime, RuntimeError, UserOutcome};
pub use error::ViewError;
pub use policy::{Family, PolicyTable, RoutePolicy};
pub use registry::{Registry, RegistryError};
pub use request::{QueryDict, ViewRequest};
pub use view::{DrfResponse, HandlerFn, PlainViewState, ViewCtx, ViewResponse};
