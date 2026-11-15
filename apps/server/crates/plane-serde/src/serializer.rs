// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! A `serde::Serializer` that writes Python's JSON rather than serde_json's.
//!
//! `serde_json::Serializer` serialises NaN and infinities as `null` before
//! any formatter sees them, which erases the difference between Python's
//! `allow_nan=False` (an error) and `allow_nan=True` (`NaN`/`Infinity`), and
//! it rejects `None` map keys. Owning the dispatch keeps every Python
//! behaviour reachable; the textual rules (separators, escaping, bracket
//! placement) are the same ones `serde_json` implements.

use std::fmt;
use std::io;

use serde::ser::{self, Impossible, Serialize};

use crate::float::PyFloatRepr;
use crate::formatter::{write_escaped_contents, write_json_string};
use crate::{Dialect, Error};

/// Writes `value` as JSON in `dialect` to `writer`.
pub(crate) struct PySerializer<W: io::Write> {
    writer: W,
    dialect: Dialect,
    /// Nesting depth; `Dialect::Drf` renders a top-level `None` as an empty
    /// body, so `null` must know whether it is at the document root.
    depth: u32,
}

impl<W: io::Write> PySerializer<W> {
    pub(crate) fn new(dialect: Dialect, writer: W) -> Self {
        Self {
            writer,
            dialect,
            depth: 0,
        }
    }

    /// Consume the serializer and return the writer (used by `to_vec` so the
    /// output buffer is owned, like `serde_json::to_vec`).
    pub(crate) fn into_writer(self) -> W {
        self.writer
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.writer.write_all(bytes).map_err(Error::Io)
    }

    fn write_str(&mut self, text: &str) -> Result<(), Error> {
        write_json_string(self.dialect, &mut self.writer, text).map_err(Error::Io)
    }

    fn begin_array(&mut self) -> Result<(), Error> {
        self.depth += 1;
        self.write(b"[")
    }

    fn end_array(&mut self) -> Result<(), Error> {
        self.depth = self.depth.saturating_sub(1);
        self.write(b"]")
    }

    fn begin_object(&mut self) -> Result<(), Error> {
        self.depth += 1;
        self.write(b"{")
    }

    fn end_object(&mut self) -> Result<(), Error> {
        self.depth = self.depth.saturating_sub(1);
        self.write(b"}")
    }

    fn begin_array_value(&mut self, first: bool) -> Result<(), Error> {
        if first {
            Ok(())
        } else {
            self.write(self.dialect.item_separator())
        }
    }

    fn begin_object_key(&mut self, first: bool) -> Result<(), Error> {
        if first {
            Ok(())
        } else {
            self.write(self.dialect.item_separator())
        }
    }

    fn end_object_key(&mut self) -> Result<(), Error> {
        self.write(self.dialect.key_separator())
    }

    fn write_int<T: itoa::Integer>(&mut self, value: T) -> Result<(), Error> {
        let mut buffer = itoa::Buffer::new();
        self.write(buffer.format(value).as_bytes())
    }

    fn write_null(&mut self) -> Result<(), Error> {
        if self.dialect == Dialect::Drf && self.depth == 0 {
            // JSONRenderer.render(None) returns b"".
            Ok(())
        } else {
            self.write(b"null")
        }
    }

    /// Python's `floatstr`: `NaN`/`Infinity` under `allow_nan=True`, an error
    /// under `allow_nan=False`, otherwise shortest round-trip digits.
    fn write_float(&mut self, value: f64, quoted: bool) -> Result<(), Error> {
        let special: &str = if value.is_nan() {
            "NaN"
        } else if value == f64::INFINITY {
            "Infinity"
        } else if value == f64::NEG_INFINITY {
            "-Infinity"
        } else {
            return match PyFloatRepr::new(value) {
                Some(repr) if quoted => self.write_str(repr.as_str()),
                Some(repr) => self.write(repr.as_str().as_bytes()),
                None => Err(Error::NonFinite(value)),
            };
        };
        if !self.dialect.allow_nan() {
            return Err(Error::NonFinite(value));
        }
        if quoted {
            self.write_str(special)
        } else {
            self.write(special.as_bytes())
        }
    }

    fn begin_object_with_variant(&mut self, variant: &'static str) -> Result<(), Error> {
        self.begin_object()?;
        self.begin_object_key(true)?;
        self.write_str(variant)?;
        self.end_object_key()
    }
}

impl<'a, W: io::Write> ser::Serializer for &'a mut PySerializer<W> {
    type Ok = ();
    type Error = Error;

    type SerializeSeq = Compound<'a, W>;
    type SerializeTuple = Compound<'a, W>;
    type SerializeTupleStruct = Compound<'a, W>;
    type SerializeTupleVariant = Compound<'a, W>;
    type SerializeMap = Compound<'a, W>;
    type SerializeStruct = Compound<'a, W>;
    type SerializeStructVariant = Compound<'a, W>;

    fn serialize_bool(self, value: bool) -> Result<(), Error> {
        self.write(if value { b"true" } else { b"false" })
    }

    fn serialize_i8(self, value: i8) -> Result<(), Error> {
        self.write_int(value)
    }

    fn serialize_i16(self, value: i16) -> Result<(), Error> {
        self.write_int(value)
    }

    fn serialize_i32(self, value: i32) -> Result<(), Error> {
        self.write_int(value)
    }

    fn serialize_i64(self, value: i64) -> Result<(), Error> {
        self.write_int(value)
    }

    fn serialize_i128(self, value: i128) -> Result<(), Error> {
        self.write_int(value)
    }

    fn serialize_u8(self, value: u8) -> Result<(), Error> {
        self.write_int(value)
    }

    fn serialize_u16(self, value: u16) -> Result<(), Error> {
        self.write_int(value)
    }

    fn serialize_u32(self, value: u32) -> Result<(), Error> {
        self.write_int(value)
    }

    fn serialize_u64(self, value: u64) -> Result<(), Error> {
        self.write_int(value)
    }

    fn serialize_u128(self, value: u128) -> Result<(), Error> {
        self.write_int(value)
    }

    fn serialize_f32(self, value: f32) -> Result<(), Error> {
        self.write_float(f64::from(value), false)
    }

    fn serialize_f64(self, value: f64) -> Result<(), Error> {
        self.write_float(value, false)
    }

    fn serialize_char(self, value: char) -> Result<(), Error> {
        let mut buffer = [0u8; 4];
        self.serialize_str(value.encode_utf8(&mut buffer))
    }

    fn serialize_str(self, value: &str) -> Result<(), Error> {
        self.write_str(value)
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<(), Error> {
        match self.dialect {
            // rest_framework.utils.encoders.JSONEncoder: obj.decode()
            Dialect::Drf => match std::str::from_utf8(value) {
                Ok(text) => self.serialize_str(text),
                Err(_) => Err(Error::Custom(
                    "'utf-8' codec can't decode bytes in position 0".to_owned(),
                )),
            },
            // django.core.serializers.json.DjangoJSONEncoder has no bytes
            // case: json.dumps raises TypeError.
            Dialect::Django => Err(Error::Custom(
                "Object of type bytes is not JSON serializable".to_owned(),
            )),
            // kombu.utils.json.JSONEncoder's bytes/base64 envelopes.
            Dialect::Kombu => crate::values::KombuBytes(value.to_vec()).serialize(self),
        }
    }

    fn serialize_none(self) -> Result<(), Error> {
        self.write_null()
    }

    fn serialize_some<T>(self, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Error> {
        self.write_null()
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Error> {
        self.write_null()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<(), Error> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        self.begin_object_with_variant(variant)?;
        value.serialize(&mut *self)?;
        self.end_object()
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        self.begin_array()?;
        Ok(Compound::Array {
            serializer: self,
            first: true,
        })
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Error> {
        self.serialize_seq(None)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        self.serialize_seq(None)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        self.begin_object_with_variant(variant)?;
        self.begin_array()?;
        Ok(Compound::TupleVariant {
            serializer: self,
            first: true,
        })
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        self.begin_object()?;
        Ok(Compound::Object {
            serializer: self,
            first: true,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        self.serialize_map(None)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        self.begin_object_with_variant(variant)?;
        self.begin_object()?;
        Ok(Compound::StructVariant {
            serializer: self,
            first: true,
        })
    }

    fn collect_str<T>(self, value: &T) -> Result<(), Error>
    where
        T: ?Sized + fmt::Display,
    {
        struct Adapter<'a, W: io::Write> {
            serializer: &'a mut PySerializer<W>,
            error: Option<io::Error>,
        }

        impl<W: io::Write> fmt::Write for Adapter<'_, W> {
            fn write_str(&mut self, text: &str) -> fmt::Result {
                match write_escaped_contents(
                    self.serializer.dialect,
                    &mut self.serializer.writer,
                    text,
                ) {
                    Ok(()) => Ok(()),
                    Err(error) => {
                        self.error = Some(error);
                        Err(fmt::Error)
                    }
                }
            }
        }

        self.write(b"\"")?;
        let mut adapter = Adapter {
            serializer: self,
            error: None,
        };
        let displayed = fmt::write(&mut adapter, format_args!("{value}"));
        if let Some(error) = adapter.error {
            return Err(Error::Io(error));
        }
        displayed.map_err(|_| Error::Custom("failed to format value".to_owned()))?;
        adapter.serializer.write(b"\"")
    }

    fn is_human_readable(&self) -> bool {
        true
    }
}

/// The in-progress compound value.
pub(crate) enum Compound<'a, W: io::Write> {
    Array {
        serializer: &'a mut PySerializer<W>,
        first: bool,
    },
    Object {
        serializer: &'a mut PySerializer<W>,
        first: bool,
    },
    TupleVariant {
        serializer: &'a mut PySerializer<W>,
        first: bool,
    },
    StructVariant {
        serializer: &'a mut PySerializer<W>,
        first: bool,
    },
}

impl<W: io::Write> Compound<'_, W> {
    fn array_element<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        match self {
            Compound::Array { serializer, first } => {
                serializer.begin_array_value(*first)?;
                *first = false;
                value.serialize(&mut **serializer)
            }
            _ => unreachable!("array element outside an array"),
        }
    }

    fn array_end(self) -> Result<(), Error> {
        match self {
            Compound::Array { serializer, .. } => serializer.end_array(),
            _ => unreachable!("array end outside an array"),
        }
    }

    fn tuple_variant_element<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        match self {
            Compound::TupleVariant { serializer, first } => {
                serializer.begin_array_value(*first)?;
                *first = false;
                value.serialize(&mut **serializer)
            }
            _ => unreachable!("tuple variant element outside a tuple variant"),
        }
    }

    fn tuple_variant_end(self) -> Result<(), Error> {
        match self {
            Compound::TupleVariant { serializer, .. } => {
                serializer.end_array()?;
                serializer.end_object()
            }
            _ => unreachable!("tuple variant end outside a tuple variant"),
        }
    }

    fn object_entry<T>(&mut self, key: &str, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        match self {
            Compound::Object { serializer, first } => {
                serializer.begin_object_key(*first)?;
                *first = false;
                serializer.write_str(key)?;
                serializer.end_object_key()?;
                value.serialize(&mut **serializer)
            }
            _ => unreachable!("object entry outside an object"),
        }
    }

    fn object_end(self) -> Result<(), Error> {
        match self {
            Compound::Object { serializer, .. } => serializer.end_object(),
            _ => unreachable!("object end outside an object"),
        }
    }

    fn struct_variant_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        match self {
            Compound::StructVariant { serializer, first } => {
                serializer.begin_object_key(*first)?;
                *first = false;
                serializer.write_str(key)?;
                serializer.end_object_key()?;
                value.serialize(&mut **serializer)
            }
            _ => unreachable!("struct variant field outside a struct variant"),
        }
    }

    fn struct_variant_end(self) -> Result<(), Error> {
        match self {
            Compound::StructVariant { serializer, .. } => {
                serializer.end_object()?;
                serializer.end_object()
            }
            _ => unreachable!("struct variant end outside a struct variant"),
        }
    }
}

impl<W: io::Write> ser::SerializeSeq for Compound<'_, W> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        self.array_element(value)
    }

    fn end(self) -> Result<(), Error> {
        self.array_end()
    }
}

impl<W: io::Write> ser::SerializeTuple for Compound<'_, W> {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        self.array_element(value)
    }

    fn end(self) -> Result<(), Error> {
        self.array_end()
    }
}

impl<W: io::Write> ser::SerializeTupleStruct for Compound<'_, W> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        self.array_element(value)
    }

    fn end(self) -> Result<(), Error> {
        self.array_end()
    }
}

impl<W: io::Write> ser::SerializeTupleVariant for Compound<'_, W> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        self.tuple_variant_element(value)
    }

    fn end(self) -> Result<(), Error> {
        self.tuple_variant_end()
    }
}

impl<W: io::Write> ser::SerializeMap for Compound<'_, W> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        match self {
            Compound::Object { serializer, first } => {
                serializer.begin_object_key(*first)?;
                *first = false;
                key.serialize(MapKeySerializer {
                    serializer: &mut **serializer,
                })?;
                serializer.end_object_key()
            }
            _ => unreachable!("map key outside a map"),
        }
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        match self {
            Compound::Object { serializer, .. } => value.serialize(&mut **serializer),
            _ => unreachable!("map value outside a map"),
        }
    }

    fn end(self) -> Result<(), Error> {
        self.object_end()
    }
}

impl<W: io::Write> ser::SerializeStruct for Compound<'_, W> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        self.object_entry(key, value)
    }

    fn end(self) -> Result<(), Error> {
        self.object_end()
    }
}

impl<W: io::Write> ser::SerializeStructVariant for Compound<'_, W> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        self.struct_variant_field(key, value)
    }

    fn end(self) -> Result<(), Error> {
        self.struct_variant_end()
    }
}

/// Serialises a map key the way Python's `json` module stringifies keys:
/// strings as themselves, integers and floats in their `repr`, booleans as
/// `true`/`false`, and `None` as `null`.
struct MapKeySerializer<'a, W: io::Write> {
    serializer: &'a mut PySerializer<W>,
}

impl<W: io::Write> MapKeySerializer<'_, W> {
    fn quoted_key<F>(self, write: F) -> Result<(), Error>
    where
        F: FnOnce(&mut W) -> io::Result<()>,
    {
        self.serializer.write(b"\"")?;
        write(&mut self.serializer.writer).map_err(Error::Io)?;
        self.serializer.write(b"\"")
    }
}

impl<W: io::Write> ser::Serializer for MapKeySerializer<'_, W> {
    type Ok = ();
    type Error = Error;

    type SerializeSeq = Impossible<(), Error>;
    type SerializeTuple = Impossible<(), Error>;
    type SerializeTupleStruct = Impossible<(), Error>;
    type SerializeTupleVariant = Impossible<(), Error>;
    type SerializeMap = Impossible<(), Error>;
    type SerializeStruct = Impossible<(), Error>;
    type SerializeStructVariant = Impossible<(), Error>;

    fn serialize_str(self, value: &str) -> Result<(), Error> {
        self.serializer.write_str(value)
    }

    fn serialize_bool(self, value: bool) -> Result<(), Error> {
        self.serializer
            .write_str(if value { "true" } else { "false" })
    }

    fn serialize_i8(self, value: i8) -> Result<(), Error> {
        self.quoted_key(|writer| write!(writer, "{value}"))
    }

    fn serialize_i16(self, value: i16) -> Result<(), Error> {
        self.quoted_key(|writer| write!(writer, "{value}"))
    }

    fn serialize_i32(self, value: i32) -> Result<(), Error> {
        self.quoted_key(|writer| write!(writer, "{value}"))
    }

    fn serialize_i64(self, value: i64) -> Result<(), Error> {
        self.quoted_key(|writer| write!(writer, "{value}"))
    }

    fn serialize_i128(self, value: i128) -> Result<(), Error> {
        self.quoted_key(|writer| write!(writer, "{value}"))
    }

    fn serialize_u8(self, value: u8) -> Result<(), Error> {
        self.quoted_key(|writer| write!(writer, "{value}"))
    }

    fn serialize_u16(self, value: u16) -> Result<(), Error> {
        self.quoted_key(|writer| write!(writer, "{value}"))
    }

    fn serialize_u32(self, value: u32) -> Result<(), Error> {
        self.quoted_key(|writer| write!(writer, "{value}"))
    }

    fn serialize_u64(self, value: u64) -> Result<(), Error> {
        self.quoted_key(|writer| write!(writer, "{value}"))
    }

    fn serialize_u128(self, value: u128) -> Result<(), Error> {
        self.quoted_key(|writer| write!(writer, "{value}"))
    }

    fn serialize_f32(self, value: f32) -> Result<(), Error> {
        self.serializer.write_float(f64::from(value), true)
    }

    fn serialize_f64(self, value: f64) -> Result<(), Error> {
        self.serializer.write_float(value, true)
    }

    fn serialize_char(self, value: char) -> Result<(), Error> {
        let mut buffer = [0u8; 4];
        self.serialize_str(value.encode_utf8(&mut buffer))
    }

    fn serialize_bytes(self, _value: &[u8]) -> Result<(), Error> {
        Err(Error::Custom(
            "keys must be str, int, float, bool or None, not bytes".to_owned(),
        ))
    }

    fn serialize_none(self) -> Result<(), Error> {
        self.serializer.write_str("null")
    }

    fn serialize_some<T>(self, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Error> {
        self.serialize_none()
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Error> {
        self.serialize_none()
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<(), Error> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T>(self, _name: &'static str, value: &T) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<(), Error>
    where
        T: ?Sized + Serialize,
    {
        Err(Error::Custom(
            "keys must be str, int, float, bool or None".to_owned(),
        ))
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        Err(Error::Custom(
            "keys must be str, int, float, bool or None".to_owned(),
        ))
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Error> {
        self.serialize_seq(None)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        self.serialize_seq(None)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        self.serialize_seq(None)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap, Error> {
        self.serialize_seq(None)
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        self.serialize_seq(None)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        self.serialize_seq(None)
    }
}
