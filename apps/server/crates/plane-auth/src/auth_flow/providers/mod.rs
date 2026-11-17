// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The four OAuth providers: `plane/authentication/provider/oauth/*.py`.
//!
//! Each module is the provider-specific half of [`OauthAdapter`]
//! (`adapter/oauth.py`) — the auth URL, the token request, the token
//! response's shape and the claim mapping — while everything they share lives
//! in [`super::oauth`]: the HTTP calls, `validate_oauth_state`, the
//! `accounts` upsert, the IDP sync and the avatar.
//!
//! | Rust | Python |
//! | --- | --- |
//! | [`google`] | `provider/oauth/google.py` |
//! | [`github`] | `provider/oauth/github.py` |
//! | [`gitlab`] | `provider/oauth/gitlab.py` |
//! | [`gitea`] | `provider/oauth/gitea.py` |
//!
//! ## The claim rules that are security behaviour, not formatting
//!
//! The four providers were patched together for GHSA-7j95-vh8g-f365 (account
//! takeover through an unverified address) and they do **not** share one rule:
//!
//! * Google requires `verified_email is True` (`google.py:108-112`) — an
//!   absent claim and a `1` are both rejected, because the comparison is
//!   identity against the boolean.
//! * GitHub needs a **primary and verified** address from `/user/emails`
//!   (`github.py:120-132`), and a non-list response is the provider error.
//! * GitLab requires a truthy `confirmed_at` (`gitlab.py:111-117`) and takes
//!   the address from the user object itself.
//! * Gitea always calls `/user/emails`, preferring primary+verified, then any
//!   verified, and never falls back to an unverified one
//!   (`gitea.py:117-155`).
//!
//! [`OauthAdapter`]: super::oauth::OauthAdapter

use chrono::{DateTime, Utc};
use serde_json::Value;

use super::adapter::AdapterError;

#[cfg(test)]
mod tests;

pub mod gitea;
pub mod github;
pub mod gitlab;
pub mod google;

pub use gitea::GiteaProvider;
pub use github::GithubProvider;
pub use gitlab::GitlabProvider;
pub use google::GoogleProvider;

/// `datetime.fromtimestamp(value, tz=pytz.utc)`.
///
/// Python accepts an `int`/`float` (a `bool` is an `int`) and raises
/// `TypeError` for anything else; a value outside the platform range raises
/// `OverflowError`/`OSError`. Both are uncaught in the provider, so they are
/// [`AdapterError::Internal`] — a 500.
pub(crate) fn from_timestamp(value: &Value) -> Result<DateTime<Utc>, AdapterError> {
    let seconds = python_number(value, "datetime.fromtimestamp")?;
    // CPython rounds the double to the nearest microsecond.
    let micros = (seconds * 1_000_000.0).round();
    if !micros.is_finite() {
        return Err(AdapterError::Internal(
            "OverflowError: timestamp out of range".to_string(),
        ));
    }
    DateTime::from_timestamp_micros(micros as i64)
        .ok_or_else(|| AdapterError::Internal("OverflowError: timestamp out of range".to_string()))
}

/// `datetime.now(tz=pytz.utc) + timedelta(seconds=<value>)` (`gitea.py:97-101`).
pub(crate) fn now_plus_seconds(value: &Value) -> Result<DateTime<Utc>, AdapterError> {
    let seconds = python_number(value, "timedelta")?;
    let now = Utc::now();
    let micros = seconds * 1_000_000.0;
    let delta = chrono::TimeDelta::microseconds(micros as i64);
    now.checked_add_signed(delta)
        .ok_or_else(|| AdapterError::Internal("OverflowError: date value out of range".to_string()))
}

/// `created_at + expires_in` (`gitlab.py:99-104`).
pub(crate) fn add_timestamps(
    created_at: &Value,
    expires_in: &Value,
) -> Result<DateTime<Utc>, AdapterError> {
    let left = python_number(created_at, "unsupported operand type(s) for +")?;
    let right = python_number(expires_in, "unsupported operand type(s) for +")?;
    from_timestamp(&Value::from(left + right))
}

/// A JSON number as `f64`, with Python's `bool`-is-an-`int` rule.
fn python_number(value: &Value, operation: &str) -> Result<f64, AdapterError> {
    match value {
        Value::Number(number) => number.as_f64().ok_or_else(|| {
            AdapterError::Internal(format!("TypeError: {operation}: bad number {number}"))
        }),
        Value::Bool(value) => Ok(if *value { 1.0 } else { 0.0 }),
        other => Err(AdapterError::Internal(format!(
            "TypeError: {operation}: '{}' object cannot be interpreted as a number",
            python_type_name(other)
        ))),
    }
}

/// `type(value).__name__` for the error messages above.
fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(number) if number.is_f64() => "float",
        Value::Number(_) => "int",
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// `token_response`/`user_info_response` must be a dict before `.get` runs;
/// a list or a scalar is Python's `AttributeError` — a 500.
pub(crate) fn require_object<'a>(
    value: &'a Value,
    call: &str,
) -> Result<&'a serde_json::Map<String, Value>, AdapterError> {
    match value {
        Value::Object(map) => Ok(map),
        other => Err(AdapterError::Internal(format!(
            "AttributeError: '{}' object has no attribute 'get' ({call})",
            python_type_name(other)
        ))),
    }
}
