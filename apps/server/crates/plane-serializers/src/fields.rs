// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Value wrappers for the DRF field classes Plane uses.
//!
//! Each wrapper is a `serde::Serialize` implementation that produces exactly
//! what the Python field's `to_representation` returns. Plain `str`, `bool`,
//! `i64` and `f64` need no wrapper: `plane_serde` already writes them the way
//! Python's `json` module does.
//!
//! | DRF field | Wrapper | Python |
//! | --- | --- | --- |
//! | `UUIDField`, `PrimaryKeyRelatedField` over a UUID pk | [`DrfUuid`] | `str(uuid)` |
//! | `DateTimeField` | [`DrfDateTime`] | `isoformat()`, `+00:00` → `Z` |
//! | `DateField` | [`PyDate`] | `date.isoformat()` |
//! | `TimeField` | [`PyTime`] | `time.isoformat()` |
//! | `DecimalField(coerce_to_string=True)` | [`DrfDecimal`] | `f"{quantized:f}"` |
//! | `DecimalField(coerce_to_string=False)` | [`EncoderDecimal::Drf`] | `float(value)` |
//! | `ListField(child=UUIDField())` | [`DrfUuidList`] | `[str(uuid), …]` |
//! | `ListField(child=CharField())` | [`DrfStrList`] | `["a", …]` |
//! | `ListField(child=IntegerField())` | [`DrfIntList`] | `[1, …]` |
//! | `ModelField(BinaryField())` | [`PyBase64`] | `b64encode(bytes).decode("ascii")` |
//! | `JSONField` | [`PgJson`] | the `jsonb` text, re-encoded |
//!
//! `PrimaryKeyRelatedField` has two output shapes and they are not
//! interchangeable: `to_representation` returns `value.pk`
//! (`rest_framework/relations.py:265-268`), which DRF's `JSONEncoder` turns
//! into `str(uuid)` for a UUID pk but leaves as a JSON number for an integer
//! pk. UUID pks use [`DrfUuid`]; integer pks are passed as `i64`.

use std::fmt;

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use serde::ser::{SerializeSeq, Serializer};
use uuid::Uuid;

pub use plane_serde::values::{DrfDateTime, DrfDecimal, EncoderDecimal, PyDate, PyTime};

use crate::DrfError;
use crate::ser::RenderCtx;
use crate::writer::DrfWriter;

// ---------------------------------------------------------------------------
// A three-state field
// ---------------------------------------------------------------------------

/// The value of a field whose Python attribute may not exist.
///
/// DRF walks a serializer's readable fields and, for each one, calls
/// `field.get_attribute(instance)` (`rest_framework/fields.py:421-446`). Three
/// outcomes are possible and they are *not* interchangeable
/// (`rest_framework/serializers.py:530-551`):
///
/// | `get_attribute` | `check_for_none` | `to_representation` writes |
/// | --- | --- | --- |
/// | raises `SkipField` (the attribute is missing) | — | no key at all |
/// | returns `None` | `None` | `"key": null` |
/// | returns a value | not `None` | `"key": <value>` |
///
/// The middle case is why `Option<T>` cannot model a skippable field: `None`
/// must mean `null` here, not "omit the key". A generated struct therefore
/// spells out which case the handler means:
///
/// ```ignore
/// // `is_favorite` is a view annotation the queryset did not add.
/// is_favorite: FieldValue::absent(),
/// // `sort_order` was annotated and the annotation is SQL NULL.
/// sort_order: FieldValue::null(),
/// // `member_role` was annotated with a value.
/// member_role: FieldValue::value(20),
/// ```
///
/// `Option<T>` is still the right type for a *nullable model column*, whose
/// attribute always exists: `None` renders `null` there (the generator uses
/// `Option` only when `allow_null`/`null=True`, and never mixes the two on one
/// field). `FieldValue<T>` is used for annotation-backed fields with
/// `required=False`, the shape that can skip.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FieldValue<T> {
    /// The attribute does not exist: `getattr` raised `AttributeError`, DRF
    /// raised `SkipField` and the key is absent from the object.
    #[default]
    Absent,
    /// The attribute exists and is `None`: DRF writes the key with `null`
    /// without calling the field's `to_representation`.
    Null,
    /// The attribute exists and has a value.
    Value(T),
}

impl<T> FieldValue<T> {
    /// The attribute is missing.
    #[must_use]
    pub const fn absent() -> Self {
        Self::Absent
    }

    /// The attribute exists and is `None`.
    #[must_use]
    pub const fn null() -> Self {
        Self::Null
    }

    /// The attribute exists with this value.
    #[must_use]
    pub const fn value(value: T) -> Self {
        Self::Value(value)
    }

    /// `get_attribute` succeeded: `None` is the null case, `Some` the value
    /// case. Use this when the handler has an `Option` from an annotation
    /// that is always present (an outer join, `MAX(...)`, …).
    #[must_use]
    pub fn from_attribute(value: Option<T>) -> Self {
        match value {
            Some(value) => Self::Value(value),
            None => Self::Null,
        }
    }

    /// Whether `get_attribute` would raise `SkipField`.
    #[must_use]
    pub const fn is_absent(&self) -> bool {
        matches!(self, Self::Absent)
    }

    /// Whether the attribute exists (as `null` or as a value).
    #[must_use]
    pub const fn is_present(&self) -> bool {
        !self.is_absent()
    }
}

impl<T> From<T> for FieldValue<T> {
    fn from(value: T) -> Self {
        Self::Value(value)
    }
}

/// The scalar renderers a [`FieldValue`] can hold.
///
/// This is the subset of DRF field classes that can sit on a skippable field
/// (an annotation-backed declared field): the value itself is already typed by
/// the generator, so the trait only has to write it the way that field class
/// does. Every generated `FieldValue<T>` uses one of these impls; handler code
/// never names the trait.
pub trait DrfScalar {
    /// Write the value (not the key).
    ///
    /// # Errors
    ///
    /// See [`DrfWriter::write_value`] and the wrapper's own rules.
    fn write_scalar(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError>;
}

impl DrfScalar for bool {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(self)
    }
}

impl DrfScalar for i64 {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(self)
    }
}

impl DrfScalar for f64 {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(self)
    }
}

impl DrfScalar for &str {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(self)
    }
}

impl DrfScalar for String {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(self)
    }
}

impl DrfScalar for Uuid {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(&DrfUuid(*self))
    }
}

impl DrfScalar for DrfUuid {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(self)
    }
}

impl DrfScalar for DrfPk<'_> {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(self)
    }
}

impl DrfScalar for DrfDecimal {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(self)
    }
}

impl DrfScalar for PgJson<'_> {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_json_text(self.0)
    }
}

impl DrfScalar for DateTime<Utc> {
    fn write_scalar(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(&drf_datetime(*self, ctx.tz()))
    }
}

impl DrfScalar for NaiveDate {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(&PyDate(*self))
    }
}

impl DrfScalar for NaiveTime {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(&PyTime(*self))
    }
}

impl DrfScalar for &[u8] {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(&PyBase64(self))
    }
}

impl DrfScalar for &[Uuid] {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(&DrfUuidList(self))
    }
}

impl DrfScalar for &[i64] {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(&DrfIntList(self))
    }
}

impl DrfScalar for &[f64] {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(&DrfFloatList(self))
    }
}

impl DrfScalar for &[bool] {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(&DrfBoolList(self))
    }
}

impl DrfScalar for &[&str] {
    fn write_scalar(&self, w: &mut DrfWriter, _: &RenderCtx<'_>) -> Result<(), DrfError> {
        w.write_value(&DrfStrList(self))
    }
}

impl DrfScalar for crate::value::DrfValue<'_> {
    fn write_scalar(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        self.write(w, ctx)
    }
}

/// `str(uuid)` — `UUIDField.to_representation`
/// (`rest_framework/fields.py:847-850`) and the UUID half of
/// `PrimaryKeyRelatedField` output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrfUuid(pub Uuid);

impl Serialize for DrfUuid {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

/// A list of UUIDs, as a `ListField` of `UUIDField`s or a read-only M2M
/// `PrimaryKeyRelatedField(many=True)` renders it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrfUuidList<'a>(pub &'a [Uuid]);

impl Serialize for DrfUuidList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for uuid in self.0 {
            seq.serialize_element(&DrfUuid(*uuid))?;
        }
        seq.end()
    }
}

/// A list of strings (`ListField` of `CharField`/`URLField`, or a `CharField`
/// array column).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrfStrList<'a>(pub &'a [&'a str]);

impl Serialize for DrfStrList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            seq.serialize_element(value)?;
        }
        seq.end()
    }
}

/// A list of integers (`ListField` of `IntegerField`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrfIntList<'a>(pub &'a [i64]);

impl Serialize for DrfIntList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            seq.serialize_element(value)?;
        }
        seq.end()
    }
}

/// A list of floats (`ListField` of `FloatField`), written with Python float
/// repr by the formatter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrfFloatList<'a>(pub &'a [f64]);

impl Serialize for DrfFloatList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            seq.serialize_element(value)?;
        }
        seq.end()
    }
}

/// A list of booleans (`ListField` of `BooleanField`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrfBoolList<'a>(pub &'a [bool]);

impl Serialize for DrfBoolList<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for value in self.0 {
            seq.serialize_element(value)?;
        }
        seq.end()
    }
}

/// A primary-key value whose model is only known at the call site.
///
/// `PrimaryKeyRelatedField.to_representation` returns `value.pk`
/// (`rest_framework/relations.py:265-268`) and the JSON encoder decides the
/// shape: `str(uuid)` for a UUID pk, a JSON number for an integer pk, `str` for
/// a text pk. A read-only declared field with no `queryset` (over a view
/// annotation, like `IssueSerializer.cycle_id`) gives the inventory no model to
/// resolve, so the handler supplies the pk through this enum. Rendering is
/// exact for all three shapes; picking the wrong variant changes bytes, which
/// is the same class of mistake as passing the wrong Python value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrfPk<'a> {
    /// The relation (or the annotation) is `None`: DRF writes `null`.
    Null,
    Uuid(Uuid),
    Int(i64),
    Str(&'a str),
}

impl Serialize for DrfPk<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Uuid(value) => serializer.collect_str(value),
            Self::Int(value) => serializer.serialize_i64(*value),
            Self::Str(value) => serializer.serialize_str(value),
        }
    }
}

/// A `BinaryField` as DRF renders it: base64, ASCII, with padding.
///
/// `ModelSerializer.build_standard_field` has no mapping for
/// `django.db.models.BinaryField`, so DRF falls back to the deprecated
/// `rest_framework.fields.ModelField`, whose `to_representation` is
/// `model_field.value_to_string(obj)` (`rest_framework/fields.py:1966-1983`).
/// Django's `BinaryField.value_to_string` is
/// `b64encode(self.value_from_object(obj)).decode("ascii")`
/// (`django/db/models/fields/__init__.py:2720-2722`) — a standard-alphabet
/// base64 string, not `repr(bytes)` and not a URL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PyBase64<'a>(pub &'a [u8]);

impl fmt::Display for PyBase64<'_> {
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
            // `out` holds only the base64 alphabet and '=', so this is UTF-8.
            f.write_str(std::str::from_utf8(&out).map_err(|_| fmt::Error)?)?;
        }
        Ok(())
    }
}

impl Serialize for PyBase64<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// `json`/`jsonb` text exactly as Postgres returned it.
///
/// `rest_framework.fields.JSONField.to_representation` returns the decoded
/// value and `JSONRenderer` re-encodes it. The bytes are the ones
/// `json.loads(text)` followed by `json.dumps(..., ensure_ascii=False)` makes:
/// *object key order is the text's order*, which for `jsonb` is Postgres'
/// length-then-bytes ordering — not alphabetical. Parsing into
/// `serde_json::Value` would sort the keys (the workspace does not enable
/// `preserve_order`), so the text goes through
/// [`plane_serde::transcode_json`] instead. Use `Option<PgJson>` for a
/// nullable column; `None` renders `null`, which is what DRF does when the
/// attribute is `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PgJson<'a>(pub &'a str);

impl<'a> PgJson<'a> {
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self(text)
    }

    /// Borrow the text of a generated model row's `json`/`jsonb` column.
    ///
    /// `plane_db::JsonText` keeps the bytes Postgres sent (including the
    /// `jsonb` version byte removed); this is the bridge into the renderer:
    /// `row.logo_props.as_ref().map(PgJson::from)`.
    #[must_use]
    pub fn from_json_text(text: &'a plane_db::JsonText) -> Self {
        Self(text.as_str())
    }

    #[must_use]
    pub fn as_str(self) -> &'a str {
        self.0
    }
}

impl<'a> From<&'a plane_db::JsonText> for PgJson<'a> {
    fn from(text: &'a plane_db::JsonText) -> Self {
        Self::from_json_text(text)
    }
}

/// A `DecimalField` read from a `plane_db::Numeric` column.
///
/// `plane_db::Numeric` is an exact [`plane_serde::PyDecimal`] decoded from Postgres'
/// binary `numeric`; `DecimalField` then quantises to the serializer's
/// `max_digits`/`decimal_places` (`rest_framework/fields.py:1107-1145`), which
/// only the serializer declares.
#[must_use]
pub fn drf_decimal_numeric(
    value: &plane_db::Numeric,
    max_digits: u32,
    decimal_places: u32,
) -> DrfDecimal {
    DrfDecimal::new(value.decimal().clone(), max_digits, decimal_places)
}

/// `DateTimeField.to_representation` in an explicit timezone.
///
/// DRF converts with `value.astimezone(enforce_timezone())`, where
/// `default_timezone()` is `django.utils.timezone.get_current_timezone()`
/// (`rest_framework/fields.py:1180-1190`, `1191-1193`). Plane activates the
/// request user's timezone in `TimezoneMixin.initial`, or deactivates it
/// (`settings.TIME_ZONE`, "UTC") for anonymous requests. The timezone is a
/// required argument here so a handler cannot forget it.
#[must_use]
pub fn drf_datetime(value: DateTime<Utc>, tz: &Tz) -> DrfDateTime<Tz> {
    DrfDateTime(value.with_timezone(tz))
}

/// The same, for a nullable column: `null` stays `null`.
#[must_use]
pub fn drf_datetime_opt(value: Option<DateTime<Utc>>, tz: &Tz) -> Option<DrfDateTime<Tz>> {
    value.map(|value| drf_datetime(value, tz))
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, TimeZone};
    use plane_serde::{Dialect, to_vec};
    use uuid::uuid;

    use super::{DrfUuid, DrfUuidList, PyBase64, drf_datetime_opt};

    fn json<T: serde::Serialize>(value: &T) -> String {
        String::from_utf8(to_vec(Dialect::Drf, value).expect("render")).expect("utf-8")
    }

    #[test]
    fn uuid_is_str() {
        let uuid = uuid!("01234567-89ab-cdef-0123-456789abcdef");
        assert_eq!(
            json(&DrfUuid(uuid)),
            "\"01234567-89ab-cdef-0123-456789abcdef\""
        );
        assert_eq!(
            json(&DrfUuidList(&[uuid, uuid])),
            "[\"01234567-89ab-cdef-0123-456789abcdef\",\"01234567-89ab-cdef-0123-456789abcdef\"]"
        );
    }

    #[test]
    fn datetime_uses_the_given_timezone() {
        let utc = chrono::Utc
            .with_ymd_and_hms(2026, 11, 1, 6, 30, 0)
            .single()
            .expect("valid");
        // DST ended at 02:00 local on 2026-11-01, so 06:30 UTC is 01:30 EST.
        let new_york = drf_datetime_opt(Some(utc), &chrono_tz::America::New_York);
        assert_eq!(json(&new_york), "\"2026-11-01T01:30:00-05:00\"");
        let kolkata = drf_datetime_opt(Some(utc), &chrono_tz::Asia::Kolkata);
        assert_eq!(json(&kolkata), "\"2026-11-01T12:00:00+05:30\"");
        assert!(drf_datetime_opt(None, &chrono_tz::UTC).is_none());
    }

    #[test]
    fn json_text_keeps_postgres_key_order() {
        let mut writer = crate::DrfWriter::new();
        // Deliberately not alphabetical: `b` then `a`, the order `jsonb` emits.
        writer
            .write_json_text("{\"b\": 1, \"a\": 1e-05}")
            .expect("transcode");
        assert_eq!(
            String::from_utf8(writer.into_vec()).expect("utf-8"),
            "{\"b\":1,\"a\":1e-05}"
        );
    }

    #[test]
    fn bytes_are_base64() {
        // From the recorded golden `probe/no_args/utc`.
        assert_eq!(json(&PyBase64(b"\x00\xffit's")), "\"AP9pdCdz\"");
        assert_eq!(json(&PyBase64(b"")), "\"\"");
        assert_eq!(json(&PyBase64(b"a")), "\"YQ==\"");
        assert_eq!(json(&PyBase64(b"ab")), "\"YWI=\"");
        assert_eq!(json(&PyBase64(b"abc")), "\"YWJj\"");
    }

    #[test]
    fn date_and_time_are_iso() {
        let date = NaiveDate::from_ymd_opt(2026, 2, 3).expect("valid");
        assert_eq!(json(&plane_serde::values::PyDate(date)), "\"2026-02-03\"");
    }
}
