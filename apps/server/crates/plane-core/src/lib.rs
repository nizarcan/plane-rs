// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Shared library for every `plane-*` service: configuration, errors, time and
//! identifiers.
//!
//! This crate must stay free of HTTP server and database details so that the
//! worker, beat and migrator binaries can reuse it unchanged (`RUST_REWRITE.md`,
//! "Target architecture").

pub mod clock;
pub mod config;
pub mod context;
pub mod error;
pub mod ids;
pub mod json;
pub mod py;
pub mod secrets;

pub use config::{ConfigError, ConfigWarning, Role, Settings};
pub use context::RequestContext;
pub use error::{ApiError, ErrorBody, ErrorSurface, model_object_name};
pub use ids::{InstanceId, SessionKey, UserId};
pub use json::Object;
