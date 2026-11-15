// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Python-compatible JSON encoding for the Rust rewrite.
//!
//! Every JSON byte a Rust handler emits must equal what the Django code
//! emits today. Three dialects matter:
//!
//! * [`Dialect::Drf`] — `rest_framework.renderers.JSONRenderer` with Plane's
//!   settings (compact separators, `ensure_ascii=False`, `allow_nan=False`,
//!   and U+2028 / U+2029 replaced by the ASCII escapes `\u2028` / `\u2029`).
//! * [`Dialect::Django`] — `django.http.JsonResponse` /
//!   `json.dumps(obj, cls=DjangoJSONEncoder)` (`, ` / `: ` separators,
//!   `ensure_ascii=True`, `allow_nan=True`).
//! * [`Dialect::Kombu`] — `kombu.utils.json.dumps` as used for Celery message
//!   bodies: same flags as Django, plus the `{"__type__": …}` envelopes for
//!   dates, datetimes, times, decimals, UUIDs and bytes.
//!
//! Floats are always written with CPython's `float.__repr__` rules in every
//! dialect (shortest round-trip digits, positional between `1e-4` and `1e16`,
//! at least two exponent digits, `.0` on integral positional values).
//!
//! [`transcode_json`] converts the text form Postgres returns for a
//! `json`/`jsonb` column the way `psycopg` + `json.loads` + the dialect would,
//! without building a `Value` tree.
//!
//! See `apps/server/HTTP_CONTRACT.md` §10.1, §10.2, §10.4 and Appendix A.4,
//! and `apps/server/TASKS.md` §2 for the behaviour these types reproduce.
//!
//! ```
//! use plane_serde::{Dialect, to_vec};
//!
//! let bytes = to_vec(Dialect::Drf, &vec![1.0, 1e-5]).unwrap();
//! assert_eq!(bytes, b"[1.0,1e-05]");
//!
//! let bytes = to_vec(Dialect::Django, &vec![1.0, 1e-5]).unwrap();
//! assert_eq!(bytes, b"[1.0, 1e-05]");
//! ```

pub mod pyjson;
pub mod values;

mod decimal;
mod float;
mod formatter;
mod serializer;
mod transcode;
mod wtf8;

pub use decimal::PyDecimal;
pub use decimal::PyDecimalError;

use std::io;

use serde::Serialize;

/// The Python JSON dialect to reproduce.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Dialect {
    /// `rest_framework.renderers.JSONRenderer` with Plane's settings:
    /// separators (",", ":"), `ensure_ascii=False`, `allow_nan=False`, and
    /// U+2028 / U+2029 written as the six-byte ASCII sequences `\u2028` and
    /// `\u2029` — that is, backslash, `u`, then the four hex digits — exactly
    /// as `rest_framework/renderers.py` does with
    /// `ret.replace("\u2028", "\\u2028").replace("\u2029", "\\u2029")`.
    ///
    /// A top-level `None` renders as an empty body, because
    /// `JSONRenderer.render(None)` returns `b""`.
    Drf,
    /// `django.http.JsonResponse` / `json.dumps(obj, cls=DjangoJSONEncoder)`
    /// with its defaults: separators (", ", ": "), `ensure_ascii=True` and
    /// `allow_nan=True`.
    Django,
    /// `kombu.utils.json.dumps` as used for Celery message bodies: the same
    /// separators and `ensure_ascii`/`allow_nan` flags as [`Dialect::Django`],
    /// with kombu's type envelopes for values that are not plain JSON types.
    Kombu,
}

impl Dialect {
    /// Whether this dialect escapes every character outside printable ASCII.
    #[inline]
    #[must_use]
    pub fn ensure_ascii(self) -> bool {
        !matches!(self, Dialect::Drf)
    }

    /// Whether `NaN`, `Infinity` and `-Infinity` are allowed in the output.
    #[inline]
    #[must_use]
    pub fn allow_nan(self) -> bool {
        !matches!(self, Dialect::Drf)
    }

    #[inline]
    pub(crate) fn item_separator(self) -> &'static [u8] {
        match self {
            Dialect::Drf => b",",
            Dialect::Django | Dialect::Kombu => b", ",
        }
    }

    #[inline]
    pub(crate) fn key_separator(self) -> &'static [u8] {
        match self {
            Dialect::Drf => b":",
            Dialect::Django | Dialect::Kombu => b": ",
        }
    }
}

/// An error from [`to_vec`], [`to_writer`] or [`transcode_json`].
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A NaN or infinity was serialised under a dialect with
    /// `allow_nan=False` (`Dialect::Drf`). CPython raises
    /// `ValueError("Out of range float values are not JSON compliant: …")`.
    #[error("out of range float values are not JSON compliant: {0}")]
    NonFinite(f64),
    /// The writer returned an error.
    #[error(transparent)]
    Io(#[from] io::Error),
    /// The input was not valid JSON, or not representable in Rust.
    #[error("invalid JSON: {0}")]
    Syntax(String),
    /// A `Serialize` implementation refused the value (the Python analogue
    /// is a `TypeError` or `decimal.InvalidOperation`).
    #[error("{0}")]
    Custom(String),
}

/// Convert `value` to the bytes `json.dumps` would produce for `dialect`.
///
/// See [`Dialect`] for the differences between the dialects.
pub fn to_vec<T>(dialect: Dialect, value: &T) -> Result<Vec<u8>, Error>
where
    T: ?Sized + Serialize,
{
    let mut serializer = serializer::PySerializer::new(dialect, Vec::with_capacity(256));
    value.serialize(&mut serializer)?;
    Ok(serializer.into_writer())
}

/// Write `value` to `writer` as `json.dumps` would for `dialect`.
///
/// Like `serde_json::to_writer`, the writer may be partially written when an
/// error is returned.
pub fn to_writer<W, T>(dialect: Dialect, writer: W, value: &T) -> Result<(), Error>
where
    W: io::Write,
    T: ?Sized + Serialize,
{
    let mut serializer = serializer::PySerializer::new(dialect, writer);
    value.serialize(&mut serializer)
}

/// Re-encode the text form of a Postgres `json`/`jsonb` value the way
/// `psycopg` + `json.loads` + `dialect` would.
///
/// * Object key order is kept exactly as in the input text (Postgres `jsonb`
///   has already normalised it; `json` keeps the stored order).
/// * Duplicate keys behave like a Python `dict`: the first occurrence keeps
///   its position and the last occurrence supplies the value, exactly as
///   `json.loads` does. (`jsonb` never produces duplicates.)
/// * An integer lexeme is emitted as Python `int` would print it: arbitrary
///   precision, `-0` becomes `0`.
/// * Any lexeme with a fraction or an exponent is converted to `f64` and
///   written with Python `float.__repr__`; a value that overflows to
///   infinity is an error under [`Dialect::Drf`] and `Infinity` under the
///   other two dialects.
/// * String escapes are decoded and re-escaped for the dialect.
///
/// This is a streaming tokeniser over the input text; it does not build a
/// value tree. Only one object's member values are buffered, to reproduce
/// Python's duplicate-key rule.
pub fn transcode_json(dialect: Dialect, json_text: &str, out: &mut Vec<u8>) -> Result<(), Error> {
    transcode::transcode(dialect, json_text, out)
}

impl serde::ser::Error for Error {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Error::Custom(message.to_string())
    }
}
