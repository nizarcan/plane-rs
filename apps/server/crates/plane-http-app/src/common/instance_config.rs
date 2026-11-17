// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane/license/utils/instance_value.py:17-39` — `get_configuration_value`
//! for a single key, at the `ViewCtx` the handlers already hold.
//!
//! The implementation moved to [`plane_auth::instance_config`] when auth P0c
//! needed it from `plane-auth` (which cannot depend on `plane-http-app`); this
//! module keeps the two signatures its callers use and adds the `ViewCtx`
//! plumbing back. The semantics — one query for all keys, a database row
//! beating the environment, an encrypted row decrypted with `SECRET_KEY` — are
//! documented on the moved function.
//!
//! One approximation, unchanged: Python's `.values()` yields `None` for a
//! `NULL` value, and [`configuration_value`] returns `Some("")` for it. Every
//! single-key call site compares the result against a string (`== "1"`,
//! truthiness), where `None` and `""` are indistinguishable, and unlike `None`
//! it keeps the row from falling back to the environment.
//! [`configuration_values`] keeps the `None`, because
//! `get_llm_config` (`plane/app/views/external/base.py:76-120`) calls `.lower()`
//! on the provider value, and Python answers a `NULL` row with
//! `AttributeError: 'NoneType' object has no attribute 'lower'` — a 500 that a
//! `""` would turn into the ordinary "not configured" 400.

use plane_drf::error::ViewError;
use plane_drf::view::ViewCtx;

use crate::common::client;

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
/// A database failure as the family's 500. Django's `.values()` query runs
/// inside the view, so a missing table or a dead connection is a 500 there too.
pub async fn configuration_values(
    ctx: &ViewCtx<'_>,
    keys: &[(&str, Option<&str>)],
) -> Result<Vec<Option<String>>, ViewError> {
    Ok(plane_auth::instance_config::configuration_values(client(ctx)?, ctx.settings, keys).await?)
}

/// One key's value, as `get_configuration_value([{"key": key, "default":
/// default}])[0]` returns it.
///
/// `default` is the call site's own default (Python evaluates it eagerly, so an
/// environment variable of the same name wins over it).
///
/// A `NULL` row would be `None` in Python and is `Some("")` here; see the module
/// note for why that is indistinguishable at every single-key call site.
///
/// # Errors
///
/// See [`configuration_values`].
pub async fn configuration_value(
    ctx: &ViewCtx<'_>,
    key: &str,
    default: &str,
) -> Result<Option<String>, ViewError> {
    let values = configuration_values(ctx, &[(key, Some(default))]).await?;
    Ok(Some(
        values.into_iter().next().flatten().unwrap_or_default(),
    ))
}
