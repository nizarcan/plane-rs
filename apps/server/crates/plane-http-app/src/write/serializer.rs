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

use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::view::ViewCtx;
use plane_serde::pyjson::{self, PyDict, Value as Json};

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

/// `serializer.errors`: ordered `(field, details)` pairs.
///
/// `Detail::code` is kept because DRF's "No data provided" rewrite depends on
/// the only error being a `null` code (`serializers.py:589-596`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ValidationErrors(Vec<(&'static str, Vec<Detail>)>);

impl ValidationErrors {
    pub fn new(field: &'static str, detail: Detail) -> Self {
        Self(vec![(field, vec![detail])])
    }

    pub fn push(&mut self, field: &'static str, detail: Detail) {
        match self.0.iter_mut().find(|(name, _)| *name == field) {
            Some((_, details)) => details.push(detail),
            None => self.0.push((field, vec![detail])),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The 400 `Response(serializer.errors)` body.
    ///
    /// Keys are in field order, each value is a JSON array of strings:
    /// `{"color": ["Ensure this field has no more than 255 characters."]}`.
    #[must_use]
    pub fn into_response(self) -> plane_drf::view::ViewResponse {
        use plane_query::json::Json as Doc;
        let entries: Vec<(String, Doc)> = self
            .0
            .into_iter()
            .map(|(field, details)| {
                (
                    field.to_owned(),
                    Doc::Array(
                        details
                            .into_iter()
                            .map(|detail| Doc::Str(detail.message))
                            .collect(),
                    ),
                )
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
#[derive(Debug, Clone, Copy)]
enum Body<'a> {
    /// A JSON object.
    Json(&'a PyDict),
    /// A form or multipart body: every value is a string
    /// (`html.is_html_input(dictionary)` is true for a `QueryDict`).
    Form(&'a plane_drf::QueryDict),
}

impl<'a> Body<'a> {
    /// `Field.get_value` (`rest_framework/fields.py:394-418`) for the html
    /// branch: an empty string means "blank" — `None` when the field allows
    /// null, "absent" when it is not required and does not allow blank.
    fn raw(&self, name: &str) -> Option<Raw<'a>> {
        match self {
            Self::Form(query) => query.get(name).map(Raw::Text),
            Self::Json(object) => object.get(name).map(Raw::Json),
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
    ) -> Option<Raw<'a>> {
        if self.fatal {
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
            && self
                .body
                .as_ref()
                .is_some_and(|body| body.raw(name).is_none())
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
