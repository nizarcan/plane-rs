// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Newtypes that implement `Serialize` the way a specific Python code path
//! formats the value.
//!
//! A struct field rendered by DRF's `DateTimeField` should use
//! [`DrfDateTime`]; a `datetime` that ends up in a Django `JsonResponse`
//! should use [`DjangoDateTime`]; a value inside a Celery message body should
//! use [`KombuDateTime`] or one of the other `Kombu*` envelopes. Using the
//! wrong wrapper changes bytes, which is the thing this crate exists to
//! prevent.
//!
//! Every type here writes its text through the dialect's string escaping, so
//! a datetime under `Dialect::Django` gets the same `ensure_ascii` treatment
//! as any other string (not that datetimes contain non-ASCII).

use std::fmt;

use chrono::{
    DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, TimeDelta, TimeZone, Timelike, Utc,
};
use serde::Serialize;
use serde::ser::{Error as _, SerializeMap, Serializer};
use uuid::Uuid;

use crate::decimal::PyDecimal;
use crate::float::PyFloatRepr;

// ---------------------------------------------------------------------------
// Datetimes, dates and times
// ---------------------------------------------------------------------------

/// DRF `DateTimeField.to_representation` with `DATETIME_FORMAT=ISO_8601`:
/// `isoformat()` with `+00:00` replaced by `Z` and microseconds written only
/// when non-zero (always six digits then).
///
/// The default type parameter keeps the common `DrfDateTime(DateTime<Utc>)`
/// construction; any `TimeZone` works, so a value already converted to the
/// request user's timezone (`DateTime<FixedOffset>` from `chrono-tz`) renders
/// with that offset exactly as Python's `astimezone()` would.
/// `rest_framework/fields.py:1216-1232`.
#[derive(Clone, Debug)]
pub struct DrfDateTime<Tz: TimeZone = Utc>(pub DateTime<Tz>);

/// `DjangoJSONEncoder.default` for a `datetime`: `isoformat()` truncated to
/// milliseconds (by slicing off the last three microsecond digits, not by
/// rounding), `+00:00` replaced by `Z`.
/// `django/core/serializers/json.py:93-99`.
#[derive(Clone, Debug)]
pub struct DjangoDateTime<Tz: TimeZone = Utc>(pub DateTime<Tz>);

/// `kombu.utils.json` envelope for an aware `datetime`:
/// `{"__type__": "datetime", "__value__": "<isoformat()>"}`. Unlike either
/// encoder above, kombu keeps `+00:00` and all six microsecond digits.
/// `kombu/utils/json.py:128-129`.
#[derive(Clone, Debug)]
pub struct KombuDateTime<Tz: TimeZone = Utc>(pub DateTime<Tz>);

/// [`DrfDateTime`] for a naive datetime (Python's `isoformat()` has no
/// offset suffix). Plane runs with `USE_TZ=True`, so this is only for values
/// that genuinely lost their timezone.
#[derive(Clone, Copy, Debug)]
pub struct DrfNaiveDateTime(pub NaiveDateTime);

/// [`DjangoDateTime`] for a naive datetime.
#[derive(Clone, Copy, Debug)]
pub struct DjangoNaiveDateTime(pub NaiveDateTime);

/// [`KombuDateTime`] for a naive datetime.
#[derive(Clone, Copy, Debug)]
pub struct KombuNaiveDateTime(pub NaiveDateTime);

/// DRF `DateField` / `DjangoJSONEncoder` for a `date`: `isoformat()`,
/// `YYYY-MM-DD`.
#[derive(Clone, Copy, Debug)]
pub struct PyDate(pub NaiveDate);

/// DRF `TimeField` / DRF `JSONEncoder` for a naive `time`: `isoformat()`,
/// `HH:MM:SS` with six microsecond digits when non-zero.
#[derive(Clone, Copy, Debug)]
pub struct PyTime(pub NaiveTime);

/// `DjangoJSONEncoder` for a naive `time`: `isoformat()` truncated to
/// milliseconds (three digits), omitted when the microsecond is zero.
/// `django/core/serializers/json.py:102-108`.
#[derive(Clone, Copy, Debug)]
pub struct DjangoTime(pub NaiveTime);

#[derive(Clone, Copy)]
enum Fraction {
    /// `.ffffff`, omitted when zero (`datetime.isoformat()`).
    Six,
    /// `.mmm`, omitted when zero (Django's `r[:23]`/`r[:12]` slicing).
    Three,
}

#[derive(Clone, Copy)]
struct IsoDateTime {
    naive: NaiveDateTime,
    offset_seconds: Option<i32>,
    fraction: Fraction,
    z_for_utc: bool,
}

impl fmt::Display for IsoDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let date = self.naive.date();
        let time = self.naive.time();
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
            date.year(),
            date.month(),
            date.day(),
            time.hour(),
            time.minute(),
            time.second()
        )?;
        // Leap seconds are not representable in Python; drop the extra
        // nanosecond by taking the remainder.
        let micros = time.nanosecond() % 1_000_000_000 / 1_000;
        if micros != 0 {
            match self.fraction {
                Fraction::Six => write!(f, ".{micros:06}")?,
                Fraction::Three => write!(f, ".{:03}", micros / 1_000)?,
            }
        }
        match self.offset_seconds {
            Some(seconds) if self.z_for_utc && seconds == 0 => f.write_str("Z"),
            Some(seconds) => write_offset(f, seconds),
            None => Ok(()),
        }
    }
}

fn write_offset(f: &mut fmt::Formatter<'_>, seconds: i32) -> fmt::Result {
    let negative = seconds < 0;
    let magnitude = seconds.unsigned_abs();
    let hours = magnitude / 3_600;
    let minutes = (magnitude % 3_600) / 60;
    let rest = magnitude % 60;
    f.write_str(if negative { "-" } else { "+" })?;
    write!(f, "{hours:02}:{minutes:02}")?;
    if rest != 0 {
        write!(f, ":{rest:02}")?;
    }
    Ok(())
}

impl<Tz: TimeZone> DrfDateTime<Tz> {
    fn iso(&self) -> IsoDateTime {
        let fixed = self.0.fixed_offset();
        IsoDateTime {
            naive: fixed.naive_local(),
            offset_seconds: Some(fixed.offset().local_minus_utc()),
            fraction: Fraction::Six,
            z_for_utc: true,
        }
    }

    /// The exact Python `str` a `DateTimeField` put into `serializer.data`.
    ///
    /// `to_representation` returns `value.isoformat()` (with `+00:00` replaced
    /// by `Z`), so a cached `serializer.data` holds a *string*, not a
    /// `datetime` (`rest_framework/fields.py:1216-1232`). A handler that builds
    /// the cache payload by hand has to construct that string; going through
    /// the JSON encoder and stripping its quotes loses nothing but is
    /// needlessly indirect, and it fails for any value the encoder would escape
    /// (a datetime never contains one). This returns the same bytes as the
    /// `Serialize` impl without the quotes.
    #[must_use]
    pub fn to_drf_string(&self) -> String {
        self.iso().to_string()
    }
}

impl<Tz: TimeZone> Serialize for DrfDateTime<Tz> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.iso())
    }
}

impl<Tz: TimeZone> fmt::Display for DrfDateTime<Tz> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.iso(), f)
    }
}

impl<Tz: TimeZone> DjangoDateTime<Tz> {
    fn iso(&self) -> IsoDateTime {
        let fixed = self.0.fixed_offset();
        IsoDateTime {
            naive: fixed.naive_local(),
            offset_seconds: Some(fixed.offset().local_minus_utc()),
            fraction: Fraction::Three,
            z_for_utc: true,
        }
    }
}

impl<Tz: TimeZone> Serialize for DjangoDateTime<Tz> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.iso())
    }
}

impl<Tz: TimeZone> KombuDateTime<Tz> {
    fn iso(&self) -> IsoDateTime {
        let fixed = self.0.fixed_offset();
        IsoDateTime {
            naive: fixed.naive_local(),
            offset_seconds: Some(fixed.offset().local_minus_utc()),
            fraction: Fraction::Six,
            z_for_utc: false,
        }
    }
}

impl<Tz: TimeZone> Serialize for KombuDateTime<Tz> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_envelope(serializer, "datetime", &IsoDisplay(self.iso()))
    }
}

impl Serialize for DrfNaiveDateTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&IsoDateTime {
            naive: self.0,
            offset_seconds: None,
            fraction: Fraction::Six,
            z_for_utc: false,
        })
    }
}

impl Serialize for DjangoNaiveDateTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&IsoDateTime {
            naive: self.0,
            offset_seconds: None,
            fraction: Fraction::Three,
            z_for_utc: false,
        })
    }
}

impl Serialize for KombuNaiveDateTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_envelope(
            serializer,
            "datetime",
            &IsoDisplay(IsoDateTime {
                naive: self.0,
                offset_seconds: None,
                fraction: Fraction::Six,
                z_for_utc: false,
            }),
        )
    }
}

/// A `Display` adapter so `collect_str`/`serialize_envelope` can take an
/// `IsoDateTime` by reference.
struct IsoDisplay(IsoDateTime);

impl fmt::Display for IsoDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for IsoDisplay {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

fn write_date(f: &mut fmt::Formatter<'_>, date: NaiveDate) -> fmt::Result {
    write!(
        f,
        "{:04}-{:02}-{:02}",
        date.year(),
        date.month(),
        date.day()
    )
}

fn write_time(f: &mut fmt::Formatter<'_>, time: NaiveTime, fraction: Fraction) -> fmt::Result {
    write!(
        f,
        "{:02}:{:02}:{:02}",
        time.hour(),
        time.minute(),
        time.second()
    )?;
    let micros = time.nanosecond() % 1_000_000_000 / 1_000;
    if micros != 0 {
        match fraction {
            Fraction::Six => write!(f, ".{micros:06}")?,
            Fraction::Three => write!(f, ".{:03}", micros / 1_000)?,
        }
    }
    Ok(())
}

struct DateDisplay(NaiveDate);

impl fmt::Display for DateDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_date(f, self.0)
    }
}

impl Serialize for DateDisplay {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

struct TimeDisplay {
    time: NaiveTime,
    fraction: Fraction,
}

impl fmt::Display for TimeDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_time(f, self.time, self.fraction)
    }
}

impl Serialize for TimeDisplay {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl Serialize for PyDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&DateDisplay(self.0))
    }
}

impl Serialize for PyTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&TimeDisplay {
            time: self.0,
            fraction: Fraction::Six,
        })
    }
}

impl Serialize for DjangoTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&TimeDisplay {
            time: self.0,
            fraction: Fraction::Three,
        })
    }
}

// ---------------------------------------------------------------------------
// Decimals
// ---------------------------------------------------------------------------

/// DRF `DecimalField.to_representation` with `COERCE_DECIMAL_TO_STRING=True`
/// (the Plane default): quantise to `decimal_places` with the context
/// precision set to `max_digits`, then `f"{quantized:f}"`.
///
/// The rounding mode is the decimal context's default, `ROUND_HALF_EVEN`,
/// because DRF only overrides it when `rounding=` is passed to the field (it
/// never is in Plane). When the quantised coefficient needs more than
/// `max_digits` digits, Python raises `decimal.InvalidOperation` and so does
/// this type (as a serialisation error).
/// `rest_framework/fields.py:1107-1145`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrfDecimal {
    /// The value as DRF receives it (`Decimal(str(value).strip())`).
    pub value: PyDecimal,
    /// `DecimalField(max_digits=…)`; also the decimal context precision.
    pub max_digits: u32,
    /// `DecimalField(decimal_places=…)`.
    pub decimal_places: u32,
}

impl DrfDecimal {
    /// Construct a DRF decimal representation.
    #[must_use]
    pub fn new(value: PyDecimal, max_digits: u32, decimal_places: u32) -> Self {
        Self {
            value,
            max_digits,
            decimal_places,
        }
    }
}

impl Serialize for DrfDecimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let quantized = self
            .value
            .quantize(self.decimal_places, Some(self.max_digits))
            .map_err(S::Error::custom)?;
        serializer.collect_str(&quantized.as_fixed())
    }
}

/// A `Decimal` that reaches a JSON encoder rather than a DRF field.
///
/// * [`EncoderDecimal::Drf`] is `rest_framework.utils.encoders.JSONEncoder`,
///   which returns `float(obj)`; the number is written with Python float
///   repr and `Dialect::Drf` rejects an overflowed infinity.
/// * [`EncoderDecimal::Django`] is `DjangoJSONEncoder`, which returns
///   `str(obj)`.
///
/// Kombu has a registered encoder of its own; use [`KombuDecimal`] for a
/// Celery body.
/// `rest_framework/utils/encoders.py:42-44`,
/// `django/core/serializers/json.py:111-112`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EncoderDecimal {
    /// DRF: `float(obj)`.
    Drf(PyDecimal),
    /// Django: `str(obj)`.
    Django(PyDecimal),
}

impl Serialize for EncoderDecimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            EncoderDecimal::Drf(value) => serializer.serialize_f64(value.to_f64()),
            EncoderDecimal::Django(value) => serializer.collect_str(value),
        }
    }
}

// ---------------------------------------------------------------------------
// Timedeltas
// ---------------------------------------------------------------------------

/// A Python `timedelta`.
///
/// * DRF's encoder renders `str(obj.total_seconds())`, a JSON **string**
///   (`"90061.5"`).
/// * Django's encoder renders `duration_iso_string(obj)` (`"P1DT01H01M01.500000S"`).
/// * Kombu has no registered encoder for `timedelta`, so a Celery body cannot
///   contain one (Python raises `TypeError`); there is deliberately no
///   `KombuTimedelta`.
///
/// `chrono::TimeDelta` has nanosecond precision while Python's `timedelta`
/// has microseconds; sub-microsecond parts are truncated toward zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PyTimedelta(pub TimeDelta);

impl PyTimedelta {
    /// Total microseconds, the internal unit Python's `timedelta` stores.
    pub fn total_microseconds(self) -> i128 {
        i128::from(self.0.num_seconds()) * 1_000_000 + i128::from(self.0.subsec_nanos() / 1_000)
    }
}

impl Serialize for PyTimedelta {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match seconds_as_float(self.total_microseconds()) {
            Some(repr) => serializer.collect_str(&repr),
            None => Err(S::Error::custom("timedelta.total_seconds() is not finite")),
        }
    }
}

/// `timedelta.total_seconds()` as CPython computes it: `PyNumber_TrueDivide`
/// on the exact integer microsecond count and 1_000_000, i.e. the correctly
/// rounded rational quotient. Converting the integer to `f64` first and then
/// dividing rounds twice and differs for large durations, so format the exact
/// quotient as a decimal string and let the correctly-rounding parser convert
/// it.
fn seconds_as_float(micros: i128) -> Option<PyFloatRepr> {
    let negative = micros < 0;
    let magnitude = micros.unsigned_abs();
    let whole = magnitude / 1_000_000;
    let fraction = magnitude % 1_000_000;
    let text = format!("{}{whole}.{fraction:06}", if negative { "-" } else { "" });
    let seconds = text.parse::<f64>().ok()?;
    PyFloatRepr::new(seconds)
}

/// Django's `duration_iso_string` representation of a timedelta.
///
/// `django/utils/duration.py:31-42`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DjangoTimedelta(pub TimeDelta);

impl Serialize for DjangoTimedelta {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&IsoDuration(PyTimedelta(self.0).total_microseconds()))
    }
}

struct IsoDuration(i128);

impl fmt::Display for IsoDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (sign, micros) = if self.0 < 0 {
            ("-", self.0.unsigned_abs())
        } else {
            ("", self.0 as u128)
        };
        let days = micros / 86_400_000_000;
        let rest = micros % 86_400_000_000;
        let hours = rest / 3_600_000_000;
        let rest = rest % 3_600_000_000;
        let minutes = rest / 60_000_000;
        let rest = rest % 60_000_000;
        let seconds = rest / 1_000_000;
        let micros = rest % 1_000_000;
        write!(f, "{sign}P{days}DT{hours:02}H{minutes:02}M{seconds:02}")?;
        if micros != 0 {
            write!(f, ".{micros:06}")?;
        }
        f.write_str("S")
    }
}

// ---------------------------------------------------------------------------
// kombu type envelopes
// ---------------------------------------------------------------------------

/// kombu's UUID envelope: `{"__type__": "uuid", "__value__": {"hex": "…"}}`
/// with 32 lowercase hex digits and no dashes.
/// `kombu/utils/json.py:138-143`, `apps/server/TASKS.md` §2.5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KombuUuid(pub Uuid);

impl Serialize for KombuUuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_envelope(serializer, "uuid", &UuidEnvelopeValue(self.0))
    }
}

struct UuidEnvelopeValue(Uuid);

impl Serialize for UuidEnvelopeValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        let simple = self.0.simple();
        map.serialize_entry("hex", &SimpleUuid(simple))?;
        map.end()
    }
}

struct SimpleUuid(uuid::fmt::Simple);

impl Serialize for SimpleUuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// kombu's `date` envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KombuDate(pub NaiveDate);

impl Serialize for KombuDate {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_envelope(serializer, "date", &DateDisplay(self.0))
    }
}

/// kombu's `time` envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KombuTime(pub NaiveTime);

impl Serialize for KombuTime {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_envelope(
            serializer,
            "time",
            &TimeDisplay {
                time: self.0,
                fraction: Fraction::Six,
            },
        )
    }
}

/// kombu's `decimal` envelope, whose value is `str(Decimal)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KombuDecimal(pub PyDecimal);

impl Serialize for KombuDecimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_envelope(serializer, "decimal", &self.0)
    }
}

/// kombu's `bytes`/`base64` envelope.
///
/// UTF-8 decodable bytes use `{"__type__": "bytes", "__value__": "…"}`; other
/// bytes use standard base64 with padding and the `base64` marker.
/// `kombu/utils/json.py:41-45, 125-146`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KombuBytes(pub Vec<u8>);

impl Serialize for KombuBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match std::str::from_utf8(&self.0) {
            Ok(text) => serialize_envelope(serializer, "bytes", text),
            Err(_) => serialize_envelope(serializer, "base64", &Base64(&self.0)),
        }
    }
}

struct Base64<'a>(&'a [u8]);

impl Serialize for Base64<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl fmt::Display for Base64<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const ALPHABET: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        for chunk in self.0.chunks(3) {
            let b0 = chunk[0];
            let b1 = chunk.get(1).copied();
            let b2 = chunk.get(2).copied();
            let triple = (u32::from(b0) << 16)
                | (u32::from(b1.unwrap_or(0)) << 8)
                | u32::from(b2.unwrap_or(0));
            let mut out = [b'='; 4];
            out[0] = ALPHABET[((triple >> 18) & 0x3F) as usize];
            out[1] = ALPHABET[((triple >> 12) & 0x3F) as usize];
            if b1.is_some() {
                out[2] = ALPHABET[((triple >> 6) & 0x3F) as usize];
            }
            if b2.is_some() {
                out[3] = ALPHABET[(triple & 0x3F) as usize];
            }
            // `out` only contains the base64 alphabet and '=', so this is
            // always valid UTF-8.
            let text = std::str::from_utf8(&out).map_err(|_| fmt::Error)?;
            f.write_str(text)?;
        }
        Ok(())
    }
}

fn serialize_envelope<S, T>(
    serializer: S,
    type_name: &'static str,
    value: &T,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: ?Sized + Serialize,
{
    let mut map = serializer.serialize_map(Some(2))?;
    map.serialize_entry("__type__", type_name)?;
    map.serialize_entry("__value__", value)?;
    map.end()
}

// ---------------------------------------------------------------------------
// Map keys
// ---------------------------------------------------------------------------

/// A dict key rendered the way Python's `json` module renders keys that are
/// not strings.
///
/// serde's map API normally requires string keys. `bool`, signed/unsigned
/// integers and finite floats already work through `serde_json`'s
/// `MapKeySerializer`; use this type when the same map has mixed key types or
/// needs Python's `None` → `"null"` rule.
#[derive(Clone, Copy, Debug)]
pub enum PyMapKey<'a> {
    /// A string key, escaped normally.
    Str(&'a str),
    /// An `int` key: `json.dumps({1: 2})` → `{"1": 2}`.
    Int(i64),
    /// A non-negative `int` key.
    UInt(u64),
    /// A `float` key, formatted with Python float repr.
    Float(f64),
    /// A `bool` key: `True` → `"true"`.
    Bool(bool),
    /// `None` → `"null"`.
    Null,
}

impl Serialize for PyMapKey<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            PyMapKey::Str(value) => serializer.serialize_str(value),
            PyMapKey::Int(value) => serializer.serialize_i64(*value),
            PyMapKey::UInt(value) => serializer.serialize_u64(*value),
            PyMapKey::Float(value) => serializer.serialize_f64(*value),
            PyMapKey::Bool(value) => serializer.serialize_bool(*value),
            PyMapKey::Null => serializer.serialize_str("null"),
        }
    }
}
