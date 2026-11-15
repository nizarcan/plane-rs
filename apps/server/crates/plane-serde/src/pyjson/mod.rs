// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `json.loads` for request bodies, with CPython's exact values and errors.
//!
//! DRF parses a JSON body in `rest_framework/parsers.py:79-91` (`json` here is
//! `rest_framework.utils.json`, which defaults `parse_constant` to
//! `strict_constant` and `allow_nan` to `False`):
//!
//! ```python
//! encoding = get_encoding(parser_context)          # Content-Type charset or utf-8
//! decoded_stream = codecs.getreader(encoding)(stream)
//! parse_constant = json.strict_constant if self.strict else None   # STRICT_JSON is True
//! return json.load(decoded_stream, parse_constant=parse_constant)
//! ```
//!
//! and turns any `ValueError` into `ParseError("JSON parse error - %s" % exc)`.
//! Everything a client can observe about a malformed body therefore comes from
//! the pinned CPython 3.12: the `json.JSONDecodeError` text and position, the
//! `UnicodeDecodeError` text, the `sys.int_max_str_digits` message,
//! `strict_constant`'s message for `NaN`/`Infinity`, and the `RecursionError`
//! that DRF does *not* catch (it is not a `ValueError`, so it reaches Django
//! and becomes a 500).
//!
//! ```
//! use plane_serde::pyjson::{loads, Value};
//!
//! let body = br#"{"name": "Fix login", "estimate": 2.5, "labels": ["bug"]}"#;
//! let value = loads(body, "utf-8").expect("valid JSON");
//! assert_eq!(value.get("name").and_then(Value::as_str), Some("Fix login"));
//! assert_eq!(value.get("estimate").and_then(Value::as_f64), Some(2.5));
//!
//! let error = loads(br#"{"name": }"#, "utf-8").unwrap_err();
//! assert_eq!(
//!     error.parse_error_detail().as_deref(),
//!     Some("JSON parse error - Expecting value: line 1 column 10 (char 9)")
//! );
//! ```
//!
//! The parser is a port of `Modules/_json.c` (`scan_once_unicode`,
//! `_parse_object_unicode`, `_parse_array_unicode`, `_match_number_unicode`,
//! `scanstring_unicode`) and `Lib/json/decoder.py` (`JSONDecoder.decode`,
//! `JSONDecodeError`). The `decode` module documents the `codecs.StreamReader`
//! behaviour it reproduces.

mod decode;
mod parse;
mod value;

use std::fmt;

pub use parse::MAX_NESTING;
pub use value::{BigInt, INT_MAX_STR_DIGITS, PyDict, PyInt, PyStr, Value, Wtf8Error};

pub(crate) use value::{RAW_NUMBER_TOKEN, WTF8_TOKEN};

/// Which layer refused the body.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum LoadErrorKind {
    /// DRF's `get_encoding` refused the charset (a non-text codec), so the
    /// body was never read. `rest_framework/parsers.py:38-46`.
    Charset,
    /// The body did not decode: `UnicodeDecodeError` or a plain
    /// `UnicodeError` such as `UTF-16 stream does not start with BOM`.
    Decode,
    /// `json.loads` raised `JSONDecodeError` or another `ValueError`
    /// (the integer digit limit, `strict_constant`).
    Json,
    /// `json.loads` raised `RecursionError`. Not a `ValueError`: DRF does not
    /// convert it and Django answers 500.
    Recursion,
}

/// A refusal to parse a request body.
#[derive(Clone, Debug)]
pub struct LoadError {
    kind: LoadErrorKind,
    message: String,
}

impl LoadError {
    pub(crate) fn charset(encoding: &str) -> Self {
        Self {
            kind: LoadErrorKind::Charset,
            message: format!("Unsupported charset \"{encoding}\" in request Content-Type header."),
        }
    }

    pub(crate) fn decode(message: String) -> Self {
        Self {
            kind: LoadErrorKind::Decode,
            message,
        }
    }

    pub(crate) fn json(message: String) -> Self {
        Self {
            kind: LoadErrorKind::Json,
            message,
        }
    }

    pub(crate) fn recursion(message: String) -> Self {
        Self {
            kind: LoadErrorKind::Recursion,
            message,
        }
    }

    /// Which layer refused the body.
    #[must_use]
    pub const fn kind(&self) -> LoadErrorKind {
        self.kind
    }

    /// `str(exc)` for a JSON/decode failure, or DRF's `Unsupported charset`
    /// text. This never includes DRF's `JSON parse error - ` prefix.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether DRF converts this into a `ParseError` (and therefore a 400).
    #[must_use]
    pub const fn is_parse_error(&self) -> bool {
        matches!(
            self.kind,
            LoadErrorKind::Charset | LoadErrorKind::Decode | LoadErrorKind::Json
        )
    }

    /// The exact `detail` DRF's `ParseError` carries, or `None` when DRF does
    /// not catch the exception (a 500 followed by Django's 500 handler).
    #[must_use]
    pub fn parse_error_detail(&self) -> Option<String> {
        match self.kind {
            LoadErrorKind::Charset => Some(self.message.clone()),
            LoadErrorKind::Decode | LoadErrorKind::Json => {
                Some(format!("JSON parse error - {}", self.message))
            }
            LoadErrorKind::Recursion => None,
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for LoadError {}

/// Parse a request body the way `JSONParser().parse` does.
///
/// `encoding` is the charset DRF resolved from the `Content-Type` header
/// (Django's `request.encoding`, or `settings.DEFAULT_CHARSET` — `utf-8` in
/// Plane — when the header has none). The body is decoded with that codec and
/// then parsed with `json.loads` semantics.
///
/// Supported codecs: `utf-8` (and its aliases), `utf-8-sig`, `ascii`,
/// `latin-1` (and its aliases), `cp1252`, `utf-16`/`-le`/`-be` and
/// `utf-32`/`-le`/`-be`. Anything else is reported as
/// [`LoadErrorKind::Charset`] with DRF's `Unsupported charset` text. That
/// matches DRF for non-text codecs (`bz2`, `base64`, ...) and for names Python
/// cannot resolve, but a text codec such as `cp437` that Django would decode
/// is refused instead. Plane clients send UTF-8; extend `decode` if that
/// changes.
pub fn loads(bytes: &[u8], encoding: &str) -> Result<Value, LoadError> {
    let text = decode::decode(bytes, encoding)?;
    parse::parse(&text)
}

#[cfg(test)]
mod tests {
    use super::{LoadErrorKind, loads};

    #[test]
    fn drf_parse_error_details() {
        let error = loads(br#"{"a":}"#, "utf-8").unwrap_err();
        assert_eq!(error.kind(), LoadErrorKind::Json);
        assert!(error.is_parse_error());
        assert_eq!(
            error.parse_error_detail().as_deref(),
            Some("JSON parse error - Expecting value: line 1 column 6 (char 5)")
        );

        let error = loads(b"\xff", "utf-8").unwrap_err();
        assert_eq!(error.kind(), LoadErrorKind::Decode);
        assert_eq!(
            error.parse_error_detail().as_deref(),
            Some(
                "JSON parse error - 'utf-8' codec can't decode byte 0xff in position 0: invalid start byte"
            )
        );

        let error = loads(b"{}", "bz2").unwrap_err();
        assert_eq!(error.kind(), LoadErrorKind::Charset);
        assert_eq!(
            error.parse_error_detail().as_deref(),
            Some("Unsupported charset \"bz2\" in request Content-Type header.")
        );
    }

    #[test]
    fn recursion_is_not_a_drf_parse_error() {
        let doc = format!("{}{}", "[".repeat(9998), "]".repeat(9998));
        let error = loads(doc.as_bytes(), "utf-8").unwrap_err();
        assert_eq!(error.kind(), LoadErrorKind::Recursion);
        assert!(!error.is_parse_error());
        assert_eq!(error.parse_error_detail(), None);
    }

    #[test]
    fn values_match_json_loads() {
        let value = loads(b"-0", "utf-8").unwrap();
        assert_eq!(value.as_i64(), Some(0));

        let value = loads(b"-0.0", "utf-8").unwrap();
        assert_eq!(value.as_f64().map(f64::to_bits), Some((-0.0f64).to_bits()));

        let value = loads(br#"{"a":1,"b":2,"a":3}"#, "utf-8").unwrap();
        let keys: Vec<&str> = value
            .as_dict()
            .unwrap()
            .keys()
            .filter_map(|key| key.as_str())
            .collect();
        assert_eq!(keys, ["a", "b"]);
        assert_eq!(value.get("a").and_then(|value| value.as_i64()), Some(3));
    }

    #[test]
    fn empty_and_whitespace_only_bodies() {
        assert_eq!(
            loads(b"", "utf-8").unwrap_err().message(),
            "Expecting value: line 1 column 1 (char 0)"
        );
        assert_eq!(
            loads(b" \t\r\n", "utf-8").unwrap_err().message(),
            "Expecting value: line 2 column 1 (char 4)"
        );
    }
}
