// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/utils/encryption.py` — re-exported from `plane-auth`.
//!
//! The implementation moved to [`plane_auth::fernet`] when auth P0c needed it
//! from `get_configuration_value`'s new home
//! ([`plane_auth::instance_config`]). This module keeps the old path working so
//! handlers did not change; it holds no logic.

pub use plane_auth::fernet::{decrypt_data, encrypt_data};
