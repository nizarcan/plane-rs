// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! One `.values()` row, rendered the way DRF's `JSONEncoder` renders a dict.
//!
//! The three search endpoints return `Response(queryset.values(...))` (or a
//! list built from one), not a serializer, so the values go through
//! `rest_framework.utils.encoders.JSONEncoder`:
//!
//! | Python value | JSON |
//! | --- | --- |
//! | `None` | `null` |
//! | `bool` / `int` | number |
//! | `str` | string |
//! | `uuid.UUID` | `str(uuid)` |
//! | `datetime` | `isoformat()` with `+00:00` → `Z` (UTC, as psycopg2 returns it) |
//! | `datetime.date` | `YYYY-MM-DD` |
//! | a `jsonb` column | the decoded dict, re-encoded with DRF's compact separators |
//!
//! [`RowValue`] keeps the key order of the `.values()` call — the key order of
//! the response — and writes each cell through the same wrappers a generated
//! serializer uses, so a datetime or a UUID renders byte-identically.

use chrono::NaiveDate;
use plane_db::JsonText;
use plane_serde::values::PyDate;
use plane_serializers::{DrfError, DrfSerialize, DrfWriter, PgJson, RenderCtx};
use uuid::Uuid;

/// One cell of a `.values()` row.
#[derive(Debug)]
pub(crate) enum Cell {
    Null,
    Int(i64),
    Text(String),
    Uuid(Uuid),
    /// A `DateField` value.
    Date(NaiveDate),
    /// A `jsonb`/`json` column.
    Json(JsonText),
    UuidList(Vec<Uuid>),
    TextList(Vec<String>),
}

/// One `.values()` dict, in key order.
#[derive(Debug)]
pub(crate) struct RowValue {
    fields: Vec<(&'static str, Cell)>,
}

impl RowValue {
    pub(crate) fn new(fields: Vec<(&'static str, Cell)>) -> Self {
        Self { fields }
    }
}

impl DrfSerialize for RowValue {
    fn drf_write(&self, w: &mut DrfWriter, _ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        for (key, cell) in &self.fields {
            match cell {
                Cell::Null => obj.null_field(key)?,
                Cell::Int(value) => obj.field(key, value)?,
                Cell::Text(value) => obj.field(key, value)?,
                Cell::Uuid(value) => obj.field(key, &plane_serializers::DrfUuid(*value))?,
                Cell::Date(value) => obj.field(key, &PyDate(*value))?,
                Cell::Json(text) => obj.field_json(key, Some(PgJson(text)))?,
                Cell::UuidList(values) => obj.field(
                    key,
                    &values
                        .iter()
                        .copied()
                        .map(plane_serializers::DrfUuid)
                        .collect::<Vec<_>>(),
                )?,
                Cell::TextList(values) => obj.field(key, values)?,
            }
        }
        obj.end();
        Ok(())
    }
}

/// The envelope of `GlobalSearchEndpoint`: `{"results": {entity: [...]}}`,
/// with the entity keys in request order (`base.py:319-326`).
#[derive(Debug)]
pub(crate) struct Results(pub(crate) Vec<(String, Vec<RowValue>)>);

impl DrfSerialize for Results {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        obj.field_nested("results", ctx, Some(&ResultsObject(&self.0)))?;
        obj.end();
        Ok(())
    }
}

/// The `results` object itself, one key per entity.
#[derive(Debug)]
struct ResultsObject<'a>(&'a [(String, Vec<RowValue>)]);

impl DrfSerialize for ResultsObject<'_> {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        for (entity, rows) in self.0 {
            obj.field_nested_list(entity, ctx, rows)?;
        }
        obj.end();
        Ok(())
    }
}

/// The envelope of `SearchEndpoint`: one key per requested `query_type`, in
/// order, each a list (`base.py:339`).
#[derive(Debug)]
pub(crate) struct TypedResults(pub(crate) Vec<(String, Vec<RowValue>)>);

impl DrfSerialize for TypedResults {
    fn drf_write(&self, w: &mut DrfWriter, ctx: &RenderCtx<'_>) -> Result<(), DrfError> {
        let mut obj = w.begin_object();
        for (query_type, rows) in &self.0 {
            obj.field_nested_list(query_type, ctx, rows)?;
        }
        obj.end();
        Ok(())
    }
}
