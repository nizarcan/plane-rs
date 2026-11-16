// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The Python value tree django-redis can store.
//!
//! `PickleSerializer` pickles an arbitrary Python object, but Plane only ever
//! stores two shapes: the throttle histories (`list[float]`) and
//! `{"data": response.data, "status": 200}` from `plane/utils/cache.py`. The
//! second one is a DRF serializer's `.data`, which is a `ReturnList`/`ReturnDict`
//! of plain JSON values **plus** `uuid.UUID` objects for every primary key
//! (`PrimaryKeyRelatedField.to_representation` returns `value.pk`, not a
//! string) and, for `Response.values()`-style handlers, `datetime` objects.
//! All of them can appear in a cached payload, so all of them are modelled
//! here.
//!
//! # Object identity
//!
//! CPython's pickler memoises by **object identity** (`id(obj)`), so
//! `pickle.dumps` writes a second reference to the same `str`/`list`/`dict`/
//! `UUID` as a `BINGET`. A DRF serializer reuses one `str` object per field
//! name across every row, which is visible in the bytes: the second row's
//! `"id"` key is `h\x0c` (`BINGET 12`), not another `SHORT_BINUNICODE`. The
//! codec therefore keys its memo on the `Arc` inside each node: cloning a
//! `PyValue` shares the object, building an equal one with a separate
//! constructor does not. Decoding never loses information (values are compared
//! structurally) but it cannot recover sharing that was not in the pickle.
//!
//! # `ReturnDict` / `ReturnList`
//!
//! `rest_framework.utils.serializer_helpers.ReturnDict.__reduce__` returns
//! `(dict, (dict(self),))` and `ReturnList.__reduce__` returns
//! `(list, (list(self),))`. A top-level `serializer.data` therefore pickles as
//! `GLOBAL builtins.list` + the plain list + `REDUCE`, not as a bare list. That
//! is what [`PyValue::return_list`] and [`PyValue::return_dict`] build, and it
//! is what a handler that stores `response.data` must use to be byte-identical
//! to Django; a plain [`PyValue::list`] round-trips to the same value but not
//! to the same bytes.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use chrono::{FixedOffset, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone, Utc};
use serde::Serialize;
use serde::ser::{SerializeMap, SerializeSeq, Serializer};
use uuid::Uuid;

use plane_serde::PyDecimal;
use plane_serde::pyjson::{BigInt, PyInt as JsonPyInt, PyStr};
use plane_serde::values::{
    DrfDateTime, DrfNaiveDateTime, EncoderDecimal, PyDate, PyMapKey, PyTime, PyTimedelta,
};

use crate::pyint::PyInt;

/// `datetime.timezone(timedelta(seconds))`.
///
/// `timezone.utc` is a singleton in CPython and is what `timezone(timedelta(0))`
/// returns, so it is one variant here rather than an offset of zero. A named
/// timezone (`timezone(timedelta(hours=1), "X")`) reduces to a two-element
/// argument tuple and is not modelled; nothing in Plane produces one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PyTz {
    /// `datetime.timezone.utc`.
    Utc,
    /// A fixed offset from UTC, in seconds. CPython requires whole minutes and
    /// an absolute value below one day.
    Offset(i32),
}

impl PyTz {
    /// The offset from UTC in seconds.
    #[must_use]
    pub fn offset_seconds(self) -> i32 {
        match self {
            PyTz::Utc => 0,
            PyTz::Offset(seconds) => seconds,
        }
    }
}

/// A Python `datetime.datetime`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PyDateTime {
    /// The wall-clock value, without a timezone.
    pub naive: NaiveDateTime,
    /// `tzinfo`, when the datetime is aware.
    pub tz: Option<Arc<PyTz>>,
    /// PEP 495 `fold`.
    pub fold: bool,
}

/// A Python `dict`, insertion-ordered, with arbitrary keys.
///
/// `Vec` rather than a map: cached payloads have a handful of members, a
/// `dict` key is any hashable Python value, and the encoder walks it in
/// insertion order anyway.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PyMapping {
    /// Members in insertion order.
    pub entries: Vec<(PyValue, PyValue)>,
}

impl PyMapping {
    /// An empty mapping.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The value stored under a `str` key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&PyValue> {
        self.entries.iter().find_map(|(existing, value)| {
            if existing.as_str() == Some(key) {
                Some(value)
            } else {
                None
            }
        })
    }

    /// Insert a member, keeping the position of an existing key.
    pub fn insert(&mut self, key: PyValue, value: PyValue) {
        if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|(existing, _)| *existing == key)
        {
            entry.1 = value;
            return;
        }
        self.entries.push((key, value));
    }

    /// The number of members.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the mapping is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// The builtin `__reduce__` of DRF's `ReturnList` / `ReturnDict`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReduceFunc {
    /// `rest_framework.utils.serializer_helpers.ReturnList`.
    List,
    /// `rest_framework.utils.serializer_helpers.ReturnDict`.
    Dict,
}

impl ReduceFunc {
    /// The Python module the callable lives in.
    #[must_use]
    pub fn module(self) -> &'static str {
        "builtins"
    }

    /// The callable's `__qualname__` last component.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            ReduceFunc::List => "list",
            ReduceFunc::Dict => "dict",
        }
    }
}

/// `builtins.list(x)` / `builtins.dict(x)`: what a DRF `ReturnList`/`ReturnDict`
/// reduces to.
#[derive(Clone, Debug, PartialEq)]
pub struct PyReduce {
    /// The builtin to call.
    pub func: ReduceFunc,
    /// Its single argument.
    pub value: PyValue,
}

/// A value django-redis can hold.
///
/// Every container is an `Arc` so that two references to the same Python
/// object share one pointer, which is what the pickler's memo needs. Scalars
/// that CPython never memoises (`None`, `bool`, `int`, `float`) are stored
/// inline.
#[derive(Clone, Debug, PartialEq)]
pub enum PyValue {
    /// `None`.
    None,
    /// `bool`.
    Bool(bool),
    /// `int`.
    Int(PyInt),
    /// `float`.
    Float(f64),
    /// `str`, as WTF-8 so a lone surrogate survives.
    Str(Arc<PyStr>),
    /// `bytes`.
    Bytes(Arc<Vec<u8>>),
    /// `list`.
    List(Arc<Vec<PyValue>>),
    /// `tuple`.
    Tuple(Arc<Vec<PyValue>>),
    /// `set`, in iteration order.
    Set(Arc<Vec<PyValue>>),
    /// `frozenset`, in iteration order.
    FrozenSet(Arc<Vec<PyValue>>),
    /// `dict`.
    Dict(Arc<PyMapping>),
    /// `uuid.UUID`.
    Uuid(Arc<Uuid>),
    /// `datetime.date`.
    Date(Arc<NaiveDate>),
    /// `datetime.datetime`.
    DateTime(Arc<PyDateTime>),
    /// `datetime.time` (always naive: DRF refuses an aware time).
    Time(Arc<NaiveTime>),
    /// `datetime.timedelta`.
    TimeDelta(Arc<TimeDelta>),
    /// `datetime.timezone` (only nested inside an aware datetime, but a
    /// payload could hold one).
    TimeZone(Arc<PyTz>),
    /// `decimal.Decimal`.
    Decimal(Arc<PyDecimal>),
    /// `builtins.list(...)` / `builtins.dict(...)`.
    Reduce(Arc<PyReduce>),
}

impl PyValue {
    /// `None`.
    #[must_use]
    pub const fn none() -> Self {
        PyValue::None
    }
    /// `bool`.
    #[must_use]
    pub const fn bool(value: bool) -> Self {
        PyValue::Bool(value)
    }

    /// `int`.
    #[must_use]
    pub fn int(value: i64) -> Self {
        PyValue::Int(PyInt::from_i64(value))
    }

    /// `int` from an unsigned value.
    #[must_use]
    pub fn uint(value: u64) -> Self {
        PyValue::Int(PyInt::from_u64(value))
    }

    /// `float`.
    #[must_use]
    pub const fn float(value: f64) -> Self {
        PyValue::Float(value)
    }

    /// `str`.
    #[must_use]
    pub fn string(value: impl Into<String>) -> Self {
        PyValue::Str(Arc::new(PyStr::from_string(value.into())))
    }

    /// `bytes`.
    #[must_use]
    pub fn bytes(value: impl Into<Vec<u8>>) -> Self {
        PyValue::Bytes(Arc::new(value.into()))
    }

    /// `list`.
    #[must_use]
    pub fn list(values: Vec<PyValue>) -> Self {
        PyValue::List(Arc::new(values))
    }

    /// `tuple`.
    #[must_use]
    pub fn tuple(values: Vec<PyValue>) -> Self {
        PyValue::Tuple(Arc::new(values))
    }

    /// `dict` from string keys.
    #[must_use]
    pub fn dict(values: Vec<(&str, PyValue)>) -> Self {
        Self::mapping(
            values
                .into_iter()
                .map(|(key, value)| (PyValue::string(key), value))
                .collect(),
        )
    }

    /// `dict` from arbitrary key/value pairs.
    #[must_use]
    pub fn mapping(entries: Vec<(PyValue, PyValue)>) -> Self {
        PyValue::Dict(Arc::new(PyMapping { entries }))
    }

    /// `uuid.UUID`.
    #[must_use]
    pub fn uuid(value: Uuid) -> Self {
        PyValue::Uuid(Arc::new(value))
    }

    /// `datetime.date`.
    #[must_use]
    pub fn date(value: NaiveDate) -> Self {
        PyValue::Date(Arc::new(value))
    }

    /// A naive `datetime.datetime`.
    #[must_use]
    pub fn datetime(value: NaiveDateTime) -> Self {
        PyValue::DateTime(Arc::new(PyDateTime {
            naive: value,
            tz: None,
            fold: false,
        }))
    }

    /// An aware `datetime.datetime`.
    #[must_use]
    pub fn aware_datetime(value: NaiveDateTime, tz: PyTz) -> Self {
        PyValue::DateTime(Arc::new(PyDateTime {
            naive: value,
            tz: Some(Arc::new(tz)),
            fold: false,
        }))
    }

    /// `decimal.Decimal` from its literal text.
    #[must_use]
    pub fn decimal(value: PyDecimal) -> Self {
        PyValue::Decimal(Arc::new(value))
    }

    /// What `cache.set(key, {"data": response.data, ...})` writes for a
    /// `ReturnList` (`many=True` serializers).
    #[must_use]
    pub fn return_list(values: Vec<PyValue>) -> Self {
        PyValue::Reduce(Arc::new(PyReduce {
            func: ReduceFunc::List,
            value: PyValue::list(values),
        }))
    }

    /// What `cache.set(key, {"data": response.data, ...})` writes for a
    /// `ReturnDict` (single-object serializers).
    #[must_use]
    pub fn return_dict(values: Vec<(&str, PyValue)>) -> Self {
        PyValue::Reduce(Arc::new(PyReduce {
            func: ReduceFunc::Dict,
            value: PyValue::dict(values),
        }))
    }

    /// [`PyValue::return_dict`] with pre-built keys.
    ///
    /// A `ReturnDict` from `serializer.data` uses the serializer's field-name
    /// objects as keys, so a handler that interns them with [`PyInterner`]
    /// must be able to pass them through instead of having `return_dict`
    /// create fresh ones.
    #[must_use]
    pub fn return_mapping(entries: Vec<(PyValue, PyValue)>) -> Self {
        PyValue::Reduce(Arc::new(PyReduce {
            func: ReduceFunc::Dict,
            value: PyValue::mapping(entries),
        }))
    }

    /// `dict.get(key)` for a `str` key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&PyValue> {
        match self {
            PyValue::Dict(dict) => dict.get(key),
            _ => None,
        }
    }

    /// The text of a `str` without a lone surrogate.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            PyValue::Str(text) => text.as_str(),
            _ => None,
        }
    }

    /// The items of a `list`/`tuple`/`set`/`frozenset`.
    #[must_use]
    pub fn as_sequence(&self) -> Option<&[PyValue]> {
        match self {
            PyValue::List(items)
            | PyValue::Tuple(items)
            | PyValue::Set(items)
            | PyValue::FrozenSet(items) => Some(items),
            PyValue::Reduce(reduce) => reduce.value.as_sequence(),
            _ => None,
        }
    }

    /// The members of a `dict`.
    #[must_use]
    pub fn as_mapping(&self) -> Option<&PyMapping> {
        match self {
            PyValue::Dict(dict) => Some(dict),
            PyValue::Reduce(reduce) => reduce.value.as_mapping(),
            _ => None,
        }
    }

    /// The `i64` value of an `int`.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PyValue::Int(value) => value.to_i64(),
            _ => None,
        }
    }

    /// The `f64` value of a `float`.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PyValue::Float(value) => Some(*value),
            _ => None,
        }
    }

    /// Whether a `ReturnList`/`ReturnDict` wraps this value.
    #[must_use]
    pub fn reduce(&self) -> Option<&PyReduce> {
        match self {
            PyValue::Reduce(reduce) => Some(reduce),
            _ => None,
        }
    }

    /// Strip `ReturnList`/`ReturnDict` wrappers, recursively.
    ///
    /// This is the tree `pickle.loads` produces: the wrappers only exist to
    /// reproduce the bytes CPython writes.
    #[must_use]
    pub fn decayed(&self) -> PyValue {
        match self {
            PyValue::Reduce(reduce) => reduce.value.decayed(),
            PyValue::List(items) => PyValue::List(Arc::new(
                items.iter().map(PyValue::decayed).collect::<Vec<_>>(),
            )),
            PyValue::Tuple(items) => PyValue::Tuple(Arc::new(
                items.iter().map(PyValue::decayed).collect::<Vec<_>>(),
            )),
            PyValue::Set(items) => PyValue::Set(Arc::new(
                items.iter().map(PyValue::decayed).collect::<Vec<_>>(),
            )),
            PyValue::FrozenSet(items) => PyValue::FrozenSet(Arc::new(
                items.iter().map(PyValue::decayed).collect::<Vec<_>>(),
            )),
            PyValue::Dict(dict) => PyValue::Dict(Arc::new(PyMapping {
                entries: dict
                    .entries
                    .iter()
                    .map(|(key, value)| (key.decayed(), value.decayed()))
                    .collect(),
            })),
            other => other.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Sharing
// ---------------------------------------------------------------------------

/// Reproduces CPython's object identity for a DRF `serializer.data` payload.
///
/// The pickler memoises by `id(obj)`, so whether two equal strings are written
/// once (`MEMOIZE` + `BINGET`) or twice (`SHORT_BINUNICODE` twice) changes the
/// cached bytes even though the decoded value is identical. Django's payload
/// has three kinds of `str`:
///
/// * **dict keys and `ChoiceField` values** — the serializer's field names are
///   class-level strings reused for every row, and a model `choices` value is a
///   compiler-interned literal. Both are the same object wherever they occur,
///   so [`PyInterner::key`] and [`PyInterner::choice`] draw from one pool.
/// * **database values** (`CharField`/`TextField`/`SlugField` output) — each
///   column is decoded into a fresh `str`, which is *not* interned even when an
///   equal interned literal exists. [`PyInterner::value`] returns a fresh
///   object, except for the two CPython singletons a decode can hit: the empty
///   string (`unicode_empty`) and a single U+0000..U+00FF character
///   (`unicode_latin1`). (`Objects/unicodeobject.c`, `PyUnicode_DecodeUTF8`.)
/// * **derived strings** (`DateTimeField`'s `isoformat()`, `str(uuid)`) — new
///   objects every time; [`PyInterner::value`] is also right for those.
///
/// A value that occurs only once is encoded identically either way, so the
/// builder only has to be exact about repeats. `null`, `true`/`false`, ints and
/// floats are never memoised by the pickler, so their identity does not matter.
///
/// ```ignore
/// let mut strings = PyInterner::new();
/// let name = strings.value(&label.name);
/// let kind = strings.choice(&estimate.r#type);
/// let row = strings.mapping([("name", name), ("type", kind)]);
/// ```
#[derive(Debug, Default)]
pub struct PyInterner {
    strings: HashMap<String, PyValue>,
}

impl PyInterner {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A dict key (or `.values()` column name): one object per name, shared by
    /// every row.
    pub fn key(&mut self, name: &str) -> PyValue {
        self.interned(name)
    }

    /// A value a `ChoiceField` produced.
    ///
    /// `ChoiceField.to_representation` returns the object stored in its
    /// `choice_strings_to_values` map (`rest_framework/fields.py:900-911`),
    /// which is the model's choice constant — a compiler-interned string, i.e.
    /// the same object as a field name with that text.
    pub fn choice(&mut self, text: &str) -> PyValue {
        self.interned(text)
    }

    /// A string read from the database (or built by a field renderer).
    ///
    /// Fresh, except for CPython's empty-string and single-Latin-1 singletons.
    pub fn value(&mut self, text: &str) -> PyValue {
        if is_cpython_singleton_string(text) {
            return self.interned(text);
        }
        PyValue::string(text.to_owned())
    }

    /// Build a row dict, interning every key.
    pub fn mapping<'a>(
        &mut self,
        entries: impl IntoIterator<Item = (&'a str, PyValue)>,
    ) -> PyValue {
        PyValue::mapping(
            entries
                .into_iter()
                .map(|(name, value)| (self.key(name), value))
                .collect(),
        )
    }

    /// The shared object for `text`, creating it on first use.
    fn interned(&mut self, text: &str) -> PyValue {
        if let Some(existing) = self.strings.get(text) {
            return existing.clone();
        }
        let value = PyValue::string(text.to_owned());
        self.strings.insert(text.to_owned(), value.clone());
        value
    }

    /// The number of interned strings (for tests and diagnostics).
    #[must_use]
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Whether nothing has been interned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

/// Whether [`PyUnicode_DecodeUTF8`](https://docs.python.org/3/c-api/unicode.html)
/// returns a singleton for this text.
///
/// CPython returns `unicode_empty` for a zero-length decode and the cached
/// `unicode_latin1` object for a single character below U+0100; every other
/// length allocates.
fn is_cpython_singleton_string(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => u32::from(c) <= 0xFF,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// JSON rendering
// ---------------------------------------------------------------------------

impl Serialize for PyValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // `None` at the document root renders as an empty body under
            // `Dialect::Drf`, which the serializer handles by depth.
            PyValue::None => serializer.serialize_unit(),
            PyValue::Bool(value) => serializer.serialize_bool(*value),
            PyValue::Int(value) => json_int(value).serialize(serializer),
            PyValue::Float(value) => serializer.serialize_f64(*value),
            PyValue::Str(value) => value.serialize(serializer),
            // `JSONEncoder.default` for bytes: `obj.decode()`.
            PyValue::Bytes(value) => match std::str::from_utf8(value) {
                Ok(text) => serializer.serialize_str(text),
                Err(error) => Err(serde::ser::Error::custom(format_args!(
                    "UnicodeDecodeError: {error}"
                ))),
            },
            PyValue::List(items) | PyValue::Tuple(items) => serialize_sequence(serializer, items),
            PyValue::Set(items) | PyValue::FrozenSet(items) => {
                serialize_sequence(serializer, items)
            }
            PyValue::Dict(dict) => {
                let mut map = serializer.serialize_map(Some(dict.entries.len()))?;
                for (key, value) in &dict.entries {
                    map.serialize_entry(&PyValueKey(key), value)?;
                }
                map.end()
            }
            PyValue::Uuid(value) => serializer.collect_str(&value.hyphenated()),
            PyValue::Date(value) => PyDate(**value).serialize(serializer),
            // `datetime.isoformat()` has no offset for a naive value; DRF's
            // encoder only rewrites `+00:00` to `Z`.
            PyValue::DateTime(value) => match value.tz {
                None => DrfNaiveDateTime(value.naive).serialize(serializer),
                Some(_) => drf_datetime(value).serialize(serializer),
            },
            PyValue::Time(value) => PyTime(**value).serialize(serializer),
            PyValue::TimeDelta(value) => PyTimedelta(**value).serialize(serializer),
            PyValue::Decimal(value) => EncoderDecimal::Drf((**value).clone()).serialize(serializer),
            PyValue::TimeZone(_) => Err(serde::ser::Error::custom(
                "TypeError: Object of type timezone is not JSON serializable",
            )),
            PyValue::Reduce(reduce) => reduce.value.serialize(serializer),
        }
    }
}

fn serialize_sequence<S: Serializer, T: Serialize>(
    serializer: S,
    items: &[T],
) -> Result<S::Ok, S::Error> {
    let mut sequence = serializer.serialize_seq(Some(items.len()))?;
    for item in items {
        sequence.serialize_element(item)?;
    }
    sequence.end()
}

/// A `PyInt` as a JSON number, including values outside the `i64` range (which
/// need `plane-serde`'s arbitrary-precision number protocol).
fn json_int(value: &PyInt) -> JsonPyInt {
    match value.to_i64() {
        Some(small) => JsonPyInt::Small(small),
        None => {
            let text = value.to_decimal_string();
            let digits = text.strip_prefix('-').unwrap_or(&text);
            match BigInt::from_decimal(value.is_negative(), digits) {
                Some(big) => JsonPyInt::Big(big),
                // `from_decimal` only rejects an empty digit string, which
                // cannot happen for a non-zero value.
                None => JsonPyInt::Small(0),
            }
        }
    }
}

/// The aware-datetime renderer. Only called when `tz` is `Some`.
fn drf_datetime(value: &PyDateTime) -> DrfDateTime<FixedOffset> {
    let offset = value.tz.as_ref().map_or(0, |tz| tz.offset_seconds());
    // `PyDateTime::naive` is the wall-clock value, exactly as Python stores
    // it, so the offset is attached to it rather than applied to it.
    // `datetime.timezone` only accepts whole-minute offsets below one day, so
    // this cannot fail for a value that came from Python; an out-of-range
    // offset falls back to UTC rather than panicking.
    let zone = FixedOffset::east_opt(offset)
        .or_else(|| FixedOffset::east_opt(0))
        .unwrap_or_else(|| FixedOffset::east_opt(0).expect("zero offset"));
    let aware = zone
        .from_local_datetime(&value.naive)
        .single()
        .unwrap_or_else(|| Utc.from_utc_datetime(&value.naive).fixed_offset());
    DrfDateTime(aware)
}

/// A `dict` key rendered the way `json.dumps` renders non-`str` keys.
struct PyValueKey<'a>(&'a PyValue);

impl Serialize for PyValueKey<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let key = match self.0 {
            PyValue::Str(text) => return text.serialize(serializer),
            PyValue::Bool(value) => PyMapKey::Bool(*value),
            PyValue::None => PyMapKey::Null,
            PyValue::Int(value) => match (value.to_i64(), value.to_u64()) {
                (Some(small), _) => PyMapKey::Int(small),
                (None, Some(large)) => PyMapKey::UInt(large),
                (None, None) => {
                    return Err(serde::ser::Error::custom(
                        "TypeError: dict key is an int outside the u64 range",
                    ));
                }
            },
            PyValue::Float(value) => PyMapKey::Float(*value),
            other => {
                return Err(serde::ser::Error::custom(format_args!(
                    "TypeError: keys must be str, int, float, bool or None, not {}",
                    other.type_name()
                )));
            }
        };
        key.serialize(serializer)
    }
}

impl PyValue {
    /// The Python type name, for error messages.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            PyValue::None => "NoneType",
            PyValue::Bool(_) => "bool",
            PyValue::Int(_) => "int",
            PyValue::Float(_) => "float",
            PyValue::Str(_) => "str",
            PyValue::Bytes(_) => "bytes",
            PyValue::List(_) => "list",
            PyValue::Tuple(_) => "tuple",
            PyValue::Set(_) => "set",
            PyValue::FrozenSet(_) => "frozenset",
            PyValue::Dict(_) => "dict",
            PyValue::Uuid(_) => "UUID",
            PyValue::Date(_) => "date",
            PyValue::DateTime(_) => "datetime",
            PyValue::Time(_) => "time",
            PyValue::TimeDelta(_) => "timedelta",
            PyValue::TimeZone(_) => "timezone",
            PyValue::Decimal(_) => "Decimal",
            PyValue::Reduce(_) => "list",
        }
    }
}

impl fmt::Display for PyValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PyValue::None => f.write_str("None"),
            PyValue::Bool(true) => f.write_str("True"),
            PyValue::Bool(false) => f.write_str("False"),
            PyValue::Int(value) => write!(f, "{value}"),
            PyValue::Float(value) => write!(f, "{value:?}"),
            PyValue::Str(value) => write!(f, "{value:?}"),
            PyValue::Uuid(value) => write!(f, "{value}"),
            PyValue::Decimal(value) => write!(f, "{value}"),
            other => write!(f, "<{}>", other.type_name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::{Dialect, to_vec};

    fn render(value: &PyValue) -> String {
        String::from_utf8(to_vec(Dialect::Drf, value).expect("renders")).expect("utf-8")
    }

    #[test]
    fn renders_like_the_drf_encoder() {
        // A top-level `None` is an empty body: `JSONRenderer.render(None)`
        // returns `b""`. Inside a container it is `null`.
        assert_eq!(render(&PyValue::none()), "");
        assert_eq!(render(&PyValue::list(vec![PyValue::none()])), "[null]");
        assert_eq!(render(&PyValue::bool(true)), "true");
        assert_eq!(render(&PyValue::int(-7)), "-7");
        assert_eq!(render(&PyValue::float(65535.0)), "65535.0");
        assert_eq!(render(&PyValue::float(1e-5)), "1e-05");
        assert_eq!(render(&PyValue::string("a\nb")), "\"a\\nb\"");
        assert_eq!(
            render(&PyValue::uuid(
                Uuid::parse_str("12345678-1234-5678-1234-567812345678").unwrap()
            )),
            "\"12345678-1234-5678-1234-567812345678\""
        );
        assert_eq!(
            render(&PyValue::date(
                NaiveDate::from_ymd_opt(2026, 11, 16).unwrap()
            )),
            "\"2026-11-16\""
        );
        assert_eq!(
            render(&PyValue::aware_datetime(
                NaiveDate::from_ymd_opt(2026, 11, 16)
                    .unwrap()
                    .and_hms_opt(12, 0, 0)
                    .unwrap(),
                PyTz::Utc
            )),
            "\"2026-11-16T12:00:00Z\""
        );
        assert_eq!(
            render(&PyValue::aware_datetime(
                NaiveDate::from_ymd_opt(2026, 11, 16)
                    .unwrap()
                    .and_hms_micro_opt(12, 0, 0, 123_456)
                    .unwrap(),
                PyTz::Offset(19_800)
            )),
            "\"2026-11-16T12:00:00.123456+05:30\""
        );
        assert_eq!(
            render(&PyValue::decimal(
                PyDecimal::parse("1.50").expect("decimal")
            )),
            "1.5"
        );
        assert_eq!(
            render(&PyValue::dict(vec![
                ("a", PyValue::int(1)),
                ("b", PyValue::list(vec![PyValue::none()])),
            ])),
            "{\"a\":1,\"b\":[null]}"
        );
        // `json.dumps({1: 2})` → `{"1": 2}`.
        assert_eq!(
            render(&PyValue::mapping(vec![(PyValue::int(1), PyValue::int(2))])),
            "{\"1\":2}"
        );
    }

    #[test]
    fn big_ints_render_as_bare_numbers() {
        let big = PyInt::parse("1267650600228229401496703205376").expect("parses");
        assert_eq!(
            render(&PyValue::Int(big)),
            "1267650600228229401496703205376"
        );
    }

    #[test]
    fn decay_strips_return_wrappers() {
        let wrapped = PyValue::return_list(vec![PyValue::int(1)]);
        assert!(wrapped.reduce().is_some());
        assert_eq!(wrapped.decayed(), PyValue::list(vec![PyValue::int(1)]));
        assert_eq!(render(&wrapped), "[1]");
    }

    fn arc_of(value: &PyValue) -> usize {
        match value {
            PyValue::Str(text) => std::sync::Arc::as_ptr(text) as *const () as usize,
            other => panic!("not a string: {other:?}"),
        }
    }

    #[test]
    fn interner_shares_keys_and_choices() {
        let mut strings = PyInterner::new();
        let first = strings.key("is_favorite");
        let second = strings.key("is_favorite");
        assert_eq!(arc_of(&first), arc_of(&second));
        // A `ChoiceField` returns the compiler-interned literal, which is the
        // same object as a field name with that text.
        let choice = strings.choice("is_favorite");
        assert_eq!(arc_of(&first), arc_of(&choice));
        assert_eq!(strings.len(), 1);
        assert!(!strings.is_empty());
    }

    #[test]
    fn interner_keeps_database_strings_fresh() {
        let mut strings = PyInterner::new();
        // Two equal multi-character DB values are distinct objects in Python,
        // even when an equal interned key exists (`sys.intern("Bug") is not
        // label.name` for a decoded value).
        let key = strings.key("Bug");
        let first = strings.value("Bug");
        let second = strings.value("Bug");
        assert_ne!(arc_of(&first), arc_of(&second));
        assert_ne!(arc_of(&key), arc_of(&first));
        // CPython's empty-string and single-Latin-1 singletons are shared by a
        // decode, including with an equal interned key.
        let empty = strings.value("");
        assert_eq!(arc_of(&empty), arc_of(&strings.key("")));
        let one = strings.value("a");
        assert_eq!(arc_of(&one), arc_of(&strings.key("a")));
        let latin1 = strings.value("\u{e9}");
        assert_eq!(arc_of(&latin1), arc_of(&strings.key("\u{e9}")));
        // A single astral character has no cached singleton.
        let astral = strings.value("\u{1f600}");
        let astral_key = strings.key("\u{1f600}");
        assert_ne!(arc_of(&astral), arc_of(&astral_key));
    }

    #[test]
    fn interner_builds_shared_row_keys() {
        let mut strings = PyInterner::new();
        let crash = strings.value("Crash");
        let api = strings.value("API");
        let first = strings.mapping([("id", PyValue::none()), ("name", crash)]);
        let second = strings.mapping([("id", PyValue::none()), ("name", api)]);
        let first = first.as_mapping().expect("mapping");
        let second = second.as_mapping().expect("mapping");
        assert_eq!(arc_of(&first.entries[0].0), arc_of(&second.entries[0].0));
        assert_eq!(arc_of(&first.entries[1].0), arc_of(&second.entries[1].0));
        assert_ne!(arc_of(&first.entries[1].1), arc_of(&second.entries[1].1));
    }
}
