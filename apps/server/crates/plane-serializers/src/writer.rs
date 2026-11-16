// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The byte writer.
//!
//! [`DrfWriter`] owns the output buffer; [`ObjectWriter`] and [`ArrayWriter`]
//! are short-lived guards that insert separators. Every scalar goes through
//! [`plane_serde::to_writer`] with [`Dialect::Drf`](plane_serde::Dialect::Drf),
//! which is the same code path `plane-drf` uses for a `serde_json::Value`
//! response, so a string or float written here is byte-identical to the one
//! written there. JSON text goes through `plane_serde::transcode_json` and is
//! appended without building a value.

use serde::Serialize;

use crate::DrfError;
use crate::fields::{DrfScalar, FieldValue, PgJson};
use crate::ser::{DrfSerialize, RenderCtx};
use crate::value::DrfValue;

/// The output buffer a serializer writes into.
#[derive(Debug)]
pub struct DrfWriter {
    buf: Vec<u8>,
}

impl Default for DrfWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl DrfWriter {
    #[must_use]
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
        }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    #[must_use]
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }

    /// Append one byte. Only used for structural characters (`{}[],:`).
    pub fn write_byte(&mut self, byte: u8) {
        self.buf.push(byte);
    }

    /// Append `null`.
    pub fn write_null(&mut self) {
        self.buf.extend_from_slice(b"null");
    }

    /// Append a value the way `JSONRenderer` writes it.
    ///
    /// `plane-serde` reproduces `JSONRenderer.render(None) → b""` for a
    /// *top-level* `None` (`serializer.rs` `write_null`: `dialect == Drf &&
    /// depth == 0`). Every call here is top-level, so a field value of `None`
    /// would append nothing and produce `"key":` — invalid JSON and not what
    /// DRF emits inside an object. The only value that serialises to zero bytes
    /// under this dialect is `None`/unit, so an empty result becomes `null`.
    ///
    /// # Errors
    ///
    /// [`DrfError::Json`] for a value DRF's encoder rejects, notably a
    /// non-finite float (`allow_nan=False`).
    pub fn write_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), DrfError> {
        let start = self.buf.len();
        plane_serde::to_writer(plane_serde::Dialect::Drf, &mut self.buf, value)?;
        if self.buf.len() == start {
            self.buf.extend_from_slice(b"null");
        }
        Ok(())
    }

    /// Append the text form of a `json`/`jsonb` column, re-encoded.
    ///
    /// # Errors
    ///
    /// [`DrfError::Json`] when the text is not valid JSON.
    pub fn write_json_text(&mut self, text: &str) -> Result<(), DrfError> {
        plane_serde::transcode_json(plane_serde::Dialect::Drf, text, &mut self.buf)?;
        Ok(())
    }

    /// Start an object. Call [`ObjectWriter::end`] to write `}`.
    pub fn begin_object(&mut self) -> ObjectWriter<'_> {
        self.buf.push(b'{');
        ObjectWriter {
            w: self,
            first: true,
        }
    }

    /// Start an array.
    pub fn begin_array(&mut self) -> ArrayWriter<'_> {
        self.buf.push(b'[');
        ArrayWriter {
            w: self,
            first: true,
        }
    }

    fn nested<T: DrfSerialize + ?Sized>(
        &mut self,
        value: &T,
        ctx: &RenderCtx<'_>,
    ) -> Result<(), DrfError> {
        value.drf_write(self, &RenderCtx::full(ctx.tz()))
    }
}

/// Writes an object's members.
pub struct ObjectWriter<'a> {
    w: &'a mut DrfWriter,
    first: bool,
}

impl<'a> ObjectWriter<'a> {
    /// Write a member name and the following colon.
    ///
    /// # Errors
    ///
    /// [`DrfError::Json`] if the name cannot be encoded (it can; names are
    /// Rust strings).
    pub fn key(&mut self, name: &str) -> Result<(), DrfError> {
        if !self.first {
            self.w.write_byte(b',');
        }
        self.first = false;
        self.w.write_value(name)?;
        self.w.write_byte(b':');
        Ok(())
    }

    /// Write `name: value` with a plain serde value.
    ///
    /// # Errors
    ///
    /// See [`DrfWriter::write_value`].
    pub fn field<T: Serialize + ?Sized>(&mut self, name: &str, value: &T) -> Result<(), DrfError> {
        self.key(name)?;
        self.w.write_value(value)
    }

    /// Write `name: null`.
    ///
    /// # Errors
    ///
    /// See [`ObjectWriter::key`].
    pub fn null_field(&mut self, name: &str) -> Result<(), DrfError> {
        self.key(name)?;
        self.w.write_null();
        Ok(())
    }

    /// Write `name: <json text>`, or `null` when the column is null.
    ///
    /// # Errors
    ///
    /// See [`DrfWriter::write_json_text`] and [`ObjectWriter::key`].
    pub fn field_json(&mut self, name: &str, value: Option<PgJson<'_>>) -> Result<(), DrfError> {
        self.key(name)?;
        match value {
            Some(json) => self.w.write_json_text(json.0),
            None => {
                self.w.write_null();
                Ok(())
            }
        }
    }

    /// Write `name: value` for a [`DrfValue`].
    ///
    /// # Errors
    ///
    /// See [`DrfValue::write`].
    pub fn field_value(
        &mut self,
        name: &str,
        ctx: &RenderCtx<'_>,
        value: &DrfValue<'_>,
    ) -> Result<(), DrfError> {
        self.key(name)?;
        value.write(self.w, ctx)
    }

    /// Write a three-state field: nothing for [`FieldValue::Absent`], `null`
    /// for [`FieldValue::Null`], the value otherwise.
    ///
    /// This is DRF's `to_representation` null rule for a skippable field
    /// (`rest_framework/serializers.py:541-546`). A key is never written for
    /// `Absent`, because `get_attribute` raised `SkipField` before
    /// `to_representation` ran.
    ///
    /// # Errors
    ///
    /// See [`DrfScalar::write_scalar`] and [`ObjectWriter::key`].
    pub fn field_optional<T: DrfScalar>(
        &mut self,
        name: &str,
        ctx: &RenderCtx<'_>,
        value: &FieldValue<T>,
    ) -> Result<(), DrfError> {
        match value {
            FieldValue::Absent => Ok(()),
            FieldValue::Null => self.null_field(name),
            FieldValue::Value(inner) => {
                self.key(name)?;
                inner.write_scalar(self.w, ctx)
            }
        }
    }

    /// [`ObjectWriter::field_optional`], with `expand=` replacement.
    ///
    /// An expansion always wins: `DynamicBaseSerializer.to_representation`
    /// runs the base serialization first (which may skip the key) and then
    /// assigns `response[expand]` (`plane/app/serializers/base.py:129-168`),
    /// so the key appears even when the attribute was missing.
    ///
    /// # Errors
    ///
    /// See [`ObjectWriter::field_optional`] and [`DrfValue::write`].
    pub fn field_optional_dyn<T: DrfScalar>(
        &mut self,
        name: &str,
        ctx: &RenderCtx<'_>,
        value: &FieldValue<T>,
    ) -> Result<(), DrfError> {
        match ctx.expansions().get(name) {
            Some(expansion) => {
                self.key(name)?;
                expansion.write(self.w, ctx)
            }
            None => self.field_optional(name, ctx, value),
        }
    }

    /// Write `name: value` for a nested serializer, `null` for `None`.
    ///
    /// # Errors
    ///
    /// See [`DrfSerialize::drf_write`].
    pub fn field_nested(
        &mut self,
        name: &str,
        ctx: &RenderCtx<'_>,
        value: Option<&(dyn DrfSerialize + '_)>,
    ) -> Result<(), DrfError> {
        self.key(name)?;
        match value {
            Some(value) => self.w.nested(value, ctx),
            None => {
                self.w.write_null();
                Ok(())
            }
        }
    }

    /// Write `name: [...]` for a list of nested serializers.
    ///
    /// # Errors
    ///
    /// See [`DrfSerialize::drf_write`].
    pub fn field_nested_list<T: DrfSerialize>(
        &mut self,
        name: &str,
        ctx: &RenderCtx<'_>,
        values: &[T],
    ) -> Result<(), DrfError> {
        self.key(name)?;
        let mut array = self.w.begin_array();
        for value in values {
            array.nested(ctx, value)?;
        }
        array.end();
        Ok(())
    }

    /// Write a member whose value is replaced by a resolved `expand=` value
    /// when one exists.
    ///
    /// # Errors
    ///
    /// See [`DrfWriter::write_value`] and [`DrfValue::write`].
    pub fn field_dyn<T: Serialize + ?Sized>(
        &mut self,
        name: &str,
        ctx: &RenderCtx<'_>,
        default: &T,
    ) -> Result<(), DrfError> {
        self.key(name)?;
        match ctx.expansions().get(name) {
            Some(value) => value.write(self.w, ctx),
            None => self.w.write_value(default),
        }
    }

    /// [`ObjectWriter::field_json`], with `expand=` replacement.
    ///
    /// # Errors
    ///
    /// See [`DrfWriter::write_json_text`] and [`DrfValue::write`].
    pub fn field_json_dyn(
        &mut self,
        name: &str,
        ctx: &RenderCtx<'_>,
        default: Option<PgJson<'_>>,
    ) -> Result<(), DrfError> {
        self.key(name)?;
        match ctx.expansions().get(name) {
            Some(value) => value.write(self.w, ctx),
            None => match default {
                Some(json) => self.w.write_json_text(json.0),
                None => {
                    self.w.write_null();
                    Ok(())
                }
            },
        }
    }

    /// [`ObjectWriter::field_nested`], with `expand=` replacement.
    ///
    /// # Errors
    ///
    /// See [`DrfSerialize::drf_write`] and [`DrfValue::write`].
    pub fn field_nested_dyn(
        &mut self,
        name: &str,
        ctx: &RenderCtx<'_>,
        default: Option<&(dyn DrfSerialize + '_)>,
    ) -> Result<(), DrfError> {
        self.key(name)?;
        match ctx.expansions().get(name) {
            Some(value) => value.write(self.w, ctx),
            None => match default {
                Some(value) => self.w.nested(value, ctx),
                None => {
                    self.w.write_null();
                    Ok(())
                }
            },
        }
    }

    /// [`ObjectWriter::field_nested_list`], with `expand=` replacement.
    ///
    /// # Errors
    ///
    /// See [`DrfSerialize::drf_write`] and [`DrfValue::write`].
    pub fn field_nested_list_dyn<T: DrfSerialize>(
        &mut self,
        name: &str,
        ctx: &RenderCtx<'_>,
        default: &[T],
    ) -> Result<(), DrfError> {
        self.key(name)?;
        match ctx.expansions().get(name) {
            Some(value) => value.write(self.w, ctx),
            None => {
                let mut array = self.w.begin_array();
                for value in default {
                    array.nested(ctx, value)?;
                }
                array.end();
                Ok(())
            }
        }
    }

    /// [`ObjectWriter::field_value`], with `expand=` replacement.
    ///
    /// # Errors
    ///
    /// See [`DrfValue::write`].
    pub fn field_value_dyn(
        &mut self,
        name: &str,
        ctx: &RenderCtx<'_>,
        default: &DrfValue<'_>,
    ) -> Result<(), DrfError> {
        self.key(name)?;
        match ctx.expansions().get(name) {
            Some(value) => value.write(self.w, ctx),
            None => default.write(self.w, ctx),
        }
    }

    /// Write a value directly (used by [`crate::DrfValue`]).
    ///
    /// # Errors
    ///
    /// See [`DrfValue::write`].
    pub fn value(&mut self, ctx: &RenderCtx<'_>, value: &DrfValue<'_>) -> Result<(), DrfError> {
        value.write(self.w, ctx)
    }

    /// Finish the object.
    pub fn end(self) {
        self.w.write_byte(b'}');
    }
}

/// Writes an array's elements.
pub struct ArrayWriter<'a> {
    w: &'a mut DrfWriter,
    first: bool,
}

impl<'a> ArrayWriter<'a> {
    /// Write one element.
    ///
    /// # Errors
    ///
    /// See [`DrfWriter::write_value`].
    pub fn value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), DrfError> {
        if !self.first {
            self.w.write_byte(b',');
        }
        self.first = false;
        self.w.write_value(value)
    }

    /// Write one [`DrfValue`] element.
    ///
    /// # Errors
    ///
    /// See [`DrfValue::write`].
    pub fn drf_value(&mut self, ctx: &RenderCtx<'_>, value: &DrfValue<'_>) -> Result<(), DrfError> {
        if !self.first {
            self.w.write_byte(b',');
        }
        self.first = false;
        value.write(self.w, ctx)
    }

    /// Write one nested serializer element.
    ///
    /// # Errors
    ///
    /// See [`DrfSerialize::drf_write`].
    pub fn nested<T: DrfSerialize>(
        &mut self,
        ctx: &RenderCtx<'_>,
        value: &T,
    ) -> Result<(), DrfError> {
        if !self.first {
            self.w.write_byte(b',');
        }
        self.first = false;
        self.w.nested(value, ctx)
    }

    /// Finish the array.
    pub fn end(self) {
        self.w.write_byte(b']');
    }
}

#[cfg(test)]
mod tests {
    use super::DrfWriter;

    fn render(value: impl serde::Serialize) -> String {
        let mut writer = DrfWriter::new();
        writer.write_value(&value).expect("write");
        String::from_utf8(writer.into_vec()).expect("utf-8")
    }

    #[test]
    fn a_none_value_is_null_not_empty() {
        assert_eq!(render(Option::<u8>::None), "null");
        assert_eq!(render(Some(7u8)), "7");
    }

    #[test]
    fn object_separators_and_nulls() {
        let mut writer = DrfWriter::new();
        let mut obj = writer.begin_object();
        obj.field("a", &1).expect("a");
        obj.null_field("b").expect("b");
        obj.field("c", "x").expect("c");
        obj.end();
        assert_eq!(
            String::from_utf8(writer.into_vec()).expect("utf-8"),
            "{\"a\":1,\"b\":null,\"c\":\"x\"}"
        );
    }

    #[test]
    fn json_text_is_not_reparsed_through_a_value_tree() {
        let mut writer = DrfWriter::with_capacity(64);
        let mut obj = writer.begin_object();
        obj.field_json("j", Some(crate::PgJson("{\"b\": 1, \"a\": 1e-05}")))
            .expect("json");
        obj.end();
        assert_eq!(
            String::from_utf8(writer.into_vec()).expect("utf-8"),
            "{\"j\":{\"b\":1,\"a\":1e-05}}"
        );
    }
}
