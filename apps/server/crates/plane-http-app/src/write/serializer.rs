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
use crate::write::url::{self, UrlOptions};

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

/// What `ListField.get_value` handed over.
#[derive(Debug)]
enum ListInput {
    /// The field is absent from this body (or skipped by `partial=True`).
    Skip,
    /// An explicit `null`: DRF's `null` error, before any element is seen.
    Null,
    /// The child inputs, cloned out of the body.
    Items(Vec<Json>),
    /// A value `ListField.to_internal_value` calls `not_a_list` on, with
    /// Python's `type(value).__name__`.
    NotIterable(&'static str),
}

/// [`ListInput`] for one JSON value.
fn list_json_value(value: Option<&Json>) -> ListInput {
    match value {
        None => ListInput::Skip,
        Some(Json::Null) => ListInput::Null,
        Some(Json::List(items)) => ListInput::Items(items.clone()),
        Some(other) => ListInput::NotIterable(python_type_name(other)),
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
        let mut serializer = Self::empty(ctx, partial);
        match ctx.data()? {
            Data::Json(value) => serializer.json_value(value),
            Data::Form(query) => {
                serializer.body = Some(Body::Form(query));
                serializer.form = true;
            }
        }
        Ok(serializer)
    }

    /// Start validating one value the request *contains* rather than the body
    /// itself: `IssueCreateSerializer(data=request.data.get("issue"), …)`
    /// (`plane/app/views/intake/base.py:261-269`, `:414-418`).
    ///
    /// The shape rules are [`Self::new`]'s: a JSON object is the body, `null`
    /// (the `.get` default for a missing key) is DRF's "No data provided", and
    /// every other value is the expected-a-dictionary error
    /// (`rest_framework/serializers.py:589-596`). A form body never reaches
    /// here — the nested value of a `QueryDict` is a string, and the views'
    /// ordering makes that a 500 before the serializer.
    pub fn from_value(ctx: &ViewCtx<'_>, value: &'a Json, partial: bool) -> Self {
        Self::from_value_in(
            ctx.timezone.copied().unwrap_or(chrono_tz::UTC),
            value,
            partial,
        )
    }

    /// [`Self::from_value`] with the active timezone supplied directly, so the
    /// body-shape rules are testable without a request.
    fn from_value_in(timezone: chrono_tz::Tz, value: &'a Json, partial: bool) -> Self {
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
        serializer.json_value(value);
        serializer
    }

    /// The serializer state both constructors start from.
    fn empty(ctx: &ViewCtx<'_>, partial: bool) -> Self {
        Self {
            body: None,
            partial,
            form: false,
            errors: ValidationErrors::default(),
            values: Vec::new(),
            skipped: Vec::new(),
            fatal: false,
            timezone: ctx.timezone.copied().unwrap_or(chrono_tz::UTC),
        }
    }

    /// The shape branch a parsed JSON value takes: an object is the body,
    /// `None` is "No data provided", anything else is DRF's non-mapping error.
    fn json_value(&mut self, value: &'a Json) {
        match value {
            Json::Dict(object) => {
                self.body = Some(Body::Json(object));
            }
            Json::Null => {
                self.fatal = true;
                self.errors
                    .push("non_field_errors", Detail::new("No data provided", "null"));
            }
            other => {
                self.fatal = true;
                self.errors.push(
                    "non_field_errors",
                    Detail::new(
                        format!(
                            "Invalid data. Expected a dictionary, but got {}.",
                            python_type_name(other)
                        ),
                        "invalid",
                    ),
                );
            }
        }
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
        let mut serializer = Self::empty(ctx, partial);
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

    /// `serializers.URLField`, whose only instance in the app is
    /// `WebhookSerializer.url` (`plane/app/serializers/webhook.py:25`).
    ///
    /// `CharField.run_validation` (`rest_framework/fields.py:742-749`) returns
    /// early for an empty input — a blank field fails with `blank`, a field
    /// that allows blanks returns `''` **without running any validator** — and
    /// the declared chain ([`url::run_url_validators`]) runs otherwise.
    ///
    /// # Errors
    ///
    /// [`ViewError::Unknown`] when `urlparse` raises `ValueError` inside a
    /// declared validator: DRF does not catch it, so the view 500s.
    pub fn url_field(
        &mut self,
        name: &'static str,
        options: UrlOptions,
    ) -> Result<Option<String>, ViewError> {
        // `Field.get_value`'s html-input branch (`fields.py:394-418`). A form
        // body's empty value is `None` when the field is nullable, `empty`
        // (skipped) when it is optional, and `''` when it is required —
        // `Serializer::field_value` only carries the first two rules because
        // every other write serializer's fields are optional.
        if self.form
            && !self.fatal
            && !self.skipped.contains(&name)
            && self
                .body
                .as_ref()
                .and_then(|body| body.raw(name))
                .is_some_and(|raw| matches!(raw, Raw::Text("")))
        {
            if options.allow_null {
                if options.allow_blank {
                    self.values.push((name, Value::Text(String::new())));
                } else {
                    self.values.push((name, Value::Null));
                }
                return Ok(None);
            }
            if !options.required {
                return Ok(None);
            }
            if !options.allow_blank {
                self.errors.push(name, Detail::blank());
                return Ok(None);
            }
            self.values.push((name, Value::Text(String::new())));
            return Ok(Some(String::new()));
        }
        let Some(raw) = self.field_value(name, options.allow_null, Some(options.allow_blank))
        else {
            return Ok(None);
        };
        // `CharField.run_validation`'s early return: only strings can be blank
        // (a number's `str()` is never empty), and an exact `''` is blank even
        // without trimming.
        let blank = match raw {
            Raw::Text(text) => {
                text.is_empty() || (options.trim_whitespace && text.trim().is_empty())
            }
            Raw::Json(Json::Str(text)) => text.as_str().is_some_and(|text| {
                text.is_empty() || (options.trim_whitespace && text.trim().is_empty())
            }),
            Raw::Json(_) => false,
        };
        if blank {
            if !options.allow_blank {
                self.errors.push(name, Detail::blank());
                return Ok(None);
            }
            self.values.push((name, Value::Text(String::new())));
            return Ok(Some(String::new()));
        }
        let value = match fields::char(
            raw,
            &CharOptions {
                allow_null: options.allow_null,
                allow_blank: options.allow_blank,
                trim_whitespace: options.trim_whitespace,
                // The length validator is part of the chain below, after the
                // declared validators.
                max_length: None,
            },
        ) {
            Ok(value) => value,
            Err(detail) => {
                // `URLField` overrides `default_error_messages['invalid']`, so
                // `CharField.to_internal_value`'s `fail('invalid')` for a
                // bool/list/dict input renders `Enter a valid URL.`, not
                // `Not a valid string.` (`rest_framework/fields.py:794-797`).
                let detail = if detail.code == "invalid" && detail.message == "Not a valid string."
                {
                    Detail::new("Enter a valid URL.", "invalid")
                } else {
                    detail
                };
                self.errors.push(name, detail);
                return Ok(None);
            }
        };
        let errors = url::run_url_validators(&value, options.validators, options.max_length)
            .map_err(ViewError::Unknown)?;
        if !errors.is_empty() {
            for detail in errors {
                self.errors.push(name, detail);
            }
            return Ok(None);
        }
        self.values.push((name, Value::Text(value.clone())));
        Ok(Some(value))
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

    /// `ListField` of `UUIDField` children (`rest_framework/fields.py:1690-1760`).
    ///
    /// `PageSerializer.label_ids`/`project_ids` are the only callers
    /// (`plane/app/serializers/page.py:33-34`). Two details separate it from
    /// [`Self::primary_key_list_field`]:
    ///
    /// * the child is DRF's `UUIDField` (`fields.py:817-844`), not a
    ///   `PrimaryKeyRelatedField`: an integer is accepted as a 128-bit UUID,
    ///   there is no `queryset.get(pk=…)`, and the message is
    ///   `Must be a valid UUID.` — not Django's `“…” is not a valid UUID.`;
    /// * the shape checks are `ListField`'s own: `not_a_list` for a
    ///   `str`/`Mapping`/non-iterable input, and a `null` child is the child's
    ///   `null` error under the element's index.
    ///
    /// Child failures are rendered as DRF's index-keyed object
    /// (`fields.py:1719-1736`), e.g.
    /// `{"label_ids": {"1": ["Must be a valid UUID."]}}`.
    pub fn uuid_list_field(&mut self, name: &'static str) -> Option<Vec<uuid::Uuid>> {
        let items = match self.list_field_value(name) {
            ListInput::Skip => return None,
            ListInput::NotIterable(kind) => {
                self.errors.push(name, fields::not_a_list(kind));
                return None;
            }
            ListInput::Null => {
                self.errors.push(name, Detail::null());
                return None;
            }
            ListInput::Items(items) => items,
        };
        let mut values = Vec::with_capacity(items.len());
        let mut failed = false;
        for (index, item) in items.iter().enumerate() {
            if item.is_null() {
                self.errors.push_indexed(name, index, Detail::null());
                failed = true;
                continue;
            }
            match fields::uuid_field(Raw::Json(item)) {
                Ok(value) => values.push(value),
                Err(detail) => {
                    self.errors.push_indexed(name, index, detail);
                    failed = true;
                }
            }
        }
        if failed {
            return None;
        }
        self.values.push((name, Value::UuidList(values.clone())));
        Some(values)
    }

    /// `ListField.get_value` (`rest_framework/fields.py:1676-1696`): a JSON
    /// body hands the raw value over, a form body is a list of strings.
    ///
    /// The values are cloned out of the body so the caller can record errors
    /// while holding no borrow of it (the same reason
    /// [`Self::primary_key_list_field`] clones its list).
    fn list_field_value(&self, name: &'static str) -> ListInput {
        if self.fatal || self.skipped.contains(&name) {
            return ListInput::Skip;
        }
        let Some(body) = self.body.as_ref() else {
            return ListInput::Skip;
        };
        match body {
            Body::Json(object) => list_json_value(object.get(name)),
            Body::Expanded { object, .. } => list_json_value(object.get(name)),
            Body::Form(query) => {
                let values = query.getlist(name);
                if !values.is_empty() {
                    return ListInput::Items(
                        values
                            .into_iter()
                            .map(|text| Json::Str(PyStr::new(text)))
                            .collect(),
                    );
                }
                // `parse_html_list(default=empty)` with no `name[i]` keys is
                // `empty`, i.e. the field is skipped.
                ListInput::Skip
            }
        }
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

    /// Render `uuid_list_field("label_ids")` over `body` and return either the
    /// validated ids or `serializer.errors`, exactly as a handler would see
    /// them.
    fn validate_uuid_list(
        input: Body<'_>,
        partial: bool,
    ) -> Result<Vec<uuid::Uuid>, serde_json::Value> {
        let mut serializer = Serializer {
            body: Some(input),
            partial,
            form: false,
            errors: ValidationErrors::default(),
            values: Vec::new(),
            skipped: Vec::new(),
            fatal: false,
            timezone: chrono_tz::UTC,
        };
        let _ = serializer.uuid_list_field("label_ids");
        match serializer.finish() {
            Ok(values) => Ok(values.uuid_list("label_ids").unwrap_or_default().to_vec()),
            Err(errors) => Err(body(errors.into_response())),
        }
    }

    fn json_body(text: &str) -> PyDict {
        match pyjson::loads(text.as_bytes(), "utf-8").expect("json") {
            Json::Dict(ref dict) => dict.clone(),
            _ => panic!("not an object"),
        }
    }

    /// Every message below is `ListField(child=UUIDField()).run_validation`
    /// output recorded from DRF 3.18.3 (see the doc comment on
    /// [`fields::uuid_field`]): `not_a_list` for a scalar/`dict`, the field's
    /// own `null` error, and the child's `Must be a valid UUID.` under the
    /// element index — *not* Django's `“…” is not a valid UUID.` that
    /// `PrimaryKeyRelatedField` produces.
    #[test]
    fn uuid_list_field_reproduces_drf_messages() {
        let cases: &[(&str, Result<&[&str], serde_json::Value>)] = &[
            (
                r#"{"label_ids": "abc"}"#,
                Err(serde_json::json!({
                    "label_ids": ["Expected a list of items but got type \"str\"."]
                })),
            ),
            (
                r#"{"label_ids": 5}"#,
                Err(serde_json::json!({
                    "label_ids": ["Expected a list of items but got type \"int\"."]
                })),
            ),
            (
                r#"{"label_ids": 1.5}"#,
                Err(serde_json::json!({
                    "label_ids": ["Expected a list of items but got type \"float\"."]
                })),
            ),
            (
                r#"{"label_ids": true}"#,
                Err(serde_json::json!({
                    "label_ids": ["Expected a list of items but got type \"bool\"."]
                })),
            ),
            (
                r#"{"label_ids": {"a": 1}}"#,
                Err(serde_json::json!({
                    "label_ids": ["Expected a list of items but got type \"dict\"."]
                })),
            ),
            (
                r#"{"label_ids": null}"#,
                Err(serde_json::json!({"label_ids": ["This field may not be null."]})),
            ),
            (
                r#"{"label_ids": ["nope"]}"#,
                Err(serde_json::json!({"label_ids": {"0": ["Must be a valid UUID."]}})),
            ),
            (
                r#"{"label_ids": [null]}"#,
                Err(serde_json::json!({
                    "label_ids": {"0": ["This field may not be null."]}
                })),
            ),
            (
                r#"{"label_ids": [1.5]}"#,
                Err(serde_json::json!({"label_ids": {"0": ["Must be a valid UUID."]}})),
            ),
            (
                r#"{"label_ids": [-1]}"#,
                Err(serde_json::json!({"label_ids": {"0": ["Must be a valid UUID."]}})),
            ),
            (
                r#"{"label_ids": [{"a": 1}]}"#,
                Err(serde_json::json!({"label_ids": {"0": ["Must be a valid UUID."]}})),
            ),
            (
                r#"{"label_ids": [[1]]}"#,
                Err(serde_json::json!({"label_ids": {"0": ["Must be a valid UUID."]}})),
            ),
            // Both failures of a mixed list, in element order.
            (
                r#"{"label_ids": ["0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f", "x", null, 7]}"#,
                Err(serde_json::json!({
                    "label_ids": {
                        "1": ["Must be a valid UUID."],
                        "2": ["This field may not be null."]
                    }
                })),
            ),
            // `uuid.UUID(int=…)` accepts an integer, `bool` included.
            (
                r#"{"label_ids": [1]}"#,
                Ok(&["00000000-0000-0000-0000-000000000001"]),
            ),
            (
                r#"{"label_ids": [true, false]}"#,
                Ok(&[
                    "00000000-0000-0000-0000-000000000001",
                    "00000000-0000-0000-0000-000000000000",
                ]),
            ),
            // The largest 128-bit integer.
            (
                r#"{"label_ids": [170141183460469231731687303715884105728]}"#,
                Ok(&["80000000-0000-0000-0000-000000000000"]),
            ),
            // Python's hex parser: simple, braced and urn forms.
            (
                r#"{"label_ids": ["0f0e7a3a2f5c4b1e9c3f1a2b3c4d5e6f"]}"#,
                Ok(&["0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f"]),
            ),
            (
                r#"{"label_ids": ["{0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f}"]}"#,
                Ok(&["0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f"]),
            ),
            (
                r#"{"label_ids": ["urn:uuid:0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f"]}"#,
                Ok(&["0f0e7a3a-2f5c-4b1e-9c3f-1a2b3c4d5e6f"]),
            ),
            (r#"{"label_ids": []}"#, Ok(&[])),
            // Absent: `required=False`, so the field is skipped.
            (r#"{"other": 1}"#, Ok(&[])),
        ];
        for (input, expected) in cases {
            let dict = json_body(input);
            let actual = validate_uuid_list(Body::Json(&dict), false);
            match expected {
                Ok(ids) => {
                    let ids: Vec<uuid::Uuid> = ids
                        .iter()
                        .map(|id| uuid::Uuid::parse_str(id).expect("uuid"))
                        .collect();
                    assert_eq!(actual, Ok(ids), "input {input}");
                }
                Err(expected) => assert_eq!(&actual, &Err(expected.clone()), "input {input}"),
            }
        }
    }

    /// A form body hands `getlist` over, so a single value is a one-element
    /// list and the child reports its own message.
    #[test]
    fn uuid_list_field_reads_a_form_value_as_a_list() {
        let query = plane_drf::QueryDict::parse("label_ids=nope", None).expect("query");
        let actual = validate_uuid_list(Body::Form(&query), false);
        assert_eq!(
            actual,
            Err(serde_json::json!({"label_ids": {"0": ["Must be a valid UUID."]}}))
        );
        // A form without the key skips the field.
        let query = plane_drf::QueryDict::parse("other=1", None).expect("query");
        assert_eq!(validate_uuid_list(Body::Form(&query), true), Ok(Vec::new()));
    }

    /// Validate `{"name": …}` through [`Serializer::from_value_in`] and return
    /// the trimmed value or the rendered `serializer.errors`.
    fn validate_nested_name(value: &Json) -> Result<Option<String>, serde_json::Value> {
        let mut serializer = Serializer::from_value_in(chrono_tz::UTC, value, false);
        let name = serializer.char_field(
            "name",
            CharOptions {
                allow_null: false,
                allow_blank: false,
                trim_whitespace: true,
                max_length: Some(255),
            },
        );
        match serializer.finish() {
            Ok(_) => Ok(name),
            Err(errors) => Err(body(errors.into_response())),
        }
    }

    /// `IssueCreateSerializer(data=request.data.get("issue"), …)`
    /// (`plane/app/views/intake/base.py:261-269`): the nested value goes
    /// through the same shape branches as a whole body. The messages are
    /// DRF 3.18.3's, recorded from `Serializer(data=…).errors` in
    /// `apps/api/.venv` (`serializers.py:589-596`).
    #[test]
    fn from_value_validates_a_nested_object_or_reports_the_body_shape() {
        let dict = json_body(r#"{"name": "  nested  "}"#);
        assert_eq!(
            validate_nested_name(&Json::Dict(dict)),
            Ok(Some("nested".to_owned()))
        );

        for (value, expected) in [
            (
                Json::Str(PyStr::from_string("x".to_owned())),
                serde_json::json!({
                    "non_field_errors": ["Invalid data. Expected a dictionary, but got str."]
                }),
            ),
            (
                Json::List(vec![Json::Str(PyStr::from_string("a".to_owned()))]),
                serde_json::json!({
                    "non_field_errors": ["Invalid data. Expected a dictionary, but got list."]
                }),
            ),
            (Json::List(Vec::new()), {
                serde_json::json!({
                    "non_field_errors": ["Invalid data. Expected a dictionary, but got list."]
                })
            }),
            // `request.data.get("issue")` on a body without the key is `None`.
            (
                Json::Null,
                serde_json::json!({"non_field_errors": ["No data provided"]}),
            ),
            (
                Json::Int(plane_serde::pyjson::PyInt::Small(5)),
                serde_json::json!({
                    "non_field_errors": ["Invalid data. Expected a dictionary, but got int."]
                }),
            ),
            (
                Json::Float(1.5),
                serde_json::json!({
                    "non_field_errors": ["Invalid data. Expected a dictionary, but got float."]
                }),
            ),
            (
                Json::Bool(true),
                serde_json::json!({
                    "non_field_errors": ["Invalid data. Expected a dictionary, but got bool."]
                }),
            ),
        ] {
            assert_eq!(
                validate_nested_name(&value),
                Err(expected),
                "value {value:?}"
            );
        }
    }

    /// Render `url_field("url")` over `body` and return either the validated
    /// string or `serializer.errors`, exactly as `WebhookSerializer`'s
    /// generated `validate` sees them.
    fn validate_url(input: Body<'_>) -> Result<String, serde_json::Value> {
        let mut serializer = Serializer {
            body: Some(input),
            partial: false,
            form: false,
            errors: ValidationErrors::default(),
            values: Vec::new(),
            skipped: Vec::new(),
            fatal: false,
            timezone: chrono_tz::UTC,
        };
        let _ = serializer
            .url_field("url", crate::write::UrlOptions::WEBHOOK_URL)
            .expect("no urlparse failure");
        match serializer.finish() {
            Ok(values) => Ok(values.text("url").unwrap_or_default().to_owned()),
            Err(errors) => Err(body(errors.into_response())),
        }
    }

    /// Recorded from DRF 3.18.3's `WebhookSerializer(data={"url": value})`
    /// (minimal settings, no database). The chain accumulates, and the two
    /// declared validators run before `Enter a valid URL.`.
    #[test]
    fn url_field_reproduces_drf_messages() {
        let cases: &[(&str, Result<&str, serde_json::Value>)] = &[
            (
                r#"{"url": "http://hooks.example.test/x"}"#,
                Ok("http://hooks.example.test/x"),
            ),
            // `validate_schema` lowercases the scheme, so this passes both
            // declared validators and Django's accepts the host.
            (r#"{"url": "HTTP://EXAMPLE.COM"}"#, Ok("HTTP://EXAMPLE.COM")),
            // Whitespace is trimmed before the validators run.
            (
                r#"{"url": "  https://hooks.example.test/x  "}"#,
                Ok("https://hooks.example.test/x"),
            ),
            (
                r#"{"url": "ftp://localhost"}"#,
                Err(serde_json::json!({
                    "url": [
                        "Invalid schema. Only HTTP and HTTPS are allowed.",
                        "Local URLs are not allowed."
                    ]
                })),
            ),
            (
                r#"{"url": "not a url"}"#,
                Err(serde_json::json!({
                    "url": [
                        "Invalid schema. Only HTTP and HTTPS are allowed.",
                        "Enter a valid URL."
                    ]
                })),
            ),
            (
                r#"{"url": "HTTP://x"}"#,
                Err(serde_json::json!({"url": ["Enter a valid URL."]})),
            ),
            (
                r#"{"url": "localhost:8000"}"#,
                Err(serde_json::json!({
                    "url": [
                        "Invalid schema. Only HTTP and HTTPS are allowed.",
                        "Enter a valid URL."
                    ]
                })),
            ),
            (
                r#"{"url": "LOCALHOST"}"#,
                Err(serde_json::json!({
                    "url": [
                        "Invalid schema. Only HTTP and HTTPS are allowed.",
                        "Enter a valid URL."
                    ]
                })),
            ),
            (
                r#"{"url": "user@localhost"}"#,
                Err(serde_json::json!({
                    "url": [
                        "Invalid schema. Only HTTP and HTTPS are allowed.",
                        "Enter a valid URL."
                    ]
                })),
            ),
            (
                r#"{"url": "http://localhost"}"#,
                Err(serde_json::json!({"url": ["Local URLs are not allowed."]})),
            ),
            // `validate_domain` compares the netloc, so a port defeats it.
            (
                r#"{"url": "http://localhost:8000/x"}"#,
                Ok("http://localhost:8000/x"),
            ),
            (
                r#"{"url": ""}"#,
                Err(serde_json::json!({"url": ["This field may not be blank."]})),
            ),
            (
                r#"{"url": "   "}"#,
                Err(serde_json::json!({"url": ["This field may not be blank."]})),
            ),
            (
                r#"{"url": null}"#,
                Err(serde_json::json!({"url": ["This field may not be null."]})),
            ),
            // `CharField` coerces a number with `str()` and the validators then
            // reject it; a bool or a composite is `fail('invalid')`, which a
            // `URLField` renders with its own message.
            (
                r#"{"url": 123}"#,
                Err(serde_json::json!({
                    "url": [
                        "Invalid schema. Only HTTP and HTTPS are allowed.",
                        "Enter a valid URL."
                    ]
                })),
            ),
            (
                r#"{"url": true}"#,
                Err(serde_json::json!({"url": ["Enter a valid URL."]})),
            ),
            (
                r#"{"url": ["http://example.com"]}"#,
                Err(serde_json::json!({"url": ["Enter a valid URL."]})),
            ),
            (
                r#"{"url": "http://example.com/\u0000"}"#,
                Err(serde_json::json!({"url": ["Null characters are not allowed."]})),
            ),
        ];
        for (raw, expected) in cases {
            let body = json_body(raw);
            let result = validate_url(Body::Json(&body));
            match expected {
                Ok(value) => assert_eq!(result, Ok((*value).to_owned()), "body {raw}"),
                Err(expected) => assert_eq!(result, Err(expected.clone()), "body {raw}"),
            }
        }
    }

    /// A form body's `url=` is `''` for this **required** field, so it is the
    /// blank error — not the "field is not set" branch an optional field takes
    /// (`rest_framework/fields.py:394-418`).
    #[test]
    fn url_field_form_input_rules() {
        let query = plane_drf::QueryDict::parse("url=", None).expect("query");
        let result = validate_url(Body::Form(&query));
        assert_eq!(
            result,
            Err(serde_json::json!({"url": ["This field may not be blank."]}))
        );

        // A form body without `url` is `empty`: `url_field` stores nothing and
        // the generated `missing_required` emits the required error.
        let empty = plane_drf::QueryDict::parse("", None).expect("query");
        let mut serializer = Serializer {
            body: Some(Body::Form(&empty)),
            partial: false,
            form: true,
            errors: ValidationErrors::default(),
            values: Vec::new(),
            skipped: Vec::new(),
            fatal: false,
            timezone: chrono_tz::UTC,
        };
        assert_eq!(
            serializer.url_field("url", crate::write::UrlOptions::WEBHOOK_URL),
            Ok(None)
        );
        assert!(serializer.missing_required("url"));
        assert!(!serializer.has_errors());
    }
}
