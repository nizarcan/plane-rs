// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The JSON dialect Django's session serializer uses.
//!
//! `django.contrib.sessions.serializers.JSONSerializer`
//! (`django/core/signing.py:142-152`) is
//! `json.dumps(obj, separators=(",", ":")).encode("latin-1")`: **compact**
//! separators, `ensure_ascii=True` (the default), and Python dict insertion
//! order. It is neither [`plane_serde::Dialect::Drf`] (no spaces but
//! `ensure_ascii=False`) nor [`plane_serde::Dialect::Django`] (ASCII but
//! `", "` / `": "` separators), so the port lives here, together with the
//! ordered value type the payload round-trips through.
//!
//! The workspace deliberately does not enable `serde_json`'s `preserve_order`
//! feature (see the workspace `Cargo.toml`), so a `serde_json::Value` object is
//! a `BTreeMap` and would re-order `device_info` alphabetically. This module
//! keeps keys in insertion order, the way Python's `json` does, and keeps
//! integer-vs-float numbers distinct the way `json.loads` does.

use std::fmt;

/// A JSON value with insertion-ordered objects and Python number typing.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// A Python `int` (`json.loads` produces one when the literal has no `.`,
    /// `e` or `E`).
    Int(i64),
    /// A Python `float`.
    Float(f64),
    Str(String),
    Array(Vec<Json>),
    /// Python dict order: a key that is assigned twice keeps its first
    /// position and takes the last value.
    Object(Vec<(String, Json)>),
}

impl Json {
    /// An empty object, the empty session payload.
    pub fn empty_object() -> Self {
        Self::Object(Vec::new())
    }

    /// `obj.get(key)` for objects; `None` for every other type.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Self::Object(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// `obj.get(key)` with a mutable reference, for in-place updates.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        match self {
            Self::Object(entries) => entries
                .iter_mut()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// `key in obj` for objects.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `dict[key] = value`: an existing key keeps its position, a new one is
    /// appended. Other value types are replaced by an object.
    pub fn insert(&mut self, key: impl Into<String>, value: Json) {
        let key = key.into();
        if let Self::Object(entries) = self {
            if let Some((_, existing)) = entries.iter_mut().find(|(name, _)| *name == key) {
                *existing = value;
                return;
            }
            entries.push((key, value));
            return;
        }
        *self = Self::Object(vec![(key, value)]);
    }

    /// `dict.pop(key)`.
    pub fn remove(&mut self, key: &str) -> Option<Json> {
        match self {
            Self::Object(entries) => {
                let index = entries.iter().position(|(name, _)| name == key)?;
                Some(entries.remove(index).1)
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(value) => Some(*value as f64),
            Self::Float(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Self::Object(entries) => Some(entries),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Python truthiness, for the handful of places the session checks
    /// `if expiry:`.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(value) => *value,
            Self::Int(value) => *value != 0,
            Self::Float(value) => *value != 0.0,
            Self::Str(value) => !value.is_empty(),
            Self::Array(values) => !values.is_empty(),
            Self::Object(entries) => !entries.is_empty(),
        }
    }

    /// The `serde_json::Value` a `jsonb` column takes.
    ///
    /// Postgres `jsonb` normalises object key order (sorted by key length, then
    /// bytes), so the ordered representation is not needed on the wire; this is
    /// only used for `sessions.device_info`.
    pub fn to_serde_json(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(value) => serde_json::Value::Bool(*value),
            Self::Int(value) => serde_json::Value::Number((*value).into()),
            Self::Float(value) => serde_json::Number::from_f64(*value)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null),
            Self::Str(value) => serde_json::Value::String(value.clone()),
            Self::Array(values) => {
                serde_json::Value::Array(values.iter().map(Self::to_serde_json).collect())
            }
            Self::Object(entries) => serde_json::Value::Object(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_serde_json()))
                    .collect(),
            ),
        }
    }

    /// The same value as a `json.loads` tree (`plane_serde::pyjson::Value`),
    /// keeping insertion order and integer-vs-float typing.
    ///
    /// `None` when the value has no representation here: a Python `int`
    /// outside `i64` (this type holds `i64`, `pyjson` holds arbitrary
    /// precision) or a string with a lone surrogate (`pyjson` stores WTF-8).
    /// Both can only come from a request body; the caller decides what a value
    /// it cannot render means.
    #[must_use]
    pub fn from_pyjson(value: &plane_serde::pyjson::Value) -> Option<Self> {
        use plane_serde::pyjson::Value as Py;
        Some(match value {
            Py::Null => Self::Null,
            Py::Bool(value) => Self::Bool(*value),
            Py::Int(value) => Self::Int(value.as_i64()?),
            Py::Float(value) => Self::Float(*value),
            Py::Str(value) => Self::Str(value.as_str()?.to_owned()),
            Py::List(values) => Self::Array(
                values
                    .iter()
                    .map(Self::from_pyjson)
                    .collect::<Option<Vec<_>>>()?,
            ),
            Py::Dict(entries) => Self::Object(
                entries
                    .iter()
                    .map(|(key, value)| Some((key.as_str()?.to_owned(), Self::from_pyjson(value)?)))
                    .collect::<Option<Vec<_>>>()?,
            ),
        })
    }
}

/// `json.dumps(value, separators=(",", ":"))` with `ensure_ascii=True`.
///
/// `allow_nan` stays at its default (`True`), so a non-finite float is written
/// as `NaN`, `Infinity` or `-Infinity` — Python's `json` accepts those on the
/// way back in as well.
pub fn to_compact_string(value: &Json) -> String {
    let mut out = String::with_capacity(256);
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Json) {
    match value {
        Json::Null => out.push_str("null"),
        Json::Bool(true) => out.push_str("true"),
        Json::Bool(false) => out.push_str("false"),
        Json::Int(number) => out.push_str(&number.to_string()),
        Json::Float(number) => out.push_str(&python_float_repr(*number)),
        Json::Str(text) => write_string(out, text),
        Json::Array(values) => {
            out.push('[');
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, value);
            }
            out.push(']');
        }
        Json::Object(entries) => {
            out.push('{');
            for (index, (key, value)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_value(out, value);
            }
            out.push('}');
        }
    }
}

/// `json.encoder.py_encode_basestring_ascii`: `"`, `\`, the five short escapes,
/// then every other character outside `0x20..=0x7e` as `\uXXXX` (lowercase
/// hex), with astral characters as a UTF-16 surrogate pair.
fn write_string(out: &mut String, value: &str) {
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            character if (' '..='~').contains(&character) => out.push(character),
            character => write_unicode_escape(out, character),
        }
    }
    out.push('"');
}

fn write_unicode_escape(out: &mut String, character: char) {
    let code = character as u32;
    if code <= 0xffff {
        write_u16_escape(out, code as u16);
        return;
    }
    let adjusted = code - 0x1_0000;
    let high = 0xd800 + (adjusted >> 10);
    let low = 0xdc00 + (adjusted & 0x3ff);
    write_u16_escape(out, high as u16);
    write_u16_escape(out, low as u16);
}

fn write_u16_escape(out: &mut String, value: u16) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    out.push_str("\\u");
    for shift in [12, 8, 4, 0] {
        out.push(DIGITS[usize::from((value >> shift) & 0xf)] as char);
    }
}

/// CPython's `float.__repr__` rules (`repr(float)`), which is what
/// `json.dumps` writes: shortest round-trip digits, positional notation for
/// decimal exponents in `-4..16`, exponent notation otherwise with at least two
/// exponent digits.
fn python_float_repr(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    // `Display` is the shortest representation that round-trips.
    let positional = format!("{value}");
    // `LowerExp` gives the shortest mantissa and the decimal exponent.
    let scientific = format!("{value:e}");
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return ensure_decimal_point(positional);
    };
    let Ok(exponent) = exponent.parse::<i32>() else {
        return ensure_decimal_point(positional);
    };
    if (-4..16).contains(&exponent) {
        return ensure_decimal_point(positional);
    }
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    // CPython writes the shortest mantissa: `1e-05`, not `1.0e-05`.
    let mantissa = mantissa.to_string();
    let exponent_sign = if exponent < 0 { '-' } else { '+' };
    format!("{sign}{mantissa}e{exponent_sign}{:02}", exponent.abs())
}

fn ensure_decimal_point(value: String) -> String {
    if value.contains('.') {
        value
    } else {
        format!("{value}.0")
    }
}

/// Why a session payload could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub message: &'static str,
    pub offset: usize,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for JsonError {}

/// `json.loads(text)`, keeping object order.
///
/// Python's `json` accepts `NaN`, `Infinity` and `-Infinity`, so this does too.
pub fn parse(text: &str) -> Result<Json, JsonError> {
    let mut parser = Parser {
        bytes: text.as_bytes(),
        position: 0,
    };
    parser.skip_whitespace();
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.position != parser.bytes.len() {
        return Err(parser.error("extra data"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl Parser<'_> {
    fn error(&self, message: &'static str) -> JsonError {
        JsonError {
            message,
            offset: self.position,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(byte) = self.peek() {
            if matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
                self.position += 1;
            } else {
                break;
            }
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), JsonError> {
        if self.peek() == Some(byte) {
            self.position += 1;
            Ok(())
        } else {
            Err(self.error("expected character"))
        }
    }

    fn parse_value(&mut self) -> Result<Json, JsonError> {
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => Ok(Json::Str(self.parse_string()?)),
            Some(b't') => self.parse_literal("true", Json::Bool(true)),
            Some(b'f') => self.parse_literal("false", Json::Bool(false)),
            Some(b'n') => self.parse_literal("null", Json::Null),
            Some(b'N') => self.parse_literal("NaN", Json::Float(f64::NAN)),
            Some(b'I') => self.parse_literal("Infinity", Json::Float(f64::INFINITY)),
            Some(b'-') if self.bytes[self.position..].starts_with(b"-Infinity") => {
                self.parse_literal("-Infinity", Json::Float(f64::NEG_INFINITY))
            }
            Some(byte) if byte == b'-' || byte.is_ascii_digit() => self.parse_number(),
            _ => Err(self.error("expected value")),
        }
    }

    fn parse_literal(&mut self, literal: &'static str, value: Json) -> Result<Json, JsonError> {
        if self.bytes[self.position..].starts_with(literal.as_bytes()) {
            self.position += literal.len();
            Ok(value)
        } else {
            Err(self.error("invalid literal"))
        }
    }

    fn parse_object(&mut self) -> Result<Json, JsonError> {
        self.expect(b'{')?;
        let mut entries: Vec<(String, Json)> = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b'}') {
            self.position += 1;
            return Ok(Json::Object(entries));
        }
        loop {
            self.skip_whitespace();
            let key = self.parse_string()?;
            self.skip_whitespace();
            self.expect(b':')?;
            self.skip_whitespace();
            let value = self.parse_value()?;
            match entries.iter_mut().find(|(name, _)| *name == key) {
                Some((_, existing)) => *existing = value,
                None => entries.push((key, value)),
            }
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b'}') => {
                    self.position += 1;
                    return Ok(Json::Object(entries));
                }
                _ => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Json, JsonError> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        self.skip_whitespace();
        if self.peek() == Some(b']') {
            self.position += 1;
            return Ok(Json::Array(values));
        }
        loop {
            self.skip_whitespace();
            values.push(self.parse_value()?);
            self.skip_whitespace();
            match self.peek() {
                Some(b',') => {
                    self.position += 1;
                }
                Some(b']') => {
                    self.position += 1;
                    return Ok(Json::Array(values));
                }
                _ => return Err(self.error("expected ',' or ']'")),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, JsonError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            let Some(byte) = self.peek() else {
                return Err(self.error("unterminated string"));
            };
            match byte {
                b'"' => {
                    self.position += 1;
                    return Ok(out);
                }
                b'\\' => {
                    self.position += 1;
                    self.parse_escape(&mut out)?;
                }
                byte if byte < 0x20 => return Err(self.error("control character in string")),
                _ => {
                    // Copy one UTF-8 scalar at a time so invalid input is
                    // rejected instead of producing a replacement character.
                    let rest = &self.bytes[self.position..];
                    let width = utf8_width(rest[0]).ok_or_else(|| self.error("invalid utf-8"))?;
                    if rest.len() < width {
                        return Err(self.error("invalid utf-8"));
                    }
                    let text = std::str::from_utf8(&rest[..width])
                        .map_err(|_| self.error("invalid utf-8"))?;
                    out.push_str(text);
                    self.position += width;
                }
            }
        }
    }

    fn parse_escape(&mut self, out: &mut String) -> Result<(), JsonError> {
        let Some(byte) = self.peek() else {
            return Err(self.error("unterminated escape"));
        };
        match byte {
            b'"' => {
                out.push('"');
                self.position += 1;
            }
            b'\\' => {
                out.push('\\');
                self.position += 1;
            }
            b'/' => {
                out.push('/');
                self.position += 1;
            }
            b'b' => {
                out.push('\u{08}');
                self.position += 1;
            }
            b'f' => {
                out.push('\u{0c}');
                self.position += 1;
            }
            b'n' => {
                out.push('\n');
                self.position += 1;
            }
            b'r' => {
                out.push('\r');
                self.position += 1;
            }
            b't' => {
                out.push('\t');
                self.position += 1;
            }
            b'u' => {
                self.position += 1;
                let high = self.parse_hex4()?;
                if (0xd800..0xdc00).contains(&high) {
                    // A high surrogate must be followed by a low surrogate.
                    if self.peek() != Some(b'\\')
                        || self.bytes.get(self.position + 1) != Some(&b'u')
                    {
                        return Err(self.error("unpaired surrogate"));
                    }
                    self.position += 2;
                    let low = self.parse_hex4()?;
                    if !(0xdc00..0xe000).contains(&low) {
                        return Err(self.error("unpaired surrogate"));
                    }
                    let code =
                        0x1_0000 + ((u32::from(high) - 0xd800) << 10) + (u32::from(low) - 0xdc00);
                    let character =
                        char::from_u32(code).ok_or_else(|| self.error("invalid codepoint"))?;
                    out.push(character);
                } else if (0xdc00..0xe000).contains(&high) {
                    return Err(self.error("unpaired surrogate"));
                } else {
                    let character = char::from_u32(u32::from(high))
                        .ok_or_else(|| self.error("invalid codepoint"))?;
                    out.push(character);
                }
            }
            _ => return Err(self.error("invalid escape")),
        }
        Ok(())
    }

    fn parse_hex4(&mut self) -> Result<u16, JsonError> {
        let mut value: u16 = 0;
        for _ in 0..4 {
            let Some(byte) = self.peek() else {
                return Err(self.error("truncated \\u escape"));
            };
            let digit = match byte {
                b'0'..=b'9' => u16::from(byte - b'0'),
                b'a'..=b'f' => u16::from(byte - b'a') + 10,
                b'A'..=b'F' => u16::from(byte - b'A') + 10,
                _ => return Err(self.error("invalid hex digit")),
            };
            value = value * 16 + digit;
            self.position += 1;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Json, JsonError> {
        let start = self.position;
        if self.peek() == Some(b'-') {
            self.position += 1;
        }
        let mut is_float = false;
        while let Some(byte) = self.peek() {
            match byte {
                b'0'..=b'9' => self.position += 1,
                b'.' | b'e' | b'E' | b'+' | b'-' => {
                    is_float = true;
                    self.position += 1;
                }
                _ => break,
            }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.position])
            .map_err(|_| self.error("invalid number"))?;
        if text.is_empty() || text == "-" {
            return Err(JsonError {
                message: "invalid number",
                offset: start,
            });
        }
        if is_float {
            let value: f64 = text.parse().map_err(|_| JsonError {
                message: "invalid number",
                offset: start,
            })?;
            Ok(Json::Float(value))
        } else {
            let value: i64 = text.parse().map_err(|_| JsonError {
                message: "integer out of range",
                offset: start,
            })?;
            Ok(Json::Int(value))
        }
    }
}

fn utf8_width(first: u8) -> Option<usize> {
    match first {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(entries: &[(&str, Json)]) -> Json {
        Json::Object(
            entries
                .iter()
                .map(|(key, value)| ((*key).to_string(), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn compact_ascii_output() {
        let value = object(&[
            ("_auth_user_id", Json::Str("0f1e".into())),
            ("_session_expiry", Json::Int(3600)),
            (
                "device_info",
                object(&[("user_agent", Json::Str("Zoë".into()))]),
            ),
        ]);
        assert_eq!(
            to_compact_string(&value),
            r#"{"_auth_user_id":"0f1e","_session_expiry":3600,"device_info":{"user_agent":"Zo\u00eb"}}"#
        );
    }

    #[test]
    fn python_string_escaping() {
        let value = Json::Str("a\"b\\c\nd\te\u{08}f\u{0c}g\u{7f}h — 日本語 🚀".into());
        assert_eq!(
            to_compact_string(&value),
            "\"a\\\"b\\\\c\\nd\\te\\bf\\fg\\u007fh \\u2014 \\u65e5\\u672c\\u8a9e \\ud83d\\ude80\""
        );
    }

    #[test]
    fn a_json_loads_tree_converts_without_losing_order_or_number_type() {
        let parsed = plane_serde::pyjson::loads(
            br#"{"email": "a@b", "role": 5, "score": 1.0, "tags": [true, null]}"#,
            "utf-8",
        )
        .expect("parse");
        let value = Json::from_pyjson(&parsed).expect("representable");
        assert_eq!(
            to_compact_string(&value),
            r#"{"email":"a@b","role":5,"score":1.0,"tags":[true,null]}"#
        );
    }

    #[test]
    fn an_unrepresentable_tree_is_none() {
        // A Python `int` beyond `i64`.
        let parsed =
            plane_serde::pyjson::loads(b"123456789012345678901234567890", "utf-8").expect("parse");
        assert!(Json::from_pyjson(&parsed).is_none());
    }

    // HTTP_CONTRACT Appendix B.1: the small session, non-ASCII session and the
    // field order of the full one.
    #[test]
    fn round_trips_appendices() {
        let full = object(&[
            (
                "_auth_user_id",
                Json::Str("0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0".into()),
            ),
            (
                "backend",
                Json::Str("django.contrib.auth.backends.ModelBackend".into()),
            ),
            (
                "_auth_user_hash",
                Json::Str(
                    "9f2f4b1d3e0c8a7b6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c".into(),
                ),
            ),
            ("_session_expiry", Json::Int(3600)),
            (
                "device_info",
                object(&[
                    (
                        "user_agent",
                        Json::Str("Mozilla/5.0 (contract probe)".into()),
                    ),
                    ("ip_address", Json::Str("203.0.113.7".into())),
                    ("domain", Json::Str("http://localhost/".into())),
                ]),
            ),
        ]);
        let text = to_compact_string(&full);
        assert_eq!(text.len(), 334);
        assert_eq!(parse(&text).expect("parse"), full);

        let non_ascii = object(&[
            ("name", Json::Str("Zoë — 日本語".into())),
            ("emoji", Json::Str("🚀".into())),
            ("sep", Json::Str("\u{2028}".into())),
        ]);
        assert_eq!(
            to_compact_string(&non_ascii),
            "{\"name\":\"Zo\\u00eb \\u2014 \\u65e5\\u672c\\u8a9e\",\"emoji\":\"\\ud83d\\ude80\",\"sep\":\"\\u2028\"}"
        );
        assert_eq!(
            parse(&to_compact_string(&non_ascii)).expect("parse"),
            non_ascii
        );
    }

    #[test]
    fn python_float_repr_rules() {
        assert_eq!(python_float_repr(1.0), "1.0");
        assert_eq!(python_float_repr(100.0), "100.0");
        assert_eq!(python_float_repr(0.5), "0.5");
        assert_eq!(python_float_repr(1e-5), "1e-05");
        assert_eq!(python_float_repr(1e-4), "0.0001");
        assert_eq!(python_float_repr(1e16), "1e+16");
        assert_eq!(python_float_repr(1.5e300), "1.5e+300");
        assert_eq!(python_float_repr(-2.5e-7), "-2.5e-07");
        assert_eq!(python_float_repr(f64::NAN), "NaN");
        assert_eq!(python_float_repr(f64::INFINITY), "Infinity");
        assert_eq!(python_float_repr(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn parses_int_and_float_distinctly() {
        assert_eq!(parse("3600").expect("int"), Json::Int(3600));
        assert_eq!(parse("3600.0").expect("float"), Json::Float(3600.0));
        assert_eq!(parse("1e3").expect("float"), Json::Float(1000.0));
        assert_eq!(parse("-1").expect("int"), Json::Int(-1));
    }

    #[test]
    fn duplicate_keys_keep_first_position_last_value() {
        let parsed = parse(r#"{"a":1,"b":2,"a":3}"#).expect("parse");
        assert_eq!(parsed, object(&[("a", Json::Int(3)), ("b", Json::Int(2))]));
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "",
            "{",
            "{\"a\"}",
            "[1,]",
            "tru",
            "\"\\ud800\"",
            "1 2",
            "{\"a\":\"\\q\"}",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn python_truthiness() {
        assert!(!Json::Null.is_truthy());
        assert!(!Json::Int(0).is_truthy());
        assert!(!Json::Str(String::new()).is_truthy());
        assert!(Json::Int(1).is_truthy());
        assert!(Json::Str("x".into()).is_truthy());
    }
}
