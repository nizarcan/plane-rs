// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane-db`: Postgres pools and the static query catalog.
//!
//! Read `crate::catalog` for why static SQL is declared with its result shape,
//! and `crate::pool` for the read/write routing rule
//! (`plane/middleware/db_routing.py`).
//!
//! Every crate declares its own statements with [`plane_query!`] and checks
//! them with [`schema_check::verify`]; see `crate::schema_check`.

pub mod catalog;
pub mod pool;
pub mod queries;
pub mod schema_check;
pub mod tls;
pub mod url;

pub use catalog::{Column, PgType, Query};
pub use pool::{DbError, DbPools, PostgresSettings, prepare_cached};
pub use url::{DatabaseConfig, DatabaseConfigs, DatabaseUrlError, OptionValue};
