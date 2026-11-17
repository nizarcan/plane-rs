// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/utils/instance_value.py:17-39` — `get_configuration_value`
//! for a single key.
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
//!   decryption failure is `""` (`common::fernet`), which every caller reads as
//!   "not configured".
//!
//! One approximation: Python's `.values()` yields `None` for a `NULL` value, and
//! this returns `Some("")` for it. Every call site compares the result against a
//! string (`== "1"`, truthiness), where `None` and `""` are indistinguishable,
//! and unlike `None` it keeps the row from falling back to the environment.

use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;

use crate::common::{client, fernet};

/// One key's value, as `get_configuration_value([{"key": key, "default":
/// default}])[0]` returns it.
///
/// `default` is the call site's own default (Python evaluates it eagerly, so an
/// environment variable of the same name wins over it).
///
/// # Errors
///
/// A database failure as the family's 500. Django's `.values()` query runs
/// inside the view, so a missing table or a dead connection is a 500 there too.
pub async fn configuration_value(
    ctx: &ViewCtx<'_>,
    key: &str,
    default: &str,
) -> Result<Option<String>, ViewError> {
    // `os.environ.get(key, default)`.
    let environment = || std::env::var(key).unwrap_or_else(|_| default.to_string());
    if !ctx.settings.skip_env_var {
        return Ok(Some(environment()));
    }
    // `InstanceConfiguration.objects.values("key", "value", "is_encrypted")`.
    let statement = plane_db::queries::LOAD_INSTANCE_CONFIGURATIONS
        .prepare(client(ctx)?)
        .await?;
    let rows = client(ctx)?.query(&statement, &[]).await?;
    for row in &rows {
        let row_key: &str = row.try_get(1)?;
        if row_key != key {
            continue;
        }
        let value: Option<&str> = row.try_get(2)?;
        let is_encrypted: bool = row.try_get(4)?;
        // `decrypt_data(None)` is `""`, like `decrypt_data("")`.
        let raw = value.unwrap_or_default();
        let value = if is_encrypted {
            fernet::decrypt_data(ctx.settings.secret_key.value(), raw)
        } else {
            raw.to_string()
        };
        return Ok(Some(value));
    }
    Ok(Some(environment()))
}
