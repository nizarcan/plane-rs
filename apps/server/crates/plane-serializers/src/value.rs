// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A dynamically shaped DRF value.
//!
//! [`DrfValue`] exists for the parts of a response whose shape is not a struct:
//! `SerializerMethodField` results, `expand=` relation values, and the
//! `issue_attachments` block. It is deliberately *ordered*: [`DrfValue::Object`]
//! is a slice of pairs, not a map, so a handler cannot accidentally sort keys.
//!
//! Everything a method returns in Plane fits one of the variants:
//!
//! | Python | Variant |
//! | --- | --- |
//! | `None` | [`DrfValue::Null`] |
//! | `bool` / `int` / `float` | `Bool` / `Int` / `Float` |
//! | `str` | `Str` (borrowed) or `String` (built, e.g. an invite link) |
//! | `uuid.UUID` | `Uuid` |
//! | `datetime` | `DateTime` (rendered in the active timezone) |
//! | a `jsonb` column's text | `JsonText` |
//! | a list | `Array` |
//! | a dict built in Python | `Object` |
//! | `.data` of another serializer | `Nested` |

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::fields::{DrfDateTime, DrfDecimal};
use crate::ser::DrfSerialize;
use crate::writer::DrfWriter;
use crate::{DrfError, RenderCtx};

/// A borrowed nested serializer.
pub type NestedRef<'a> = &'a (dyn DrfSerialize + 'a);

/// A DRF value whose shape is decided at runtime.
#[derive(Clone, Debug)]
pub enum DrfValue<'a> {
    /// `null`.
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// A borrowed string.
    Str(&'a str),
    /// A string the view built (an invite link, a formatted id).
    String(String),
    Uuid(Uuid),
    /// `DateTimeField`: DRF renders it in the active timezone.
    DateTime(DateTime<Utc>),
    /// A decimal as `DecimalField` would render it.
    Decimal(DrfDecimal),
    /// `jsonb` text from Postgres; re-encoded without touching key order.
    JsonText(&'a str),
    /// A JSON array. Owned so a handler can build one from a query result
    /// (attachments, expanded relations) without a second allocation for the
    /// slice; `DrfValue` is only used on the dynamic paths, never per scalar.
    Array(Vec<DrfValue<'a>>),
    /// An insertion-ordered object.
    Object(Vec<(&'a str, DrfValue<'a>)>),
    /// Another serializer's `.data`.
    Nested(NestedRef<'a>),
}

impl<'a> DrfValue<'a> {
    /// A nested serializer's `.data`.
    #[must_use]
    pub fn nested<T: DrfSerialize + 'a>(value: &'a T) -> Self {
        Self::Nested(value)
    }

    /// A JSON array of values.
    #[must_use]
    pub fn array(values: Vec<Self>) -> Self {
        Self::Array(values)
    }

    /// Write this value.
    ///
    /// # Errors
    ///
    /// Any [`DrfError`] from `plane-serde` (a non-finite float, invalid JSON
    /// text) or from the nested serializer.
    pub fn write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        match self {
            Self::Null => {
                w.write_null();
                Ok(())
            }
            Self::Bool(value) => w.write_value(value),
            Self::Int(value) => w.write_value(value),
            Self::Float(value) => w.write_value(value),
            Self::Str(value) => w.write_value(value),
            Self::String(value) => w.write_value(value),
            Self::Uuid(value) => w.write_value(&crate::fields::DrfUuid(*value)),
            Self::DateTime(value) => w.write_value(&DrfDateTime(value.with_timezone(ctx.tz()))),
            Self::Decimal(value) => w.write_value(value),
            Self::JsonText(text) => w.write_json_text(text),
            Self::Array(items) => {
                w.write_byte(b'[');
                for (index, item) in items.iter().enumerate() {
                    if index != 0 {
                        w.write_byte(b',');
                    }
                    item.write(w, ctx)?;
                }
                w.write_byte(b']');
                Ok(())
            }
            Self::Object(entries) => {
                w.write_byte(b'{');
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index != 0 {
                        w.write_byte(b',');
                    }
                    w.write_value(key)?;
                    w.write_byte(b':');
                    value.write(w, ctx)?;
                }
                w.write_byte(b'}');
                Ok(())
            }
            Self::Nested(serializer) => serializer.drf_write(w, &RenderCtx::full(ctx.tz())),
        }
    }
}

impl<'a> From<&'a str> for DrfValue<'a> {
    fn from(value: &'a str) -> Self {
        Self::Str(value)
    }
}

impl From<String> for DrfValue<'_> {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<Uuid> for DrfValue<'_> {
    fn from(value: Uuid) -> Self {
        Self::Uuid(value)
    }
}

impl From<bool> for DrfValue<'_> {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for DrfValue<'_> {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}
