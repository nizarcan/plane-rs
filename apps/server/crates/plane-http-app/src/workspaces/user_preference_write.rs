// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `WorkspaceUserPreferenceViewSet.patch` (`plane/app/views/workspace/
//! user_preference.py:82-103`) — workspace-core set C, route 225.
//!
//! | Route | Method | Python | Handler |
//! | --- | --- | --- | --- |
//! | 225 `PATCH /api/workspaces/{slug}/sidebar-preferences/` | `PATCH` | `user_preference.py:82-103` | [`patch`] |
//!
//! ## The raw-body loop, and its 500s
//!
//! The view never builds a serializer: it iterates `request.data` and calls
//! `data.pop("key", None)` on every element (`user_preference.py:83-84`).
//!
//! * A JSON **object** iterates its **keys**, so `str.pop` is an
//!   `AttributeError` → the family 500. `{}` iterates nothing → 200.
//! * A JSON **list** iterates its elements: a non-object element is the same
//!   `AttributeError`, an empty list is 200.
//! * A JSON string iterates its **characters**: `""` is 200, anything else is
//!   the same `AttributeError`.
//! * A number, `true`, `false` or `null` is not iterable → `TypeError` → 500.
//! * A form body iterates its keys, so a non-empty form is the 500 and an
//!   empty one is 200.
//!
//! ## The writes
//!
//! For every element with a truthy `key`, one `.first()` lookup by
//! `(key, workspace__slug, user)` (`:85-89`) and then, when it hits,
//! `save(update_fields=["is_pinned", "sort_order"])` (`:101`). That is the only
//! place in this domain where the model's `save()` runs but H01's
//! `updated_at`/`updated_by` are **not** written, because Django's `_do_update`
//! keeps only the named columns. `is_pinned` absent from an element leaves the
//! stored value; the UPDATE still runs.
//!
//! The values are raw body values, coerced by the **model** fields on save
//! (`BooleanField.get_prep_value`, `FloatField.get_prep_value`), not by DRF:
//! an `is_pinned` outside Django's strict `True`/`False`/`"1"`/`"0"` set is a
//! Django `ValidationError` → 400 `{"error": "Please provide valid detail"}`,
//! `null` is a NOT NULL violation → 400 `{"error": "The payload is not
//! valid"}`, and a non-numeric `sort_order` is a `ValueError` → 500.
//! `key` goes through `CharField.get_prep_value`, i.e. Python's `str()`.

use http::Method;
use plane_core::error::APPLICATION_JSON;
use plane_db::models::workspace::WorkspaceUserPreference;
use plane_drf::error::ViewError;
use plane_drf::parsers::Data;
use plane_drf::registry::{Registry, RegistryError};
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_serde::pyjson::Value as Json;

use crate::common::client;
use crate::views::write_support::integrity;
use crate::workspaces::write_c_queries as queries;
use crate::write::repr::python_repr;

/// `plane.app.views.workspace.user_preference.WorkspaceUserPreferenceViewSet`.
const VIEW: &str = "plane.app.views.workspace.user_preference.WorkspaceUserPreferenceViewSet";

/// Register route 225's write verb.
///
/// `user_preference::register` already registers `get` for this view; the
/// registry accumulates methods per view.
pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.api_view(VIEW)?.method(Method::PATCH, patch)?;
    Ok(())
}

/// `WorkspaceUserPreferenceViewSet.patch` (`user_preference.py:82-103`).
fn patch<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let slug = ctx.kwarg("slug").ok_or(ViewError::Key)?;
        let user = ctx
            .user
            .id()
            .ok_or_else(|| ViewError::Unknown("an authenticated view without a user".into()))?;
        let client = client(ctx)?;

        // `for data in request.data:` (`:83`) — `request.data` is parsed here,
        // and the iteration shape decides which 500 the request gets.
        let data = ctx.data()?;
        let elements = elements(data)?;
        for element in elements {
            let Json::Dict(entry) = element else {
                // Only a list body can reach a non-dict element here: the
                // object/string/form paths fail on the first key below.
                return Err(pop_error(element));
            };
            // `key = data.pop("key", None)` — only `key` is read out of the
            // element afterwards, so no copy of the dict is needed.
            let Some(key) = entry.get("key") else {
                // A missing key is `None`, which is falsy.
                continue;
            };
            if !truthy(key) {
                continue;
            }
            // `CharField.get_prep_value` is Python's `str()` for a non-string.
            let key = python_str(key)?;

            // `filter(key=key, workspace__slug=slug, user=request.user).first()`
            // (`:85-89`).
            let statement = queries::USER_PREFERENCE_BY_KEY.prepare(client).await?;
            let Some(row) = client.query_opt(&statement, &[&key, &slug, &user]).await? else {
                continue;
            };
            let mut preference = WorkspaceUserPreference::from_row(&row, 0)?;

            if let Some(value) = entry.get("is_pinned") {
                preference.is_pinned = model_boolean(value)?;
            }
            if let Some(value) = entry.get("sort_order") {
                preference.sort_order = model_float(value)?;
            }

            // `save(update_fields=["is_pinned", "sort_order"])` (`:101`):
            // exactly those two columns, so no `updated_at`/`updated_by`.
            let statement = queries::USER_PREFERENCE_UPDATE.prepare(client).await?;
            client
                .execute(
                    &statement,
                    &[
                        &preference.is_pinned,
                        &preference.sort_order,
                        &preference.id,
                    ],
                )
                .await
                .map_err(integrity)?;
        }

        let body = plane_serde::to_vec(
            plane_serde::Dialect::Drf,
            &plane_core::json::Object::new().field("message", "Successfully updated".to_string()),
        )?;
        Ok(ViewResponse::raw(200, APPLICATION_JSON, body))
    })
}

/// The elements Python's `for data in request.data` yields.
///
/// The 500s are the iteration itself: a non-iterable body is a `TypeError`, a
/// dict/`QueryDict`/string iterates keys (strings), and only a list can carry
/// dicts.
fn elements(data: &Data) -> Result<Vec<&Json>, ViewError> {
    match data {
        Data::Form(query) => {
            // `request.data` merges `request.FILES` into the `QueryDict`, so a
            // multipart body with only a file part still iterates one key.
            if query.keys().next().is_none() && !query.has_files() {
                return Ok(Vec::new());
            }
            // Iterating a `QueryDict` yields its keys; `key.pop` on a `str`
            // fails on the first one.
            Err(ViewError::Unknown(
                "AttributeError: 'str' object has no attribute 'pop'".to_string(),
            ))
        }
        Data::Json(Json::Dict(entries)) => {
            if entries.is_empty() {
                return Ok(Vec::new());
            }
            Err(ViewError::Unknown(
                "AttributeError: 'str' object has no attribute 'pop'".to_string(),
            ))
        }
        Data::Json(Json::List(items)) => Ok(items.iter().collect()),
        // `for character in "":` runs zero times; a longer string fails on its
        // first character, a `str` having no `pop`.
        Data::Json(Json::Str(text)) => match text.as_str() {
            Some("") => Ok(Vec::new()),
            _ => Err(ViewError::Unknown(
                "AttributeError: 'str' object has no attribute 'pop'".to_string(),
            )),
        },
        Data::Json(other) => Err(ViewError::Unknown(format!(
            "TypeError: '{}' object is not iterable",
            python_type_name(other)
        ))),
    }
}

/// `type(value).__name__` for the `AttributeError`/`TypeError` above.
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

/// `'<type>' object has no attribute 'pop'` for a list element.
fn pop_error(value: &Json) -> ViewError {
    ViewError::Unknown(format!(
        "AttributeError: '{}' object has no attribute 'pop'",
        python_type_name(value)
    ))
}

/// Python truthiness (`if not key: continue`).
fn truthy(value: &Json) -> bool {
    match value {
        Json::Null => false,
        Json::Bool(flag) => *flag,
        Json::Int(number) => number.as_i64() != Some(0),
        Json::Float(number) => *number != 0.0,
        Json::Str(text) => text.as_str().is_some_and(|text| !text.is_empty()),
        Json::List(items) => !items.is_empty(),
        Json::Dict(entries) => !entries.is_empty(),
    }
}

/// Python's `str(value)` — `CharField.get_prep_value` for a non-string key.
fn python_str(value: &Json) -> Result<String, ViewError> {
    match value {
        Json::Str(text) => text
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| ViewError::Unknown("a lone surrogate in a key".to_string())),
        other => Ok(python_repr(other)),
    }
}

/// `BooleanField.get_prep_value` (`django/db/models/fields/__init__.py`):
/// `to_python`'s **strict** set, not DRF's.
///
/// A value outside it raises Django's `ValidationError`, which the family
/// handler turns into `400 {"error": "Please provide valid detail"}`;
/// `None` stays `None` and violates the column's NOT NULL constraint instead
/// (an `IntegrityError` → `400 {"error": "The payload is not valid"}`).
fn model_boolean(value: &Json) -> Result<bool, ViewError> {
    match value {
        Json::Null => Err(ViewError::Integrity),
        Json::Bool(flag) => Ok(*flag),
        Json::Int(number) => match number.as_i64() {
            Some(1) => Ok(true),
            Some(0) => Ok(false),
            _ => Err(ViewError::Validation),
        },
        Json::Float(number) => {
            if *number == 1.0 {
                Ok(true)
            } else if *number == 0.0 {
                Ok(false)
            } else {
                Err(ViewError::Validation)
            }
        }
        Json::Str(text) => match text.as_str() {
            Some("t" | "True" | "1") => Ok(true),
            Some("f" | "False" | "0") => Ok(false),
            _ => Err(ViewError::Validation),
        },
        Json::List(_) | Json::Dict(_) => Err(ViewError::Validation),
    }
}

/// `FloatField.get_prep_value` (`django/db/models/fields/__init__.py`):
/// `float(value)`, with `None` left for the NOT NULL violation.
fn model_float(value: &Json) -> Result<f64, ViewError> {
    match value {
        Json::Null => Err(ViewError::Integrity),
        Json::Bool(flag) => Ok(if *flag { 1.0 } else { 0.0 }),
        Json::Int(number) => number
            .as_i64()
            .map(|value| value as f64)
            .or_else(|| number.as_u64().map(|value| value as f64))
            .ok_or_else(|| {
                ViewError::Unknown("OverflowError: int too large to convert to float".to_string())
            }),
        Json::Float(number) => Ok(*number),
        Json::Str(text) => text.as_str().and_then(python_float).ok_or_else(|| {
            ViewError::Unknown("ValueError: could not convert string to float".to_string())
        }),
        other => Err(ViewError::Unknown(format!(
            "TypeError: float() argument must be a string or a real number, not '{}'",
            python_type_name(other)
        ))),
    }
}

/// CPython's `float(text)` (`PyFloat_FromString`): surrounding whitespace, a
/// sign, `inf`/`infinity`/`nan`, and an underscore only **between digits**.
fn python_float(text: &str) -> Option<f64> {
    let trimmed = text.trim_matches(|character: char| character.is_whitespace());
    let (negative, digits) = match trimmed.as_bytes().first()? {
        b'-' => (true, &trimmed[1..]),
        b'+' => (false, &trimmed[1..]),
        _ => (false, trimmed),
    };
    let folded = digits.to_ascii_lowercase();
    if matches!(folded.as_str(), "inf" | "infinity" | "nan") {
        let value = if folded == "nan" {
            f64::NAN
        } else {
            f64::INFINITY
        };
        return Some(if negative { -value } else { value });
    }
    if digits.contains('_') {
        let characters: Vec<char> = digits.chars().collect();
        let mut cleaned = String::with_capacity(digits.len());
        for (index, character) in characters.iter().enumerate() {
            if *character == '_' {
                let before = index.checked_sub(1).and_then(|i| characters.get(i));
                let after = characters.get(index + 1);
                if !before.is_some_and(char::is_ascii_digit)
                    || !after.is_some_and(char::is_ascii_digit)
                {
                    return None;
                }
                continue;
            }
            cleaned.push(*character);
        }
        return python_float(&format!("{}{}", if negative { "-" } else { "" }, cleaned));
    }
    trimmed.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Json {
        plane_serde::pyjson::loads(text.as_bytes(), "utf-8").expect("json")
    }

    #[test]
    fn a_dict_body_fails_on_its_first_key() {
        assert!(matches!(
            elements(&Data::Json(parse(r#"{"a": 1}"#))),
            Err(ViewError::Unknown(_))
        ));
        assert_eq!(elements(&Data::Json(parse("{}"))).expect("empty").len(), 0);
    }

    #[test]
    fn only_a_list_carries_dicts() {
        let data = Data::Json(parse(r#"[{"key": "views"}]"#));
        assert_eq!(elements(&data).expect("list").len(), 1);
        assert!(matches!(
            elements(&Data::Json(parse("3"))),
            Err(ViewError::Unknown(message)) if message.contains("not iterable")
        ));
        assert_eq!(
            elements(&Data::Json(parse(r#""""#))).expect("empty").len(),
            0
        );
    }

    #[test]
    fn the_model_boolean_is_stricter_than_drf() {
        assert_eq!(model_boolean(&parse("true")), Ok(true));
        assert_eq!(model_boolean(&parse("1")), Ok(true));
        assert_eq!(model_boolean(&parse(r#""1""#)), Ok(true));
        assert_eq!(model_boolean(&parse(r#""True""#)), Ok(true));
        assert_eq!(
            model_boolean(&parse(r#""true""#)),
            Err(ViewError::Validation)
        );
        assert_eq!(model_boolean(&parse("null")), Err(ViewError::Integrity));
        assert_eq!(model_boolean(&parse("2")), Err(ViewError::Validation));
    }

    #[test]
    fn python_floats_match_cpython() {
        assert_eq!(python_float("1_0"), Some(10.0));
        assert_eq!(python_float("1_"), None);
        assert_eq!(python_float("_1"), None);
        assert_eq!(python_float(" 1 "), Some(1.0));
        assert_eq!(python_float("inf"), Some(f64::INFINITY));
        assert!(python_float("nan").expect("nan").is_nan());
        assert_eq!(python_float("abc"), None);
        assert_eq!(model_float(&parse("null")), Err(ViewError::Integrity));
        assert!(matches!(
            model_float(&parse("[]")),
            Err(ViewError::Unknown(_))
        ));
    }
}
