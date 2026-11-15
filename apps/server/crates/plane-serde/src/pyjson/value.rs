// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The value tree `json.loads` returns.
//!
//! `json.loads` returns only `None`, `bool`, `int`, `float`, `str`, `list`
//! and `dict`. Two of those have no direct Rust equivalent:
//!
//! * `int` is arbitrary precision. [`PyInt`] stores values that fit in an
//!   `i64` inline and everything else as a sign plus decimal digits. The
//!   parser enforces CPython's `sys.int_max_str_digits` limit (4300 digits),
//!   so the digit string is bounded.
//! * `str` may contain lone surrogates produced by a `\uD800` escape
//!   (`Modules/_json.c`, `scanstring_unicode`). [`PyStr`] stores WTF-8, which
//!   is injective, so `PyStr` equality is Python string equality.
//!
//! A `dict` keeps insertion order and applies Python's duplicate-key rule: the
//! first occurrence keeps its position, the last occurrence supplies the
//! value (`PyDict_SetItem` in `Modules/_json.c`, `_parse_object_unicode`).
//!
//! Serialising a [`Value`] through [`crate::to_vec`] produces the bytes DRF's
//! renderer produces for the same Python object, including its failures: a
//! `float` that overflowed to infinity is rejected under [`crate::Dialect::Drf`]
//! (`allow_nan=False`), and a lone surrogate reaches the UTF-8 encoder and
//! raises `UnicodeEncodeError` with the same message and document position.

use std::borrow::Cow;
use std::fmt;
use std::mem;

use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, Serializer};

use crate::wtf8;

/// `sys.int_max_str_digits` as CPython 3.12 sets it.
///
/// `int()` and `str(int)` refuse conversions of more than this many decimal
/// digits unless the limit is changed at runtime
/// (`Python/sysmodule.c`, `longobject.c:long_from_string_with_base`).
pub const INT_MAX_STR_DIGITS: usize = 4300;

/// The private marker serde_json uses for an arbitrary-precision number.
///
/// `PyInt::serialize` emits a one-field struct with this name, which
/// serde_json (with the `arbitrary_precision` feature, as the differential
/// harness builds it) and this crate's serialiser both write as a bare
/// number. Without that feature serde_json would write an object instead, so
/// a `PyInt::Big` must not be handed to a plain serde_json serialiser.
pub(crate) const RAW_NUMBER_TOKEN: &str = "$serde_json::private::Number";

/// The private marker used to hand a WTF-8 string to this crate's serialiser.
///
/// `serialize_str` takes a `&str` and therefore cannot carry a lone
/// surrogate; the payload is sent as bytes instead.
pub(crate) const WTF8_TOKEN: &str = "$plane_serde::private::Wtf8";

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// A value `json.loads` can return.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// Python `None`.
    Null,
    /// Python `bool`.
    Bool(bool),
    /// Python `int`.
    Int(PyInt),
    /// Python `float`.
    Float(f64),
    /// Python `str`.
    Str(PyStr),
    /// Python `list`.
    List(Vec<Value>),
    /// Python `dict`.
    Dict(PyDict),
}

impl Value {
    /// Whether this is Python `None`.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// The value of a `bool`.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The value of an `int` that fits in an `i64`.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(value) => value.as_i64(),
            _ => None,
        }
    }

    /// The value of an `int` that fits in a `u64`.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Int(value) => value.as_u64(),
            _ => None,
        }
    }

    /// The `int` itself.
    #[must_use]
    pub const fn as_int(&self) -> Option<&PyInt> {
        match self {
            Value::Int(value) => Some(value),
            _ => None,
        }
    }

    /// The value of a `float`.
    #[must_use]
    pub const fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Float(value) => Some(*value),
            _ => None,
        }
    }

    /// The text of a `str` that contains no lone surrogates (the common case).
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(value) => value.as_str(),
            _ => None,
        }
    }

    /// The `str` itself, including lone surrogates.
    #[must_use]
    pub const fn as_pystr(&self) -> Option<&PyStr> {
        match self {
            Value::Str(value) => Some(value),
            _ => None,
        }
    }

    /// The items of a `list`.
    #[must_use]
    pub fn as_list(&self) -> Option<&[Value]> {
        match self {
            Value::List(items) => Some(items),
            _ => None,
        }
    }

    /// The `dict`.
    #[must_use]
    pub const fn as_dict(&self) -> Option<&PyDict> {
        match self {
            Value::Dict(dict) => Some(dict),
            _ => None,
        }
    }

    /// Consume the value and return its list items.
    #[must_use]
    pub fn into_list(mut self) -> Option<Vec<Value>> {
        match &mut self {
            Value::List(items) => Some(mem::take(items)),
            _ => None,
        }
    }

    /// Consume the value and return its dict.
    #[must_use]
    pub fn into_dict(mut self) -> Option<PyDict> {
        match &mut self {
            Value::Dict(dict) => Some(mem::take(dict)),
            _ => None,
        }
    }

    /// Consume the value and return its text, rejecting lone surrogates.
    #[must_use]
    pub fn into_string(self) -> Option<String> {
        self.into_pystr().and_then(PyStr::into_string)
    }

    /// Consume the value and return its string.
    #[must_use]
    pub fn into_pystr(mut self) -> Option<PyStr> {
        match &mut self {
            Value::Str(text) => Some(mem::take(text)),
            _ => None,
        }
    }

    /// The value of an object member, or `None` for any other type.
    ///
    /// Python's `dict.get` compares `str` keys by code point, which is byte
    /// equality on WTF-8.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_dict()?.get(key)
    }

    /// The value of an object member whose key contains a lone surrogate.
    #[must_use]
    pub fn get_wtf8(&self, key: &[u8]) -> Option<&Value> {
        self.as_dict()?.get_wtf8(key)
    }

    /// The `index`th item of a list.
    #[must_use]
    pub fn index(&self, index: usize) -> Option<&Value> {
        self.as_list()?.get(index)
    }

    /// The number of items in a list or members in an object.
    #[must_use]
    pub fn len(&self) -> Option<usize> {
        match self {
            Value::List(items) => Some(items.len()),
            Value::Dict(dict) => Some(dict.len()),
            Value::Str(text) => Some(text.char_len()),
            _ => None,
        }
    }

    /// Whether a list, object or string is empty; `false` for scalars.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Value::List(items) => items.is_empty(),
            Value::Dict(dict) => dict.is_empty(),
            Value::Str(text) => text.is_empty(),
            _ => false,
        }
    }

    /// Whether this value is a list or an object.
    #[must_use]
    pub const fn is_container(&self) -> bool {
        matches!(self, Value::List(_) | Value::Dict(_))
    }

    /// The items of a list.
    pub fn list_iter(&self) -> impl Iterator<Item = &Value> {
        self.as_list().unwrap_or_default().iter()
    }

    /// The members of an object in insertion order.
    pub fn dict_iter(&self) -> impl Iterator<Item = (&PyStr, &Value)> {
        self.as_dict().into_iter().flat_map(PyDict::iter)
    }

    /// Walk a path of object keys.
    #[must_use]
    pub fn path(&self, keys: &[&str]) -> Option<&Value> {
        let mut current = self;
        for key in keys {
            current = current.get(key)?;
        }
        Some(current)
    }

    fn take_children(&mut self, out: &mut Vec<Value>) {
        match self {
            Value::List(items) => out.append(items),
            Value::Dict(dict) => {
                for (_, value) in dict.entries.drain(..) {
                    out.push(value);
                }
            }
            _ => {}
        }
    }

    fn has_container_child(&self) -> bool {
        match self {
            Value::List(items) => items.iter().any(Value::is_container),
            Value::Dict(dict) => dict.entries.iter().any(|(_, value)| value.is_container()),
            _ => false,
        }
    }
}

/// `json.loads` can nest up to the decoder's C recursion limit, which is far
/// deeper than a recursive Rust destructor can safely handle on a 2 MiB
/// thread stack (a request handler runs on one). Drop the tree with an
/// explicit work list instead.
///
/// The fast path (children are all scalars, the usual request body) does no
/// allocation: the compiler-generated drop glue for those children is one
/// level deep.
///
/// `Clone`, `Debug` and `PartialEq` are still recursive (they are derived),
/// so a handler that clones or prints a value nested thousands of levels deep
/// needs a thread with room for it. Parsing and dropping do not.
impl Drop for Value {
    fn drop(&mut self) {
        if !self.has_container_child() {
            return;
        }
        let mut pending = Vec::new();
        self.take_children(&mut pending);
        while let Some(mut value) = pending.pop() {
            if value.has_container_child() {
                value.take_children(&mut pending);
            }
        }
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(value) => serializer.serialize_bool(*value),
            Value::Int(value) => value.serialize(serializer),
            Value::Float(value) => serializer.serialize_f64(*value),
            Value::Str(value) => value.serialize(serializer),
            Value::List(items) => {
                let mut sequence = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    sequence.serialize_element(item)?;
                }
                sequence.end()
            }
            Value::Dict(dict) => {
                let mut map = serializer.serialize_map(Some(dict.entries.len()))?;
                for (key, value) in &dict.entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Value::Bool(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Value::Int(PyInt::Small(value))
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Value::Float(value)
    }
}

impl From<&str> for Value {
    fn from(value: &str) -> Self {
        Value::Str(PyStr::new(value))
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Value::Str(PyStr::from_string(value))
    }
}

impl From<Vec<Value>> for Value {
    fn from(value: Vec<Value>) -> Self {
        Value::List(value)
    }
}

impl From<PyDict> for Value {
    fn from(value: PyDict) -> Self {
        Value::Dict(value)
    }
}

// ---------------------------------------------------------------------------
// int
// ---------------------------------------------------------------------------

/// A Python `int`.
///
/// Values in the `i64` range are stored inline; the rest keep their decimal
/// digits. There is no upper bound beyond the digit limit `json.loads`
/// enforces.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PyInt {
    /// A value that fits in an `i64`.
    Small(i64),
    /// A value outside the `i64` range.
    Big(BigInt),
}

/// The arbitrary-precision arm of [`PyInt`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BigInt {
    negative: bool,
    /// Decimal digits with no leading zero. Never `"0"`.
    digits: Box<str>,
}

impl BigInt {
    /// Build a big integer from a sign and decimal digits, normalising
    /// leading zeros. Returns `None` for an empty digit string.
    #[must_use]
    pub fn from_decimal(negative: bool, digits: &str) -> Option<Self> {
        let trimmed = digits.trim_start_matches('0');
        if trimmed.is_empty() {
            return None;
        }
        Some(Self {
            negative,
            digits: trimmed.into(),
        })
    }

    /// Whether the value is negative.
    #[must_use]
    pub const fn is_negative(&self) -> bool {
        self.negative
    }

    /// The decimal digits, without a sign.
    #[must_use]
    pub fn digits(&self) -> &str {
        &self.digits
    }

    /// The decimal representation, with a sign when negative.
    #[must_use]
    pub fn to_decimal_string(&self) -> String {
        let mut out = String::with_capacity(self.digits.len() + usize::from(self.negative));
        if self.negative {
            out.push('-');
        }
        out.push_str(&self.digits);
        out
    }
}

impl PyInt {
    /// The `i64` value, if it fits.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PyInt::Small(value) => Some(*value),
            PyInt::Big(value) => value
                .digits()
                .parse::<i128>()
                .ok()
                .map(|magnitude| {
                    if value.is_negative() {
                        -magnitude
                    } else {
                        magnitude
                    }
                })
                .and_then(|value| i64::try_from(value).ok()),
        }
    }

    /// The `u64` value, if it fits and is not negative.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            PyInt::Small(value) => u64::try_from(*value).ok(),
            PyInt::Big(value) if !value.is_negative() => value.digits().parse::<u64>().ok(),
            PyInt::Big(_) => None,
        }
    }

    /// Whether the value is negative. Zero is not negative.
    #[must_use]
    pub fn is_negative(&self) -> bool {
        match self {
            PyInt::Small(value) => *value < 0,
            PyInt::Big(value) => value.is_negative(),
        }
    }

    /// The decimal representation Python's `str()` would produce.
    #[must_use]
    pub fn to_decimal_string(&self) -> String {
        match self {
            PyInt::Small(value) => value.to_string(),
            PyInt::Big(value) => value.to_decimal_string(),
        }
    }
}

impl fmt::Display for PyInt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PyInt::Small(value) => write!(f, "{value}"),
            PyInt::Big(value) => {
                if value.is_negative() {
                    f.write_str("-")?;
                }
                f.write_str(value.digits())
            }
        }
    }
}

impl Serialize for PyInt {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PyInt::Small(value) => serializer.serialize_i64(*value),
            PyInt::Big(value) => {
                let text = value.to_decimal_string();
                let mut structure = serializer.serialize_struct(RAW_NUMBER_TOKEN, 1)?;
                serde::ser::SerializeStruct::serialize_field(
                    &mut structure,
                    RAW_NUMBER_TOKEN,
                    &text,
                )?;
                serde::ser::SerializeStruct::end(structure)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// str
// ---------------------------------------------------------------------------

/// A Python `str`, stored as WTF-8.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct PyStr {
    bytes: Box<[u8]>,
}

/// Returned when bytes are not valid WTF-8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wtf8Error;

impl fmt::Display for Wtf8Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("string is not valid WTF-8")
    }
}

impl std::error::Error for Wtf8Error {}

impl PyStr {
    /// Wrap text that is valid UTF-8.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            bytes: text.as_bytes().into(),
        }
    }

    /// Wrap an owned `String`.
    #[must_use]
    pub fn from_string(text: String) -> Self {
        Self {
            bytes: text.into_bytes().into_boxed_slice(),
        }
    }

    /// Wrap bytes that may contain lone surrogates in WTF-8 form.
    pub fn from_wtf8(bytes: Vec<u8>) -> Result<Self, Wtf8Error> {
        if wtf8::is_valid(&bytes) {
            Ok(Self {
                bytes: bytes.into_boxed_slice(),
            })
        } else {
            Err(Wtf8Error)
        }
    }

    /// The WTF-8 bytes.
    #[must_use]
    pub fn as_wtf8(&self) -> &[u8] {
        &self.bytes
    }

    /// The text, or `None` when a lone surrogate is present.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.bytes).ok()
    }

    /// Whether the string contains a lone surrogate.
    #[must_use]
    pub fn has_surrogates(&self) -> bool {
        std::str::from_utf8(&self.bytes).is_err()
    }

    /// The text with unpaired surrogates replaced by U+FFFD.
    #[must_use]
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        match std::str::from_utf8(&self.bytes) {
            Ok(text) => Cow::Borrowed(text),
            Err(_) => {
                let mut out = String::with_capacity(self.bytes.len());
                for code_point in self.chars() {
                    match char::from_u32(code_point) {
                        Some(ch) => out.push(ch),
                        None => out.push('\u{FFFD}'),
                    }
                }
                Cow::Owned(out)
            }
        }
    }

    /// The code points of the string, including surrogates.
    pub fn chars(&self) -> impl Iterator<Item = u32> + '_ {
        wtf8::Chars::new(&self.bytes)
    }

    /// The length Python's `len()` reports.
    #[must_use]
    pub fn char_len(&self) -> usize {
        if self.bytes.is_ascii() {
            return self.bytes.len();
        }
        match self.as_str() {
            Some(text) => text.chars().count(),
            None => self.chars().count(),
        }
    }

    /// Whether the string is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Consume the string and return a `String` when no surrogate is present.
    #[must_use]
    pub fn into_string(self) -> Option<String> {
        String::from_utf8(self.bytes.into_vec()).ok()
    }
}

impl fmt::Debug for PyStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("\"")?;
        for code_point in self.chars() {
            match char::from_u32(code_point) {
                Some(ch) => write!(f, "{}", ch.escape_debug())?,
                None => write!(f, "\\u{code_point:04x}")?,
            }
        }
        f.write_str("\"")
    }
}

impl fmt::Display for PyStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl From<&str> for PyStr {
    fn from(value: &str) -> Self {
        PyStr::new(value)
    }
}

impl From<String> for PyStr {
    fn from(value: String) -> Self {
        PyStr::from_string(value)
    }
}

/// Payload for [`WTF8_TOKEN`]: a string Python allows but Rust's `str` does
/// not.
struct Wtf8Payload<'a>(&'a [u8]);

impl Serialize for Wtf8Payload<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(self.0)
    }
}

impl Serialize for PyStr {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.as_str() {
            Some(text) => serializer.serialize_str(text),
            None => serializer.serialize_newtype_struct(WTF8_TOKEN, &Wtf8Payload(&self.bytes)),
        }
    }
}

// ---------------------------------------------------------------------------
// dict
// ---------------------------------------------------------------------------

/// A Python `dict` with insertion order.
///
/// A duplicate key keeps the position of its first occurrence and takes the
/// value of its last one.
///
/// Lookups are a linear scan. A request body's objects have a handful of
/// members, and a scan over `(key length, bytes)` beats hashing there: it
/// keeps `PyDict` to one `Vec` (so a `Value` is 32 bytes instead of 112, and
/// parsing moves values instead of copying a hundred bytes each time).
#[derive(Clone, Default)]
pub struct PyDict {
    entries: Vec<(PyStr, Value)>,
}

impl PyDict {
    /// An empty object.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty object with room for `capacity` members.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
        }
    }

    /// Insert a member, returning the value it replaced.
    pub fn insert(&mut self, key: PyStr, value: Value) -> Option<Value> {
        if let Some(position) = self
            .entries
            .iter()
            .position(|(existing, _)| *existing == key)
        {
            return Some(mem::replace(&mut self.entries[position].1, value));
        }
        self.entries.push((key, value));
        None
    }

    /// The value stored under a `str` key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.get_wtf8(key.as_bytes())
    }

    /// The value stored under a key that may contain lone surrogates.
    #[must_use]
    pub fn get_wtf8(&self, key: &[u8]) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(existing, _)| existing.as_wtf8() == key)
            .map(|(_, value)| value)
    }

    /// Whether the key is present.
    #[must_use]
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// The number of members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the object has no members.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The members in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&PyStr, &Value)> {
        self.entries.iter().map(|(key, value)| (key, value))
    }

    /// The keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &PyStr> {
        self.entries.iter().map(|(key, _)| key)
    }

    /// The values in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.entries.iter().map(|(_, value)| value)
    }

    /// The members, consuming the object.
    #[must_use]
    pub fn into_entries(mut self) -> Vec<(PyStr, Value)> {
        mem::take(&mut self.entries)
    }
}

impl PartialEq for PyDict {
    fn eq(&self, other: &Self) -> bool {
        self.entries == other.entries
    }
}

impl fmt::Debug for PyDict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|(key, value)| (key, value)))
            .finish()
    }
}

impl FromIterator<(PyStr, Value)> for PyDict {
    fn from_iter<T: IntoIterator<Item = (PyStr, Value)>>(iter: T) -> Self {
        let mut dict = Self::new();
        for (key, value) in iter {
            dict.insert(key, value);
        }
        dict
    }
}

#[cfg(test)]
mod tests {
    use super::{BigInt, PyDict, PyInt, PyStr, Value};

    #[test]
    fn duplicate_keys_keep_the_first_position_and_the_last_value() {
        let mut dict = PyDict::new();
        for (key, value) in [("a", 1), ("b", 2), ("a", 3)] {
            dict.insert(PyStr::new(key), Value::from(i64::from(value)));
        }
        let keys: Vec<&str> = dict.keys().filter_map(PyStr::as_str).collect();
        assert_eq!(keys, ["a", "b"]);
        assert_eq!(dict.get("a").and_then(Value::as_i64), Some(3));
    }

    #[test]
    fn integers_convert_and_print_like_python() {
        assert_eq!(PyInt::Small(0).to_string(), "0");
        let big =
            PyInt::Big(BigInt::from_decimal(true, "000123456789012345678901234567890").unwrap());
        assert_eq!(big.to_decimal_string(), "-123456789012345678901234567890");
        assert_eq!(big.as_i64(), None);
        assert_eq!(PyInt::Small(i64::MIN).as_i64(), Some(i64::MIN));
        assert!(big.is_negative());
    }

    #[test]
    fn strings_round_trip_wtf8() {
        let text = PyStr::from_wtf8(vec![b'"', 0xED, 0xA0, 0x80, b'"']).unwrap();
        assert!(text.has_surrogates());
        assert_eq!(text.as_str(), None);
        assert_eq!(text.to_string_lossy(), "\"\u{FFFD}\"");
        assert_eq!(text.char_len(), 3);
        assert_eq!(text.chars().collect::<Vec<_>>(), [0x22, 0xD800, 0x22]);
        assert!(PyStr::from_wtf8(vec![0xFF]).is_err());
    }
}
