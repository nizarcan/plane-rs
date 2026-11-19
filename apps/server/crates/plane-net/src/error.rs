// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The exception contract each call site relies on.
//!
//! Plane's Python callers branch on the *class* of the exception, not only its
//! message:
//!
//! * `bgtasks/webhook_task.py:340` catches `requests.RequestException` and
//!   retries (Celery `autoretry_for`), deactivating the webhook after five
//!   attempts and writing a `webhook_logs` row with status 500;
//! * `bgtasks/webhook_task.py:368` catches `ValueError` separately: invalid or
//!   blocked URLs are **not** retried, are logged with status 400 and
//!   `"Webhook URL rejected: <message>"`, and never deactivate the webhook;
//! * `bgtasks/work_item_link_task.py:132` treats `requests.RequestException` as
//!   "fetch failed, keep going with no title" and `ValueError` as "URL
//!   validation failed", both without failing the task.
//!
//! [`FetchError::is_value_error`] and [`FetchError::is_request_exception`]
//! answer those two questions so a handler cannot accidentally conflate them.

use std::fmt;

use crate::pyurl::PyUrlError;

#[derive(Debug)]
pub enum FetchError {
    /// `ValueError` from `plane.utils.url_security` / `plane.utils.ip_address`:
    /// an invalid URL, an unresolvable hostname, or a blocked address. Never
    /// retried by Plane; the message is part of the webhook log row.
    Value(PyUrlError),
    /// `requests.RequestException`: connect/TLS/read failures and protocol
    /// errors. Retryable (the webhook task backs off and eventually
    /// deactivates); each attempt's `str(exc)` lands in the log row.
    Transport(reqwest::Error),
    /// `requests.TooManyRedirects`, a `RequestException` subclass. Its message
    /// is `"Exceeded {max_redirects} redirects for URL: {url}"`.
    TooManyRedirects { max_redirects: u32, url: String },
    /// Plane's own streaming cap (`DATA_UPLOAD_MAX_MEMORY_SIZE` in the OAuth
    /// avatar upload) was exceeded. Python returns `None` from the caller,
    /// which falls back to the provider's URL.
    BodyTooLarge { limit: usize },
    /// `requests.exceptions.InvalidURL` or another failure while building the
    /// prepared request (a URL that passed `_split_target` but not
    /// `requests`' own preparation). A `RequestException` subclass, so Plane
    /// retries it.
    Preparation(String),
    /// TLS configuration could not be built (a `REQUESTS_CA_BUNDLE` that does
    /// not load is an `OSError` in Python, neither a `ValueError` nor a
    /// `RequestException`).
    Tls(String),
    /// A header could not be encoded. Python raises `UnicodeEncodeError` from
    /// `http.client` (again neither `ValueError` nor `RequestException`); the
    /// only caller-visible consequence is that the webhook task's generic
    /// `except Exception` path runs.
    Header(String),
    /// An OTLP/gRPC transport or protocol failure (connect, TLS, HTTP/2, a
    /// reset stream). `grpc` raises `RpcError` here; the exporter maps it to
    /// `UNAVAILABLE`.
    Grpc(String),
}

impl FetchError {
    pub fn value(message: impl Into<String>) -> Self {
        Self::Value(PyUrlError::new(message))
    }

    /// True for the Python `ValueError` branch.
    pub fn is_value_error(&self) -> bool {
        matches!(self, Self::Value(_))
    }

    /// True for the Python `requests.RequestException` branch (including
    /// `TooManyRedirects` and `InvalidURL`).
    pub fn is_request_exception(&self) -> bool {
        matches!(
            self,
            Self::Transport(_)
                | Self::TooManyRedirects { .. }
                | Self::Preparation(_)
                | Self::Grpc(_)
        )
    }

    /// `str(exc)` as Python would see it, for logging and webhook log rows.
    pub fn message(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value(error) => f.write_str(error.message()),
            Self::Transport(error) => write!(f, "{error}"),
            Self::TooManyRedirects { max_redirects, url } => {
                write!(f, "Exceeded {max_redirects} redirects for URL: {url}")
            }
            Self::BodyTooLarge { limit } => {
                write!(f, "response body exceeds {limit} bytes")
            }
            Self::Preparation(message)
            | Self::Tls(message)
            | Self::Header(message)
            | Self::Grpc(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for FetchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Value(error) => Some(error),
            Self::Transport(error) => Some(error),
            _ => None,
        }
    }
}

impl From<PyUrlError> for FetchError {
    fn from(error: PyUrlError) -> Self {
        Self::Value(error)
    }
}

impl From<reqwest::Error> for FetchError {
    fn from(error: reqwest::Error) -> Self {
        Self::Transport(error)
    }
}
