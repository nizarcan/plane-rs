// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The request-body and error helpers shared by the `views` writes (set B).
//!
//! Neither route in this group uses a serializer — both read `request.data`
//! directly (`plane/app/views/view/base.py:460-479`,
//! `plane/app/views/workspace/member.py:224-231`) — so the body accessors here
//! reproduce `dict.get` / `QueryDict.get` rather than DRF's field validation.
//!
//! Two shapes matter and are easy to get wrong:
//!
//! * A JSON body that is not an object has no `.get` in Python: a list, string,
//!   number or `null` body is an `AttributeError` inside the view, i.e. the
//!   family 500. [`body_get`] returns [`ViewError::Unknown`] for it.
//! * A urlencoded/multipart body is a `QueryDict`, whose values are **strings**.
//!   `view_props` therefore stores the JSON *string* when a form carries it,
//!   exactly like `JSONField`'s `json.dumps(<str>)` does.

use plane_db::JsonText;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_serde::pyjson::{PyDict, PyStr, Value};
use uuid::Uuid;

/// `request.data.get(key)`, as the Python value the view would have received.
pub(super) enum Body<'a> {
    /// The key is absent (or the body is an empty object): `request.data.get`
    /// returns its default.
    Missing,
    /// A JSON member: the parsed value.
    Json(&'a Value),
    /// A form member: always a string (`QueryDict.get`).
    Form(&'a str),
}

/// `request.data.get(key)`; `request.data` must be a dict/`QueryDict`.
pub(super) fn body_get<'a>(data: &'a Data, key: &str) -> Result<Body<'a>, ViewError> {
    match data {
        Data::Form(query) => Ok(query.get(key).map_or(Body::Missing, Body::Form)),
        Data::Json(Value::Dict(members)) => Ok(members.get(key).map_or(Body::Missing, Body::Json)),
        // `list`/`str`/`int`/`float`/`bool`/`None` have no attribute `get`.
        Data::Json(other) => Err(ViewError::Unknown(format!(
            "AttributeError: '{}' object has no attribute 'get'",
            python_type_name(other)
        ))),
    }
}

/// The Python type name, for the `AttributeError` above.
fn python_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Int(_) => "int",
        Value::Float(_) => "float",
        Value::Str(_) => "str",
        Value::List(_) => "list",
        Value::Dict(_) => "dict",
    }
}

/// `IssueView.objects…get(pk=request.data.get("view"), …)`'s `pk` argument
/// through Django's `UUIDField.to_python` (`django/db/models/fields/__init__.py`).
///
/// * absent or `null` → `None`, which the lookup turns into `id IS NULL` and a
///   `DoesNotExist` (the caller answers 404);
/// * a string → `uuid.UUID(hex=…)`, whose failure is a `ValidationError` (400);
/// * an int (a `bool` included, as `bool` subclasses `int`) → `uuid.UUID(int=…)`;
///   a negative or `>= 2**128` value is a `ValidationError`;
/// * anything else (float, list, dict) → `AttributeError` inside `uuid.UUID`,
///   which `to_python` re-raises as a `ValidationError`.
pub(super) fn view_id(data: &Data) -> Result<Option<Uuid>, ViewError> {
    match body_get(data, "view")? {
        Body::Missing | Body::Json(Value::Null) => Ok(None),
        Body::Form(text) => uuid_from_hex(text).map(Some),
        Body::Json(Value::Str(text)) => text.as_str().map_or(Err(ViewError::Validation), |text| {
            uuid_from_hex(text).map(Some)
        }),
        Body::Json(Value::Int(int)) => {
            let text = int.to_string();
            // `uuid.UUID(int=…)` takes the decimal value; Python's ints are
            // unbounded, so compare as text-parsed `u128`.
            text.parse::<u128>()
                .map(|value| Some(Uuid::from_u128(value)))
                .map_err(|_| ViewError::Validation)
        }
        Body::Json(Value::Bool(flag)) => Ok(Some(Uuid::from_u128(u128::from(*flag)))),
        Body::Json(_) => Err(ViewError::Validation),
    }
}

/// `uuid.UUID(hex=…)`: hyphenated, 32 hex digits, `urn:uuid:` or braced form.
fn uuid_from_hex(text: &str) -> Result<Uuid, ViewError> {
    Uuid::parse_str(text).map_err(|_| ViewError::Validation)
}

/// `request.data.get(key, {})` for a JSON column, as
/// `WorkspaceMemberUserViewsEndpoint.post` reads `view_props`.
///
/// `None` is SQL `NULL` (a `NOT NULL` column refuses it → the family 400); the
/// absent key is `{}`, **not** the stored value.
pub(super) fn json_or_empty_object(data: &Data, key: &str) -> Result<Option<JsonText>, ViewError> {
    match body_get(data, key)? {
        Body::Missing => Ok(Some(JsonText::from_json(&Value::Dict(PyDict::new()))?)),
        Body::Form(text) => Ok(Some(JsonText::from_json(&Value::Str(PyStr::new(text)))?)),
        Body::Json(Value::Null) => Ok(None),
        Body::Json(value) => Ok(Some(JsonText::from_json(value)?)),
    }
}

/// `django.db.IntegrityError` → the family's
/// `400 {"error": "The payload is not valid"}` (`plane/app/views/base.py:210-214`).
///
/// Every SQLSTATE class 23 error is an `IntegrityError` in Django — unique
/// violations (the duplicate-favourite index) and NOT NULL violations (a `null`
/// `view_props`) alike.
pub(super) fn integrity(error: tokio_postgres::Error) -> ViewError {
    if error
        .code()
        .is_some_and(|code| code.code().starts_with("23"))
    {
        ViewError::Integrity
    } else {
        ViewError::from(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::pyjson;

    fn json(text: &str) -> Data {
        Data::Json(pyjson::loads(text.as_bytes(), "utf-8").expect("json"))
    }

    #[test]
    fn a_non_object_body_has_no_get() {
        for body in ["[1]", "\"x\"", "3", "null"] {
            assert!(matches!(
                body_get(&json(body), "view"),
                Err(ViewError::Unknown(_))
            ));
        }
    }

    #[test]
    fn view_ids_follow_python_uuid_coercion() {
        let canonical = "062b3259-338c-48e4-923c-32bdf03fb91a";
        assert_eq!(
            view_id(&json(&format!("{{\"view\": \"{canonical}\"}}"))).expect("uuid"),
            Some(Uuid::parse_str(canonical).expect("uuid"))
        );
        // An int is `uuid.UUID(int=…)`.
        assert_eq!(
            view_id(&json("{\"view\": 1}")).expect("int"),
            Some(Uuid::from_u128(1))
        );
        // A negative or too-large int is Django's ValidationError.
        assert_eq!(
            view_id(&json("{\"view\": -1}")).expect_err("negative"),
            ViewError::Validation
        );
        assert_eq!(
            view_id(&json("{\"view\": 1e5}")).expect_err("float"),
            ViewError::Validation
        );
        // `null` and an absent key are `pk=None` → 404, not a 400.
        assert_eq!(view_id(&json("{\"view\": null}")).expect("null"), None);
        assert_eq!(view_id(&json("{}")).expect("missing"), None);
    }

    #[test]
    fn a_form_value_is_a_json_string() {
        let form = Data::Form(
            plane_drf::request::QueryDict::parse("view_props=hello", None).expect("form"),
        );
        assert_eq!(
            json_or_empty_object(&form, "view_props")
                .expect("json")
                .expect("text")
                .as_str(),
            "\"hello\""
        );
        // The absent key is `{}`, the Python default.
        assert_eq!(
            json_or_empty_object(&form, "other")
                .expect("json")
                .expect("text")
                .as_str(),
            "{}"
        );
    }
}
