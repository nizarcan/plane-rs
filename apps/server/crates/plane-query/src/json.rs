// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! An order-preserving JSON document.
//!
//! DRF renders a Python `dict`, so the key order of a response is the insertion
//! order of the dict that built it: the paginator envelope is `grouped_by`,
//! `sub_grouped_by`, `total_count`, …, `results`, and every row's keys follow
//! `issue_on_results`'s field list. `serde_json::Value` stores objects in a
//! `BTreeMap` (alphabetical) or, with `preserve_order`, an `IndexMap` — and
//! this workspace deliberately does not enable that feature
//! (`apps/server/Cargo.toml`). [`Json`] is a small document type whose objects
//! are `Vec<(String, Json)>`, serialised through `plane-serde`'s DRF dialect so
//! the bytes are what `JSONRenderer` would produce.
//!
//! ```
//! use plane_query::json::Json;
//!
//! let body = Json::object(vec![
//!     ("total_count", Json::Int(2)),
//!     ("results", Json::Array(vec![])),
//! ]);
//! assert_eq!(body.to_drf(), b"{\"total_count\":2,\"results\":[]}");
//! ```

use chrono::{DateTime, NaiveDate, Utc};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use uuid::Uuid;

use plane_serde::Dialect;
use plane_serde::values::{DrfDateTime, PyDate};

/// A JSON value that remembers object key order.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    /// Rendered by `plane_serde::values::DrfDateTime` (RFC 3339, `Z` for UTC,
    /// microseconds only when non-zero).
    DateTime(DateTime<Utc>),
    /// A datetime already converted to the user's timezone, for the endpoints
    /// that run `user_timezone_converter` (`global_paginator`): the offset is
    /// `+09:00` rather than `Z`.
    DateTimeZoned(DateTime<chrono::FixedOffset>),
    /// Rendered as `YYYY-MM-DD`.
    Date(NaiveDate),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn object(entries: Vec<(&str, Json)>) -> Self {
        Self::Object(
            entries
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value))
                .collect(),
        )
    }

    pub fn string(value: impl Into<String>) -> Self {
        Self::Str(value.into())
    }

    pub fn uuid(value: Uuid) -> Self {
        Self::Str(value.to_string())
    }

    pub fn opt_uuid(value: Option<Uuid>) -> Self {
        value.map_or(Self::Null, Self::uuid)
    }

    pub fn opt_string(value: Option<impl Into<String>>) -> Self {
        value.map_or(Self::Null, |value| Self::string(value))
    }

    pub fn opt_datetime(value: Option<DateTime<Utc>>) -> Self {
        value.map_or(Self::Null, Self::DateTime)
    }

    pub fn opt_date(value: Option<NaiveDate>) -> Self {
        value.map_or(Self::Null, Self::Date)
    }

    /// The rendered DRF bytes (what `JSONRenderer.render` returns).
    pub fn to_drf(&self) -> Vec<u8> {
        plane_serde::to_vec(Dialect::Drf, self).unwrap_or_default()
    }

    /// A scalar rendered by the DRF dialect, as a `String` (no surrounding
    /// quotes). Used by [`Json::to_serde`] for the two formatted variants.
    fn scalar_text(value: &impl Serialize) -> String {
        let bytes = plane_serde::to_vec(Dialect::Drf, value).unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        text.trim_matches('"').to_owned()
    }

    /// Structural conversion, for error details and tests. Dates and datetimes
    /// become their rendered strings, as they are over the wire.
    pub fn to_serde(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Bool(value) => serde_json::Value::Bool(*value),
            Self::Int(value) => serde_json::Value::from(*value),
            Self::Float(value) => serde_json::Value::from(*value),
            Self::Str(value) => serde_json::Value::from(value.clone()),
            Self::DateTime(value) => {
                serde_json::Value::from(Self::scalar_text(&DrfDateTime(*value)))
            }
            Self::DateTimeZoned(value) => {
                serde_json::Value::from(Self::scalar_text(&DrfDateTime(*value)))
            }
            Self::Date(value) => serde_json::Value::from(Self::scalar_text(&PyDate(*value))),
            Self::Array(values) => {
                serde_json::Value::Array(values.iter().map(Self::to_serde).collect())
            }
            Self::Object(entries) => serde_json::Value::Object(
                entries
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_serde()))
                    .collect(),
            ),
        }
    }
}

impl Serialize for Json {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Null => serializer.serialize_none(),
            Self::Bool(value) => serializer.serialize_bool(*value),
            Self::Int(value) => serializer.serialize_i64(*value),
            Self::Float(value) => serializer.serialize_f64(*value),
            Self::Str(value) => serializer.serialize_str(value),
            Self::DateTime(value) => DrfDateTime(*value).serialize(serializer),
            Self::DateTimeZoned(value) => DrfDateTime(*value).serialize(serializer),
            Self::Date(value) => PyDate(*value).serialize(serializer),
            Self::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    seq.serialize_element(value)?;
                }
                seq.end()
            }
            Self::Object(entries) => {
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    map.serialize_entry(key, value)?;
                }
                map.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_order_is_preserved() {
        let value = Json::object(vec![
            ("z", Json::Int(1)),
            ("a", Json::Array(vec![Json::Bool(true), Json::Null])),
            ("m", Json::string("x")),
        ]);
        assert_eq!(value.to_drf(), br#"{"z":1,"a":[true,null],"m":"x"}"#);
    }

    #[test]
    fn datetimes_and_dates_render_like_drf() {
        let dt = DateTime::parse_from_rfc3339("2026-06-15T12:00:00Z")
            .expect("parse")
            .with_timezone(&Utc);
        assert_eq!(Json::DateTime(dt).to_drf(), br#""2026-06-15T12:00:00Z""#);
        let date = NaiveDate::from_ymd_opt(2026, 6, 15).expect("date");
        assert_eq!(Json::Date(date).to_drf(), br#""2026-06-15""#);
        assert_eq!(
            Json::opt_datetime(Some(dt)).to_serde(),
            serde_json::json!("2026-06-15T12:00:00Z")
        );
    }
}
