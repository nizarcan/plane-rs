// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The write side of a DRF serializer: field order, `partial=True`, the error
//! shape and the validated-value bag.
//!
//! Python: `Serializer.to_internal_value` (`rest_framework/serializers.py:
//! 493-529`). The rules that show up in a response:
//!
//! * fields are visited in `self.fields` order — the declaration order the
//!   generated read serializers already carry — so the error keys are in that
//!   order, not the request's;
//! * a field that fails does not stop the others: every failing field is
//!   reported;
//! * `read_only` fields are skipped without reading the body;
//! * `partial=True` (PATCH) skips every absent field, including required ones;
//! * a body that is not a mapping is a `non_field_errors` entry — and
//!   `data=None` is DRF's "No data provided" special case
//!   (`serializers.py:589-596`).
//!
//! The body itself was parsed by `plane-drf` (`ctx.data()`), which reproduces
//! DRF's `Request._parse`: JSON text/`ParseError`, the 415 for a
//! wrong `Content-Type`, forms and multipart as a `QueryDict`.

use chrono::NaiveDate;
use deadpool_postgres::Client;
use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::ViewCtx;
use plane_serde::pyjson::{self, PyDict, PyStr, Value as Json};

use crate::write::fields::{self, CharOptions, Detail, Raw};

/// One field's validated value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Text(String),
    Uuid(uuid::Uuid),
    Json(JsonText),
    Float(f64),
    DateTime(chrono::DateTime<chrono::Utc>),
    Date(NaiveDate),
    UuidList(Vec<uuid::Uuid>),
}

/// The `validated_data` dict, in the serializer's field order.
#[derive(Debug, Clone, Default)]
pub struct Validated {
    values: Vec<(&'static str, Value)>,
}

impl Validated {
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.values
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// The text of a `CharField` value (`None` when absent or `null`).
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name) {
            Some(Value::Text(value)) => Some(value),
            _ => None,
        }
    }

    /// A boolean value.
    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.get(name) {
            Some(Value::Bool(value)) => Some(*value),
            _ => None,
        }
    }

    /// An integer value (`IntegerField`, `ChoiceField` over ints).
    pub fn integer(&self, name: &str) -> Option<i64> {
        match self.get(name) {
            Some(Value::Int(value)) => Some(*value),
            _ => None,
        }
    }

    /// A UUID value (`PrimaryKeyRelatedField`).
    pub fn uuid(&self, name: &str) -> Option<uuid::Uuid> {
        match self.get(name) {
            Some(Value::Uuid(value)) => Some(*value),
            _ => None,
        }
    }

    /// The `JsonText` of a `JSONField` value.
    pub fn json(&self, name: &str) -> Option<&JsonText> {
        match self.get(name) {
            Some(Value::Json(value)) => Some(value),
            _ => None,
        }
    }

    /// A `DateField` value.
    pub fn date(&self, name: &str) -> Option<NaiveDate> {
        match self.get(name) {
            Some(Value::Date(value)) => Some(*value),
            _ => None,
        }
    }

    /// A `ListField`/`ManyRelatedField` of primary keys.
    pub fn uuid_list(&self, name: &str) -> Option<&[uuid::Uuid]> {
        match self.get(name) {
            Some(Value::UuidList(value)) => Some(value),
            _ => None,
        }
    }

    pub fn float(&self, name: &str) -> Option<f64> {
        match self.get(name) {
            Some(Value::Float(value)) => Some(*value),
            _ => None,
        }
    }

    pub fn datetime(&self, name: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        match self.get(name) {
            Some(Value::DateTime(value)) => Some(*value),
            _ => None,
        }
    }

    /// Whether the field was present with an explicit `null`.
    pub fn is_null(&self, name: &str) -> bool {
        matches!(self.get(name), Some(Value::Null))
    }
}

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<i16> for Value {
    fn from(value: i16) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Self::Int(i64::from(value))
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::Int(value)
    }
}

impl From<uuid::Uuid> for Value {
    fn from(value: uuid::Uuid) -> Self {
        Self::Uuid(value)
    }
}

impl From<NaiveDate> for Value {
    fn from(value: NaiveDate) -> Self {
        Self::Date(value)
    }
}

/// The value of one field in `serializer.errors`.
///
/// A `ListField` collects child errors as a dict keyed by the element's index
/// (`{"1": ["…"]}` in the body); every other field is a list of messages.
#[derive(Debug)]
enum FieldErrors {
    Messages(Vec<Detail>),
    Indexed(Vec<(usize, Vec<Detail>)>),
    /// A `ValidationError(dict)` raised by a `validate_<field>` method, which
    /// DRF assigns to the field verbatim instead of wrapping it in a list
    /// (`rest_framework/serializers.py:430-445`): `validate_url`'s
    /// `{"error": "Invalid URL format."}` renders as
    /// `{"url": {"error": "Invalid URL format."}}`.
    Body(plane_query::json::Json),
}

/// Options for a `ListField` of primary keys.
#[derive(Debug, Clone, Copy)]
pub struct PrimaryKeyListOptions {
    pub allow_null: bool,
    pub allow_empty: bool,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub table: &'static str,
}

/// `serializer.errors`: ordered `(field, details)` pairs.
#[derive(Debug, Default)]
pub struct ValidationErrors(Vec<(&'static str, FieldErrors)>);

impl ValidationErrors {
    pub fn new(field: &'static str, detail: Detail) -> Self {
        Self(vec![(field, FieldErrors::Messages(vec![detail]))])
    }

    pub fn push(&mut self, field: &'static str, detail: Detail) {
        match self.0.iter_mut().find(|(name, _)| *name == field) {
            Some((_, FieldErrors::Messages(details))) => details.push(detail),
            // A top-level list error returns before child validation, so the
            // two shapes cannot coexist for one field; keep the first.
            Some((_, FieldErrors::Indexed(_) | FieldErrors::Body(_))) => {}
            None => self.0.push((field, FieldErrors::Messages(vec![detail]))),
        }
    }

    /// A field error whose body is a JSON document rather than a message list
    /// (a `validate_<field>` that raised `ValidationError(dict)`).
    pub fn push_body(&mut self, field: &'static str, body: plane_query::json::Json) {
        if self.0.iter().any(|(name, _)| *name == field) {
            return;
        }
        self.0.push((field, FieldErrors::Body(body)));
    }

    /// A `ListField` child error: DRF raises `ValidationError({idx: detail})`
    /// (`fields.py:1720-1735`), which renders as an object keyed by index.
    pub fn push_indexed(&mut self, field: &'static str, index: usize, detail: Detail) {
        if let Some((_, details)) = self.0.iter_mut().find(|(name, _)| *name == field) {
            if let FieldErrors::Indexed(details) = details {
                match details.iter_mut().find(|(slot, _)| *slot == index) {
                    Some((_, slot)) => slot.push(detail),
                    None => details.push((index, vec![detail])),
                }
            }
            return;
        }
        self.0
            .push((field, FieldErrors::Indexed(vec![(index, vec![detail])])));
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Stable-sort the errors into the serializer's `self.fields` order.
    ///
    /// A re-check made after `to_internal_value` appends its field at the end;
    /// DRF's body has the keys in `_readable_fields` order. Fields not in the
    /// list (`non_field_errors`, a `validate()` field) stay last.
    pub fn sort_by_field_order(&mut self, order: &[&str]) {
        let position = |name: &str| order.iter().position(|field| *field == name);
        self.0
            .sort_by_key(|(name, _)| position(name).unwrap_or(order.len()));
    }

    /// The 400 `Response(serializer.errors)` body.
    ///
    /// Keys are in field order, each value is a JSON array of strings (or, for
    /// a list field's child errors, an object keyed by the element index):
    /// `{"color": ["Ensure this field has no more than 255 characters."]}` or
    /// `{"label_ids": {"0": ["Invalid pk \"…\" - object does not exist."]}}`.
    #[must_use]
    pub fn into_response(self) -> plane_drf::view::ViewResponse {
        use plane_query::json::Json as Doc;
        let messages = |details: Vec<Detail>| -> Doc {
            Doc::Array(
                details
                    .into_iter()
                    .map(|detail| Doc::Str(detail.message))
                    .collect(),
            )
        };
        let entries: Vec<(String, Doc)> = self
            .0
            .into_iter()
            .map(|(field, errors)| {
                let value = match errors {
                    FieldErrors::Messages(details) => messages(details),
                    FieldErrors::Indexed(items) => Doc::Object(
                        items
                            .into_iter()
                            .map(|(index, details)| (index.to_string(), messages(details)))
                            .collect(),
                    ),
                    FieldErrors::Body(body) => body,
                };
                (field.to_owned(), value)
            })
            .collect();
        plane_drf::view::ViewResponse::raw(
            400,
            plane_core::error::APPLICATION_JSON,
            Doc::Object(entries).to_drf(),
        )
    }
}

/// One request body, as DRF's field `get_value` sees it.
#[derive(Debug)]
enum Body<'a> {
    /// A JSON object.
    Json(&'a PyDict),
    /// A form or multipart body: every value is a string
    /// (`html.is_html_input(dictionary)` is true for a `QueryDict`).
    Form(&'a plane_drf::QueryDict),
    /// `{**request.data}` over a `QueryDict` (`projects` create/PATCH): the
    /// raw `MultiValueDict` entries, i.e. one **list** of strings per key. Owned
    /// because the view builds it per request.
    Expanded {
        object: Box<PyDict>,
        /// The file parts' field names: `request.FILES` is merged into the
        /// mapping, with the file object in place of the field's values.
        file_keys: Vec<String>,
    },
}

impl Body<'_> {
    /// `Field.get_value` (`rest_framework/fields.py:394-418`) for the html
    /// branch: an empty string means "blank" — `None` when the field allows
    /// null, "absent" when it is not required and does not allow blank.
    fn raw(&self, name: &str) -> Option<Raw<'_>> {
        match self {
            Self::Form(query) => query.get(name).map(Raw::Text),
            Self::Json(object) => object.get(name).map(Raw::Json),
            Self::Expanded { object, .. } => object.get(name).map(Raw::Json),
        }
    }

    /// Whether this body carried a multipart file part under `name`
    /// (`request.data` holds the file object, which `json.dumps` refuses).
    fn has_file(&self, name: &str) -> bool {
        match self {
            Self::Json(_) => false,
            Self::Form(query) => query.is_file(name),
            Self::Expanded { file_keys, .. } => file_keys.iter().any(|key| key == name),
        }
    }
}

/// A serializer being validated.
#[derive(Debug)]
pub struct Serializer<'a> {
    body: Option<Body<'a>>,
    partial: bool,
    /// `html.is_html_input(dictionary)`.
    form: bool,
    errors: ValidationErrors,
    values: Vec<(&'static str, Value)>,
    /// Fields the serializer made read-only for this request (`get_fields`):
    /// DRF skips them in `to_internal_value` without reading the body.
    skipped: Vec<&'static str>,
    /// The `non_field_errors` a non-mapping body produced; kept so field calls
    /// become no-ops and `finish()` reports it.
    fatal: bool,
    /// The active timezone for a naive datetime (`enforce_timezone`).
    timezone: chrono_tz::Tz,
}

impl<'a> Serializer<'a> {
    /// Start validating `request.data`.
    ///
    /// # Errors
    ///
    /// A parser failure: DRF's `ParseError` (400) or `UnsupportedMediaType`
    /// (415), both as `ViewError::Api`.
    pub fn new(ctx: &'a ViewCtx<'a>, partial: bool) -> Result<Self, ViewError> {
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let mut serializer = Self {
            body: None,
            partial,
            form: false,
            errors: ValidationErrors::default(),
            values: Vec::new(),
            skipped: Vec::new(),
            fatal: false,
            timezone,
        };
        match ctx.data()? {
            Data::Json(Json::Dict(object)) => {
                serializer.body = Some(Body::Json(object));
            }
            Data::Form(query) => {
                serializer.body = Some(Body::Form(query));
                serializer.form = true;
            }
            // `Serializer(data=None).errors` → `{"non_field_errors":
            // ["No data provided"]}` (`serializers.py:589-596`).
            Data::Json(Json::Null) => {
                serializer.fatal = true;
                serializer
                    .errors
                    .push("non_field_errors", Detail::new("No data provided", "null"));
            }
            Data::Json(value) => {
                serializer.fatal = true;
                serializer.errors.push(
                    "non_field_errors",
                    Detail::new(
                        format!(
                            "Invalid data. Expected a dictionary, but got {}.",
                            python_type_name(value)
                        ),
                        "invalid",
                    ),
                );
            }
        }
        Ok(serializer)
    }

    /// The `partial=True` flag (PATCH).
    #[must_use]
    pub fn partial(&self) -> bool {
        self.partial
    }

    /// Start validating `{**request.data}` — the mapping the project views
    /// build before handing it to the serializer
    /// (`plane/app/views/project/base.py:263,352`).
    ///
    /// `**` copies a Python mapping's raw entries. For a JSON object that is
    /// the same tree `Serializer::new` reads. For a `QueryDict` it is **not**
    /// `.items()`: `MultiValueDict` stores a list per key, so every field sees
    /// `["value", …]` and DRF's field parsers report their list-input errors
    /// (`CharField` → "Not a valid string.", `IntegerField` → "A valid integer
    /// is required.", `JSONField` accepts the list).
    ///
    /// # Errors
    ///
    /// A parser failure (400/415), or the `TypeError` `{**request.data}`
    /// raises when `request.data` is not a mapping (family 500).
    pub fn new_expanded(ctx: &'a ViewCtx<'a>, partial: bool) -> Result<Self, ViewError> {
        let timezone = ctx.timezone.copied().unwrap_or(chrono_tz::UTC);
        let mut serializer = Self {
            body: None,
            partial,
            form: false,
            errors: ValidationErrors::default(),
            values: Vec::new(),
            skipped: Vec::new(),
            fatal: false,
            timezone,
        };
        match ctx.data()? {
            Data::Json(Json::Dict(object)) => {
                serializer.body = Some(Body::Json(object));
            }
            Data::Form(query) => {
                let mut dict = PyDict::new();
                let mut seen = std::collections::BTreeSet::new();
                for key in query.keys() {
                    if !seen.insert(key) {
                        continue;
                    }
                    let values = query
                        .getlist(key)
                        .into_iter()
                        .map(Json::from)
                        .collect::<Vec<_>>();
                    dict.insert(PyStr::new(key), Json::List(values));
                }
                // `_full_data.update(self._files)`: a file part replaces any
                // same-named field's values with a one-element list holding
                // the file object. The bytes are not retained; the field name
                // is what lets a `JSONField` reproduce `json.dumps`'s refusal
                // and every other parser see a non-string element.
                let file_keys = query.file_keys().to_vec();
                for key in &file_keys {
                    dict.insert(PyStr::new(key.as_str()), Json::List(vec![Json::Null]));
                }
                serializer.body = Some(Body::Expanded {
                    object: Box::new(dict),
                    file_keys,
                });
            }
            Data::Json(value) => {
                // `{**request.data}` on a list/scalar/None is a `TypeError`
                // before the serializer is built.
                return Err(ViewError::Unknown(format!(
                    "TypeError: '{}' object is not a mapping",
                    python_type_name(value)
                )));
            }
        }
        Ok(serializer)
    }

    /// Make one field read-only for this request.
    ///
    /// Python's `ModelSerializer.get_fields` can flip a generated field's
    /// `read_only` per instance (`IssueCreateSerializer.get_fields` does it for
    /// `is_draft` on PATCH, `plane/app/serializers/issue.py:126-131`). DRF's
    /// `to_internal_value` skips read-only fields without reading the body, so
    /// the field is neither validated nor added to `validated_data`.
    pub fn skip_field(&mut self, name: &'static str) {
        if !self.skipped.contains(&name) {
            self.skipped.push(name);
        }
    }

    /// `CharField` (`rest_framework/fields.py:718-762`).
    pub fn char_field(&mut self, name: &'static str, options: CharOptions) -> Option<String> {
        let raw = self.field_value(name, options.allow_null, Some(options.allow_blank))?;
        match fields::char(raw, &options) {
            Ok(value) => {
                self.values.push((name, Value::Text(value.clone())));
                Some(value)
            }
            Err(detail) => {
                self.errors.push(name, detail);
                None
            }
        }
    }

    /// `FloatField` (finite numbers only).
    pub fn float_field(&mut self, name: &'static str, allow_null: bool) -> Option<f64> {
        // `FloatField` has no `allow_blank`, so an empty form value is skipped
        // (not required) rather than parsed.
        let raw = self.field_value(name, allow_null, Some(false))?;
        match fields::float(raw) {
            Ok(value) => {
                self.values.push((name, Value::Float(value)));
                Some(value)
            }
            Err(detail) => {
                self.errors.push(name, detail);
                None
            }
        }
    }

    /// `BooleanField` (`rest_framework/fields.py:766-790`).
    pub fn bool_field(&mut self, name: &'static str) -> Option<bool> {
        self.field(name, false, fields::boolean)
    }

    /// `BooleanField` over a value the *view* supplies rather than the body —
    /// `ProjectViewSet.partial_update`'s `{**request.data, "intake_view":
    /// request.data.get("inbox_view", project.intake_view)}`
    /// (`plane/app/views/project/base.py:342-355`).
    ///
    /// `raw` carries the injected value: a JSON value for a JSON body, the
    /// form value for a form (`''` is DRF's `empty`, i.e. skipped).
    pub fn bool_field_value(&mut self, name: &'static str, raw: fields::Raw<'_>) -> Option<bool> {
        if self.fatal {
            return None;
        }
        if let fields::Raw::Json(Json::Null) = raw {
            self.errors.push(name, Detail::null());
            return None;
        }
        if let fields::Raw::Text("") = raw {
            // `get_value`'s html branch: `''` on a non-required field without
            // `allow_blank` is `empty` (`rest_framework/fields.py:394-418`).
            return None;
        }
        match fields::boolean(raw) {
            Ok(value) => {
                self.values.push((name, Value::Bool(value)));
                Some(value)
            }
            Err(detail) => {
                self.errors.push(name, detail);
                None
            }
        }
    }

    /// `ChoiceField` over integer choices (`network`): the input is compared as
    /// `str(data)` and the field returns the choice key.
    pub fn integer_choice_field(
        &mut self,
        name: &'static str,
        allowed: &'static [i16],
    ) -> Option<i16> {
        let parsed = self.field(name, false, move |raw| fields::integer_choice(raw, allowed))?;
        // `field` stored the int64; a choice is an int16 model column.
        Some(parsed)
    }

    /// `PrimaryKeyRelatedField` (`rest_framework/relations.py:250-290`): parse
    /// the pk, then one `SELECT 1 FROM <queryset model> WHERE id = $1`. Django
    /// runs exactly one `queryset.get(pk=…)` per field, so the round-trip count
    /// is the same.
    pub async fn primary_key_field(
        &mut self,
        client: &Client,
        name: &'static str,
        allow_null: bool,
        table: &'static str,
    ) -> Result<Option<uuid::Uuid>, ViewError> {
        let Some(raw) = self.field_value(name, allow_null, Some(false)) else {
            return Ok(None);
        };
        let value = match fields::primary_key(raw) {
            Ok(value) => value,
            Err(detail) => {
                self.errors.push(name, detail);
                return Ok(None);
            }
        };
        // The table name is a generated const from model metadata, never input.
        let sql = format!("SELECT 1 FROM {table} WHERE id = $1");
        let exists = client
            .query_opt(sql.as_str(), &[&value])
            .await
            .map_err(ViewError::from)?
            .is_some();
        if exists {
            self.values.push((name, Value::Uuid(value)));
            return Ok(Some(value));
        }
        // `Invalid pk "{pk_value}" - object does not exist.` — `pk_value` is the
        // input as the body carried it.
        let raw = self.raw_text(name).unwrap_or_else(|| value.to_string());
        self.errors.push(
            name,
            Detail::new(
                format!("Invalid pk \"{raw}\" - object does not exist."),
                "does_not_exist",
            ),
        );
        Ok(None)
    }

    /// `IntegerField` (`rest_framework/fields.py:1080-1100`).
    pub fn integer_field(
        &mut self,
        name: &'static str,
        allow_null: bool,
        options: fields::IntegerOptions,
    ) -> Option<i64> {
        let parsed = self.field(name, allow_null, move |raw| fields::integer(raw, &options))?;
        Some(parsed)
    }

    /// `ChoiceField` over string choices (`rest_framework/fields.py:1930-1970`).
    pub fn choice_field(
        &mut self,
        name: &'static str,
        allow_null: bool,
        allow_blank: bool,
        allowed: &'static [&'static str],
    ) -> Option<String> {
        let parsed = self.field(name, allow_null, move |raw| {
            fields::choice(raw, allow_blank, allowed)
        })?;
        Some(parsed)
    }

    /// `DateField` (`rest_framework/fields.py:1200-1240`).
    pub fn date_field(
        &mut self,
        name: &'static str,
        allow_null: bool,
        formats: &'static [&'static str],
    ) -> Option<NaiveDate> {
        let parsed = self.field(name, allow_null, move |raw| {
            fields::date(fields::DateInput::Raw(raw), formats)
        })?;
        Some(parsed)
    }

    /// `ListField` of `PrimaryKeyRelatedField` children
    /// (`rest_framework/fields.py:1690-1740`): `not_a_list`/`empty`, then one
    /// `queryset.get(pk=…)` per element. Child failures are rendered under the
    /// element's index, as DRF's `errors[idx]` dict.
    pub async fn primary_key_list_field(
        &mut self,
        client: &Client,
        name: &'static str,
        options: PrimaryKeyListOptions,
    ) -> Result<Option<Vec<uuid::Uuid>>, ViewError> {
        let PrimaryKeyListOptions {
            allow_null,
            allow_empty,
            min_length,
            max_length,
            table,
        } = options;
        let Some(raw) = self.field_value(name, allow_null, Some(false)) else {
            return Ok(None);
        };
        let items = match raw {
            Raw::Json(Json::List(items)) => items.clone(),
            other => {
                let detail = fields::not_a_list(raw_type_name(other));
                self.errors.push(name, detail);
                return Ok(None);
            }
        };
        if items.is_empty() && !allow_empty {
            self.errors.push(name, fields::empty_list());
            return Ok(None);
        }
        let sql = format!("SELECT 1 FROM {table} WHERE id = $1");
        let mut values = Vec::with_capacity(items.len());
        let mut failed = false;
        for (index, item) in items.iter().enumerate() {
            // The child is `PrimaryKeyRelatedField(required=…)` with
            // `allow_null=False` by default: DRF's `validate_empty_values`
            // rejects an explicit `null` before the pk coercion.
            if item.is_null() {
                self.errors.push_indexed(name, index, Detail::null());
                failed = true;
                continue;
            }
            let raw = Raw::Json(item);
            let value = match fields::primary_key(raw) {
                Ok(value) => value,
                Err(detail) => {
                    self.errors.push_indexed(name, index, detail);
                    failed = true;
                    continue;
                }
            };
            let exists = client
                .query_opt(sql.as_str(), &[&value])
                .await
                .map_err(ViewError::from)?
                .is_some();
            if exists {
                values.push(value);
            } else {
                self.errors.push_indexed(
                    name,
                    index,
                    Detail::new(
                        format!(
                            "Invalid pk \"{}\" - object does not exist.",
                            fields::primary_key_text(raw)
                        ),
                        "does_not_exist",
                    ),
                );
                failed = true;
            }
        }
        if failed {
            return Ok(None);
        }
        if let Some(min) = min_length
            && values.len() < min
        {
            self.errors.push(name, fields::list_min_length(min));
            return Ok(None);
        }
        if let Some(max) = max_length
            && values.len() > max
        {
            self.errors.push(name, fields::list_max_length(max));
            return Ok(None);
        }
        self.values.push((name, Value::UuidList(values.clone())));
        Ok(Some(values))
    }

    /// `ManyRelatedField` (`rest_framework/relations.py:480-560`): the child's
    /// error propagates directly (no per-index dict), and validation stops at
    /// the first failure.
    pub async fn primary_key_many_field(
        &mut self,
        client: &Client,
        name: &'static str,
        allow_null: bool,
        allow_empty: bool,
        table: &'static str,
    ) -> Result<Option<Vec<uuid::Uuid>>, ViewError> {
        let Some(raw) = self.field_value(name, allow_null, Some(false)) else {
            return Ok(None);
        };
        let items = match raw {
            Raw::Json(Json::List(items)) => items.clone(),
            other => {
                let detail = fields::not_a_list(raw_type_name(other));
                self.errors.push(name, detail);
                return Ok(None);
            }
        };
        if items.is_empty() && !allow_empty {
            self.errors.push(name, fields::empty_list());
            return Ok(None);
        }
        let sql = format!("SELECT 1 FROM {table} WHERE id = $1");
        let mut values = Vec::with_capacity(items.len());
        for item in &items {
            // `ManyRelatedField` delegates to the child's
            // `validate_empty_values`, so `null` is the child's null error and
            // propagates without the per-index wrapper.
            if item.is_null() {
                self.errors.push(name, Detail::null());
                return Ok(None);
            }
            let raw = Raw::Json(item);
            let value = match fields::primary_key(raw) {
                Ok(value) => value,
                Err(detail) => {
                    self.errors.push(name, detail);
                    return Ok(None);
                }
            };
            let exists = client
                .query_opt(sql.as_str(), &[&value])
                .await
                .map_err(ViewError::from)?
                .is_some();
            if !exists {
                self.errors.push(
                    name,
                    Detail::new(
                        format!(
                            "Invalid pk \"{}\" - object does not exist.",
                            fields::primary_key_text(raw)
                        ),
                        "does_not_exist",
                    ),
                );
                return Ok(None);
            }
            values.push(value);
        }
        self.values.push((name, Value::UuidList(values.clone())));
        Ok(Some(values))
    }

    /// Record a field error whose body is a JSON document — a
    /// `validate_<field>` that raised `ValidationError(dict)`.
    pub fn push_body(&mut self, field: &'static str, body: plane_query::json::Json) {
        self.errors.push_body(field, body);
    }

    /// A field whose parser decides the message and whose value the caller
    /// keeps; the validated bag records it for `partial`-style presence tests.
    pub fn field<T>(
        &mut self,
        name: &'static str,
        allow_null: bool,
        parse: impl FnOnce(fields::Raw<'_>) -> Result<T, Detail>,
    ) -> Option<T>
    where
        Value: From<T>,
        T: Clone,
    {
        let raw = self.field_value(name, allow_null, Some(false))?;
        match parse(raw) {
            Ok(value) => {
                self.values.push((name, Value::from(value.clone())));
                Some(value)
            }
            Err(detail) => {
                self.errors.push(name, detail);
                None
            }
        }
    }

    /// `DateTimeField` with DRF's iso-8601 input formats.
    pub fn datetime_field(
        &mut self,
        name: &'static str,
        allow_null: bool,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        let timezone = self.timezone;
        let raw = self.field_value(name, allow_null, Some(false))?;
        match fields::datetime(raw, timezone) {
            Ok(value) => {
                self.values.push((name, Value::DateTime(value)));
                Some(value)
            }
            Err(detail) => {
                self.errors.push(name, detail);
                None
            }
        }
    }

    /// `JSONField`.
    pub fn json_field(&mut self, name: &'static str, allow_null: bool) -> Option<JsonText> {
        // `JSONField.to_internal_value` runs `json.dumps(data)` for a
        // non-binary field (`rest_framework/fields.py:1830-1848`); a multipart
        // file object, or the `[<file>]` a `{**request.data}` mapping holds,
        // is not serialisable and fails before the value is stored.
        if !self.fatal && !self.skipped.contains(&name) && self.file_field(name) {
            self.errors.push(name, fields::invalid_json());
            return None;
        }
        // `JSONField.get_value` overrides the html branch (`fields.py:
        // 1836-1846`): a form value is always marked as a JSON string and
        // parsed, even when it is empty.
        let raw = self.field_value(name, allow_null, None)?;
        match fields::json(raw) {
            Ok(value) => {
                self.values.push((name, Value::Json(value.clone())));
                Some(value)
            }
            Err(detail) => {
                self.errors.push(name, detail);
                None
            }
        }
    }

    /// `Field.validate_empty_values` (`fields.py:494-530`) followed by the
    /// html-input empty-string rules (`fields.py:394-418`).
    ///
    /// `None` means "no value to write" — absent, skipped by `partial=True`,
    /// explicitly `null` (recorded as [`Value::Null`]), or invalid (the error
    /// is already recorded).
    fn field_value(
        &mut self,
        name: &'static str,
        allow_null: bool,
        allow_blank: Option<bool>,
    ) -> Option<Raw<'_>> {
        if self.fatal || self.skipped.contains(&name) {
            return None;
        }
        let Some(raw) = self.body.as_ref().and_then(|body| body.raw(name)) else {
            // Absent. DRF raises `SkipField` for `partial=True` and for a
            // non-required field without a default.
            return None;
        };
        if let Raw::Json(Json::Null) = raw {
            if allow_null {
                self.values.push((name, Value::Null));
            } else {
                self.errors.push(name, Detail::null());
            }
            return None;
        }
        if let Raw::Text("") = raw
            && self.form
            && let Some(allow_blank) = allow_blank
        {
            // Generic `get_value` for html input (`fields.py:394-418`):
            // `''` and `allow_null` → `''` when the field allows blank, else
            // `None`; `''` and not required → `''` when it allows blank, else
            // empty (skipped). All fields are `required=False` in the
            // serializers this pipeline is used for; a required field would
            // fall through to its own validation.
            if allow_null {
                if allow_blank {
                    self.values.push((name, Value::Text(String::new())));
                } else {
                    self.values.push((name, Value::Null));
                }
            } else if allow_blank {
                self.values.push((name, Value::Text(String::new())));
            }
            return None;
        }
        Some(raw)
    }

    /// Finish validation: the validated values, or every error collected so
    /// far in field order.
    ///
    /// # Errors
    ///
    /// [`ValidationErrors`] when any field or the body failed.
    pub fn finish(self) -> Result<Validated, ValidationErrors> {
        if self.errors.is_empty() {
            Ok(Validated {
                values: self.values,
            })
        } else {
            Err(self.errors)
        }
    }

    /// Whether a field or the body has already failed. `validate()` and
    /// `validate_<field>` only run when `to_internal_value` succeeded, so a
    /// handler must not add their errors on top of a field error.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Record an error on a field (`validate_<field>` / `validate()`).
    pub fn push_error(&mut self, field: &'static str, detail: Detail) {
        self.errors.push(field, detail);
    }

    /// Discard a validated value and record an error in its place — a
    /// `PrimaryKeyRelatedField` whose `queryset.get(pk=…)` missed
    /// (`rest_framework/relations.py:250-290`).
    pub fn reject(&mut self, field: &'static str, detail: Detail) {
        self.values.retain(|(name, _)| *name != field);
        self.errors.push(field, detail);
    }

    /// [`Self::reject`] for one child of a `ListField`, keeping DRF's
    /// index-keyed error object (`fields.py:1720-1735`).
    pub fn reject_indexed(&mut self, field: &'static str, index: usize, detail: Detail) {
        self.values.retain(|(name, _)| *name != field);
        self.errors.push_indexed(field, index, detail);
    }

    /// One validated value, before `finish()` consumes the serializer. Used by
    /// a handler that must re-check a field with its manager's predicate after
    /// the generated table-only existence check.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&Value> {
        self.values
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value)
    }

    /// Whether the request body carried a multipart file part under `name`.
    fn file_field(&self, name: &str) -> bool {
        self.body.as_ref().is_some_and(|body| body.has_file(name))
    }

    /// The raw text of a field's value, for an error message that quotes the
    /// input (`PrimaryKeyRelatedField`'s `Invalid pk "{pk_value}"…`).
    #[must_use]
    pub fn raw_text(&self, name: &str) -> Option<String> {
        match self.body.as_ref()?.raw(name)? {
            Raw::Text(text) => Some(text.to_owned()),
            Raw::Json(Json::Str(value)) => value.as_str().map(str::to_owned),
            Raw::Json(Json::Int(value)) => Some(value.to_string()),
            Raw::Json(Json::Bool(value)) => Some(if *value { "True" } else { "False" }.to_owned()),
            Raw::Json(Json::Float(value)) => Some(plane_query::pyfloat::repr(*value)),
            Raw::Json(_) => None,
        }
    }

    /// `Field.validate_empty_values`'s `required` branch: the field is absent
    /// from a non-`partial` body (`rest_framework/fields.py:494-530`).
    ///
    /// A `char_field` call cannot distinguish "absent" from "invalid"; this
    /// tells the caller which one to report for a required field.
    #[must_use]
    pub fn missing_required(&self, name: &'static str) -> bool {
        !self.partial
            && !self.fatal
            && !self.skipped.contains(&name)
            && self
                .body
                .as_ref()
                .is_some_and(|body| body.raw(name).is_none())
    }
}

/// Python's `type(value).__name__` for a `ListField`'s `not_a_list` message.
fn raw_type_name(raw: Raw<'_>) -> &'static str {
    match raw {
        Raw::Text(_) => "str",
        Raw::Json(Json::Bool(_)) => "bool",
        Raw::Json(Json::Int(_)) => "int",
        Raw::Json(Json::Float(_)) => "float",
        Raw::Json(Json::Str(_)) => "str",
        Raw::Json(Json::Null) => "NoneType",
        Raw::Json(Json::List(_)) => "list",
        Raw::Json(Json::Dict(_)) => "dict",
    }
}

/// Python's `type(value).__name__` for the non-mapping body message.
fn python_type_name(value: &Json) -> &'static str {
    match value {
        Json::Null => "NoneType",
        Json::Bool(_) => "bool",
        Json::Int(_) => "int",
        Json::Float(_) => "float",
        Json::Str(_) => "str",
        Json::List(_) => "list",
        Json::Dict(_) => "dict",
    }
}

/// Re-export so a domain can build a `PyDict` for tests without importing
/// `plane_serde`.
pub fn empty_dict() -> PyDict {
    pyjson::PyDict::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_drf::view::ViewResponse;

    fn body(response: ViewResponse) -> serde_json::Value {
        match response {
            ViewResponse::Raw { body, .. } => serde_json::from_slice(&body).expect("json"),
            other => panic!("expected a raw response, got {other:?}"),
        }
    }

    #[test]
    fn list_child_errors_render_as_an_index_object() {
        let mut errors = ValidationErrors::default();
        errors.push_indexed(
            "label_ids",
            0,
            Detail::new(
                "Invalid pk \"x\" - object does not exist.",
                "does_not_exist",
            ),
        );
        errors.push_indexed(
            "label_ids",
            2,
            Detail::new("\"bad\" is not a valid UUID.", "invalid"),
        );
        assert_eq!(
            body(errors.into_response()),
            serde_json::json!({
                "label_ids": {
                    "0": ["Invalid pk \"x\" - object does not exist."],
                    "2": ["\"bad\" is not a valid UUID."]
                }
            })
        );
    }

    #[test]
    fn ordinary_field_errors_stay_a_list() {
        let mut errors = ValidationErrors::new("name", Detail::required());
        errors.push("color", Detail::blank());
        assert_eq!(
            body(errors.into_response()),
            serde_json::json!({
                "name": ["This field is required."],
                "color": ["This field may not be blank."]
            })
        );
    }
}
