// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/utils/instance_value.py:17-39` — `get_configuration_value`.
//!
//! ```python
//! def get_configuration_value(keys):
//!     environment_list = []
//!     if settings.SKIP_ENV_VAR:
//!         instance_configuration = InstanceConfiguration.objects.values(
//!             "key", "value", "is_encrypted"
//!         )
//!         for key in keys:
//!             for item in instance_configuration:
//!                 if key.get("key") == item.get("key"):
//!                     if item.get("is_encrypted", False):
//!                         environment_list.append(decrypt_data(item.get("value")))
//!                     else:
//!                         environment_list.append(item.get("value"))
//!                     break
//!             else:
//!                 environment_list.append(key.get("default"))
//!     else:
//!         for key in keys:
//!             environment_list.append(os.environ.get(key.get("key"), key.get("default")))
//!     return tuple(environment_list)
//! ```
//!
//! Three details matter for a caller:
//!
//! * `SKIP_ENV_VAR` (`plane/settings/common.py:401`, default `"1"`) chooses
//!   between the database and the environment. With the default, every call
//!   reads the **whole** table, even when the key is absent, and the
//!   environment variable is only the fallback for a missing row.
//! * the fallback itself is `os.environ.get(key, default)` — the environment
//!   wins over the call site's default, in both branches.
//! * an encrypted value is decrypted with the process `SECRET_KEY`; a
//!   decryption failure is `""` ([`crate::fernet`]), which every caller reads as
//!   "not configured".
//!
//! One approximation: Python's `.values()` yields `None` for a `NULL` value, and
//! [`configuration_values`] keeps that `None` here. (The single-key wrapper in
//! `plane-http-app::common::instance_config` turns it into `Some("")`, which is
//! indistinguishable at its call sites; this module does not, because the
//! `/auth/` flags compare against `"0"`/`"1"` and pass the value on to typed
//! readers.)
//!
//! Moved here from `plane-http-app::common::instance_config` by auth P0c:
//! `plane-auth` cannot depend on `plane-http-app` (the dependency runs the other
//! way), and the `/auth/` flag reads (§2i) need the same semantics. The old
//! path re-exports it, so its callers did not change.

use deadpool_postgres::Client;
use plane_core::config::Settings;

use crate::error::AuthError;
use crate::fernet::decrypt_data;

/// `get_configuration_value([...])` for several keys, in the order given.
///
/// Each entry is `(key, default)`, and `default` is the call site's own default
/// — `None` for Python's `None`. Both branches of the Python fall back to
/// `os.environ.get(key, default)`, because every multi-key call site writes
/// `"default": os.environ.get(...)` explicitly; an environment variable that is
/// set (even to an empty string) therefore wins over `default`, and a database
/// row wins over both.
///
/// Django evaluates the queryset **once** for all keys
/// (`plane/license/utils/instance_value.py:20-23`), so this does too: one
/// statement, one pass over the rows per key, the first matching row winning.
///
/// # Errors
///
/// A database failure as [`AuthError::Database`]. Django's `.values()` query
/// runs inside the view, so a missing table or a dead connection is a 500 there
/// too.
pub async fn configuration_values(
    client: &Client,
    settings: &Settings,
    keys: &[(&str, Option<&str>)],
) -> Result<Vec<Option<String>>, AuthError> {
    // `os.environ.get(key, default)`.
    let fallbacks: Vec<Option<String>> = keys
        .iter()
        .map(|(key, default)| {
            std::env::var(key)
                .ok()
                .or_else(|| default.map(str::to_string))
        })
        .collect();
    if !settings.skip_env_var {
        return Ok(fallbacks);
    }
    // `InstanceConfiguration.objects.values("key", "value", "is_encrypted")`.
    let statement = plane_db::queries::LOAD_INSTANCE_CONFIGURATIONS
        .prepare(client)
        .await?;
    let rows = client.query(&statement, &[]).await?;
    let mut values: Vec<Option<String>> = Vec::with_capacity(keys.len());
    for (index, (key, _)) in keys.iter().enumerate() {
        let mut found = None;
        for row in &rows {
            let row_key: &str = row.try_get(1)?;
            if row_key != *key {
                continue;
            }
            let value: Option<&str> = row.try_get(2)?;
            let is_encrypted: bool = row.try_get(4)?;
            // A `NULL` row is Python's `None` — unless it is marked encrypted,
            // where `decrypt_data(None)` is `""` (`if not encrypted_data:
            // return ""`, `plane/license/utils/encryption.py:20-30`).
            found = Some(if is_encrypted {
                Some(decrypt_data(
                    settings.secret_key.value(),
                    value.unwrap_or_default(),
                ))
            } else {
                value.map(str::to_string)
            });
            break;
        }
        values.push(match found {
            Some(value) => value,
            None => fallbacks[index].clone(),
        });
    }
    Ok(values)
}
