// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! DRF's `ListSerializer` — the `many=True` error contract.
//!
//! Python: `BulkEstimatePointEndpoint.create`'s
//! `EstimatePointSerializer(data=request.data.get("estimate_points"),
//! many=True)` (`plane/app/views/estimate/base.py:76-80`). `many=True` builds
//! a `ListSerializer` around the child serializer (`many_init`,
//! `rest_framework/serializers.py:135-178`), and the whole error body comes
//! from it:
//!
//! | Input | Body | Python |
//! | --- | --- | --- |
//! | key absent, or `null` | `{"non_field_errors": ["No data provided"]}` | `fields.py:487-517`, `serializers.py:821-831` |
//! | not a list | `{"non_field_errors": ["Expected a list of items but got type \"dict\"."]}` | `serializers.py:669-688` |
//! | a list with failures | `{"1": {"value": ["…"]}}` — only the failing indices | `serializers.py:702-726` |
//!
//! `data=None` never reaches `to_internal_value`: `validate_empty_values`
//! (`fields.py:487-517`) sees `data is None` and, because a `ListSerializer`'s
//! `allow_null` is `False`, raises the field's bare `null` message. The
//! `errors` property rewrites a lone null error to "No data provided"
//! (`serializers.py:821-831`) — that is the whole reason the message is not
//! "This field may not be null.".
//!
//! The index object is `LIST_SERIALIZER_ERRORS_AS_DICT=True`, DRF 3.18.3's
//! shipped default (`rest_framework/settings.py:89`) and not overridden by
//! Plane: `to_internal_value` keeps `errors[index]` for the elements that
//! failed and drops the successful ones (`serializers.py:702-724`). Each value
//! is the child's `ValidationError.detail`, unchanged — usually a field
//! object, a bare list for the child's own `null` error, and
//! `{"non_field_errors": […]}` for a `validate()` failure (`as_serializer_error`,
//! `serializers.py:314-345`).
//!
//! Not reproduced because no call site passes them: `allow_empty=False`,
//! `max_length` and `min_length` (`many_init`'s `LIST_SERIALIZER_KWARGS`). A
//! handler that needs one adds the branch next to the two above.
//!
//! The call site only ever reaches this with a JSON object body: `create`
//! dereferences `request.data.get("estimate")` with `.get(...)` before the
//! serializer (`base.py:65-66`), so a form body (a `QueryDict` whose `get`
//! returns a string) or a non-object JSON body is an `AttributeError` 500
//! before `EstimatePointSerializer` exists.
//!
//! The shape a handler uses:
//!
//! ```ignore
//! let points = ctx.data()?.json().and_then(|body| body.get("estimate_points"));
//! let validated = match write::list_of(points, |_index, item| async move {
//!     let mut serializer = Serializer::from_value(ctx, item, false);
//!     if let Err(error) = generated::estimate_point_serializer::validate(&mut serializer, client).await {
//!         // A DB failure is Django's 500, not a field error.
//!         return Err(ChildError::fatal(error));
//!     }
//!     match serializer.finish() {
//!         Ok(values) => match estimate_point_validate(&values) {
//!             Ok(()) => Ok(values),
//!             Err(message) => Err(ChildError::non_field(message)),
//!         },
//!         Err(errors) => Err(ChildError::from_errors(errors)),
//!     }
//! }).await
//! {
//!     Ok(values) => values,
//!     Err(ListError::Validation(response)) => return Ok(response),
//!     Err(ListError::Fatal(error)) => return Err(error),
//! };
//! ```

use std::future::Future;

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::ViewResponse;
use plane_query::json::Json;
use plane_serde::pyjson;

use crate::write::fields::Detail;
use crate::write::serializer::{Validated, ValidationErrors};

/// One child serializer's failure, as `ListSerializer.to_internal_value`
/// treats it.
///
/// A `ValidationError` becomes the element's index entry; anything else is not
/// caught by DRF (`except ValidationError`, `serializers.py:705-711`) and
/// leaves the list serializer, so it must not turn into a 400.
///
/// The validation detail is kept in its rendered form (rather than a parsed
/// value) so nesting it is byte-identical to `JSONRenderer`'s output for the
/// child: object key order (the serializer's field order) and number
/// formatting survive without a round trip.
#[derive(Debug, Clone, PartialEq)]
pub struct ChildError {
    outcome: ChildOutcome,
}

#[derive(Debug, Clone, PartialEq)]
enum ChildOutcome {
    /// `ValidationError.detail`, rendered as the JSON document DRF nests
    /// under the element's index.
    Detail(String),
    /// A failure DRF does not catch (a database error inside a related-field
    /// lookup, say). It propagates unchanged, as Python's would.
    Fatal(ViewError),
}

impl ChildError {
    /// A detail document already rendered by DRF's `JSONRenderer`.
    fn rendered(detail: String) -> Self {
        Self {
            outcome: ChildOutcome::Detail(detail),
        }
    }

    /// A detail document supplied by the caller.
    #[must_use]
    pub fn detail(document: Json) -> Self {
        Self::rendered(String::from_utf8_lossy(&document.to_drf()).into_owned())
    }

    /// `raise ValidationError("…")` from the child's `validate()`, which
    /// `as_serializer_error` wraps under `non_field_errors`
    /// (`rest_framework/serializers.py:314-345`).
    #[must_use]
    pub fn non_field(message: impl Into<String>) -> Self {
        Self::detail(Json::object(vec![(
            "non_field_errors",
            Json::Array(vec![Json::string(message)]),
        )]))
    }

    /// The child's `serializer.errors` after `finish()`, i.e. its
    /// `to_internal_value` failures in field order.
    #[must_use]
    pub fn from_errors(errors: ValidationErrors) -> Self {
        match errors.into_response() {
            ViewResponse::Raw { body, .. } => {
                Self::rendered(String::from_utf8_lossy(&body).into_owned())
            }
            // `into_response()` always builds a raw JSON response; keep the
            // function total without panicking on a request path.
            other => Self::non_field(format!("unexpected child error response: {other:?}")),
        }
    }

    /// A failure the child raised that is not a validation error — the
    /// generated `validate`'s `ViewError` in practice. `list_of` returns it as
    /// [`ListError::Fatal`] instead of nesting it.
    #[must_use]
    pub fn fatal(error: ViewError) -> Self {
        Self {
            outcome: ChildOutcome::Fatal(error),
        }
    }
}

/// `Field.fail('null')`: DRF coerces the message to a one-element list before
/// the parent sees it, so a `null` element's index maps to a **list**, not an
/// object (`rest_framework/exceptions.py:148-161`).
fn null_detail() -> String {
    String::from_utf8_lossy(&Json::Array(vec![Json::string(Detail::null().message)]).to_drf())
        .into_owned()
}

/// Why [`list_of`] failed.
#[derive(Debug)]
pub enum ListError {
    /// `Response(serializer.errors)`: the 400 body, index-keyed.
    Validation(ViewResponse),
    /// A child raised something DRF does not catch; answer it as the handler
    /// would answer any other `ViewError`.
    Fatal(ViewError),
}

/// `EstimatePointSerializer(data=…, many=True).is_valid()` and `.errors`.
///
/// `data` is the call site's `request.data.get(key)`: `None` when the key is
/// absent. A `null` value is the same thing to Python (`dict.get` returns the
/// stored `None`), so both are DRF's "No data provided".
///
/// `validate_child` runs the child serializer for one element: its field pass
/// first, then the hand-written `validate()` **only if no field failed** (a
/// failed field makes `to_internal_value` raise, so DRF never calls
/// `validate()` — `serializers.py:493-529`). A `null` element is the child's
/// `null` error and is not passed to the callback.
///
/// The returned values are DRF's `validated_data`, which is **not** what
/// `BulkEstimatePointEndpoint.create` writes: it re-reads the raw body list
/// (`base.py:82-98`), so a `CharField`'s trimming and an `IntegerField`'s
/// coercion never reach the row (`value: " a "` validates as `"a"` and is
/// stored as `" a "`; `key: "3"` validates as `3` and is stored from the raw
/// value). The caller keeps its raw tree for the INSERT and uses this result
/// only as the 400 gate, as Python does.
///
/// # Errors
///
/// [`ListError::Validation`] with the 400 `Response(serializer.errors)` body,
/// or [`ListError::Fatal`] with a child failure DRF does not catch.
pub async fn list_of<'a, F, Fut>(
    data: Option<&'a pyjson::Value>,
    mut validate_child: F,
) -> Result<Vec<Validated>, ListError>
where
    F: FnMut(usize, &'a pyjson::Value) -> Fut + Send,
    Fut: Future<Output = Result<Validated, ChildError>> + Send,
{
    let Some(data) = data else {
        return Err(ListError::Validation(no_data_provided()));
    };
    if data.is_null() {
        return Err(ListError::Validation(no_data_provided()));
    }
    let pyjson::Value::List(items) = data else {
        return Err(ListError::Validation(not_a_list(python_type_name(data))));
    };

    let mut validated = Vec::with_capacity(items.len());
    let mut errors: Vec<(usize, String)> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if item.is_null() {
            errors.push((index, null_detail()));
            continue;
        }
        match validate_child(index, item).await {
            Ok(values) => validated.push(values),
            Err(ChildError {
                outcome: ChildOutcome::Detail(detail),
            }) => errors.push((index, detail)),
            Err(ChildError {
                outcome: ChildOutcome::Fatal(error),
            }) => return Err(ListError::Fatal(error)),
        }
    }
    if errors.is_empty() {
        Ok(validated)
    } else {
        Err(ListError::Validation(index_errors(&errors)))
    }
}

/// `{"non_field_errors": ["No data provided"]}` (`serializers.py:821-831`).
fn no_data_provided() -> ViewResponse {
    let body = Json::object(vec![(
        "non_field_errors",
        Json::Array(vec![Json::string("No data provided")]),
    )]);
    ViewResponse::raw(400, APPLICATION_JSON, body.to_drf())
}

/// `{"non_field_errors": ["Expected a list of items but got type \"str\"."]}`
/// (`serializers.py:606-608`, `:676-682`).
fn not_a_list(input_type: &str) -> ViewResponse {
    let body = Json::object(vec![(
        "non_field_errors",
        Json::Array(vec![Json::string(format!(
            "Expected a list of items but got type \"{input_type}\"."
        ))]),
    )]);
    ViewResponse::raw(400, APPLICATION_JSON, body.to_drf())
}

/// `{index: detail, …}` with the failing indices only, in list order
/// (`serializers.py:702-724`).
///
/// Assembled from the already-rendered details: the keys are integers and
/// every detail is a JSON document, so concatenation is exactly the object
/// `JSONRenderer` would write — no re-parse, no re-ordering.
fn index_errors(errors: &[(usize, String)]) -> ViewResponse {
    let mut body = String::from("{");
    for (position, (index, detail)) in errors.iter().enumerate() {
        if position > 0 {
            body.push(',');
        }
        body.push('"');
        body.push_str(&index.to_string());
        body.push_str("\":");
        body.push_str(detail);
    }
    body.push('}');
    ViewResponse::raw(400, APPLICATION_JSON, body.into_bytes())
}

/// Python's `type(value).__name__` for the `not_a_list` message.
fn python_type_name(value: &pyjson::Value) -> &'static str {
    match value {
        pyjson::Value::Null => "NoneType",
        pyjson::Value::Bool(_) => "bool",
        pyjson::Value::Int(_) => "int",
        pyjson::Value::Float(_) => "float",
        pyjson::Value::Str(_) => "str",
        pyjson::Value::List(_) => "list",
        pyjson::Value::Dict(_) => "dict",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from the real `EstimatePointSerializer` and DRF 3.18.3 by
    /// `apps/server/tools/inventory/estimate_list_corpus.py`; regenerate with
    /// `apps/api/.venv/bin/python apps/server/tools/inventory/estimate_list_corpus.py`.
    const CORPUS: &str = include_str!("../../tests/golden/estimate-list-errors.json");

    fn json(text: &str) -> pyjson::Value {
        pyjson::loads(text.as_bytes(), "utf-8").expect("corpus JSON")
    }

    /// A child validator that must never run: a test uses it for the two
    /// shapes DRF resolves before `to_internal_value`.
    fn unreachable_future() -> std::future::Ready<Result<Validated, ChildError>> {
        unreachable!("the list serializer must not validate a child")
    }

    fn body(response: ViewResponse) -> String {
        match response {
            ViewResponse::Raw { body, .. } => String::from_utf8(body).expect("utf-8"),
            other => panic!("expected a raw response, got {other:?}"),
        }
    }

    /// The whole corpus: the two shapes that never reach a child, and every
    /// list case composed from the recorded per-element details. The child
    /// callback is the only place a detail comes from, so the envelope (which
    /// indices appear, in which order, and how a detail is nested) is what
    /// this test measures.
    #[tokio::test]
    async fn the_corpus_matches_drf() {
        let corpus: serde_json::Value = serde_json::from_str(CORPUS).expect("corpus");
        let cases = corpus["cases"].as_array().expect("cases");
        assert!(!cases.is_empty(), "empty corpus");

        for case in cases {
            let name = case["name"].as_str().expect("name");
            let valid = case["valid"].as_bool().expect("valid");
            let expected = case["body"].as_str();
            let children: Vec<Option<String>> = case["children"]
                .as_array()
                .map(|children| {
                    children
                        .iter()
                        .map(|detail| detail.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();

            let input = case["input"].as_str().map(json);
            if input.is_some() {
                assert!(
                    case["absent"].is_null(),
                    "{name}: both `absent` and `input`"
                );
            }
            let result = list_of(input.as_ref(), |index, _item| {
                let detail = children[index].clone();
                async move {
                    match detail {
                        Some(detail) => Err(ChildError::rendered(detail)),
                        None => Ok(Validated::default()),
                    }
                }
            })
            .await;

            match (valid, result) {
                (true, Ok(values)) => {
                    assert_eq!(values.len(), children.len(), "{name}: count");
                }
                (false, Err(ListError::Validation(response))) => {
                    assert_eq!(
                        body(response),
                        expected.expect("body"),
                        "{name}: error body"
                    );
                }
                (true, Err(ListError::Validation(response))) => {
                    panic!("{name}: expected valid, got {}", body(response))
                }
                (_, Err(ListError::Fatal(error))) => {
                    panic!("{name}: unexpected fatal child error: {error:?}")
                }
                (false, Ok(_)) => panic!("{name}: expected invalid, got valid"),
            }
        }
    }

    /// The handler returns `BoxFuture`, which requires `Send`: prove the
    /// generic future is `Send` for a plain child callback so a handler can
    /// `Box::pin(list_of(…))` directly, and that the child halve of it — a
    /// `Serializer` held across the `generated::validate` await — is `Send`.
    #[tokio::test]
    async fn the_list_future_is_send() {
        fn assert_send<T: Send>(_: T) {}
        assert_send(Option::<crate::write::Serializer<'static>>::None);

        let input = json("[]");
        let future = list_of(Some(&input), |_index, _item| async move {
            Ok::<Validated, ChildError>(Validated::default())
        });
        let boxed: plane_drf::view::BoxFuture<'_, _> = Box::pin(future);
        assert_eq!(boxed.await.expect("valid").len(), 0);
    }

    /// `data=None` (the key is absent) and a non-list value never reach the
    /// child serializer; a child callback that panics proves it.
    #[tokio::test]
    async fn non_lists_do_not_validate_a_child() {
        let no_child = |_: usize, _: &pyjson::Value| unreachable_future();
        for (input, message) in [
            (None, "No data provided".to_owned()),
            (Some(json("null")), "No data provided".to_owned()),
            (
                Some(json("{}")),
                "Expected a list of items but got type \"dict\".".to_owned(),
            ),
            (
                Some(json("\"abc\"")),
                "Expected a list of items but got type \"str\".".to_owned(),
            ),
            (
                Some(json("5")),
                "Expected a list of items but got type \"int\".".to_owned(),
            ),
            (
                Some(json("1.5")),
                "Expected a list of items but got type \"float\".".to_owned(),
            ),
            (
                Some(json("true")),
                "Expected a list of items but got type \"bool\".".to_owned(),
            ),
        ] {
            let response = list_of(input.as_ref(), no_child)
                .await
                .expect_err("a non-list must fail");
            let ListError::Validation(response) = response else {
                panic!("a non-list is a validation failure");
            };
            let expected = Json::object(vec![(
                "non_field_errors",
                Json::Array(vec![Json::string(message)]),
            )]);
            assert_eq!(
                body(response),
                String::from_utf8(expected.to_drf()).expect("utf-8")
            );
        }
    }

    /// Only the failing indices are kept, in list order, and the validated
    /// items come back in input order when every element passes.
    #[tokio::test]
    async fn only_failing_indices_are_reported() {
        let input = json(r#"[{"value":"ok"},{"value":""},{"value":"ok"},{}]"#);
        let messages = ["value is blank", "value is required"];
        let response = list_of(Some(&input), |index, _item| {
            let message = match index {
                1 => Some(messages[0]),
                3 => Some(messages[1]),
                _ => None,
            };
            async move {
                match message {
                    Some(message) => Err(ChildError::non_field(message)),
                    None => Ok(Validated::default()),
                }
            }
        })
        .await
        .expect_err("two failures");
        let ListError::Validation(response) = response else {
            panic!("a field failure is a validation failure");
        };
        assert_eq!(
            body(response),
            r#"{"1":{"non_field_errors":["value is blank"]},"3":{"non_field_errors":["value is required"]}}"#
        );
    }

    /// A detail with quotes, a backslash and non-ASCII text is nested
    /// verbatim; the parent must not escape it a second time.
    #[tokio::test]
    async fn a_detail_is_nested_verbatim() {
        let input = json("[1]");
        let response = list_of(Some(&input), |_index, _item| async move {
            Err(ChildError::non_field(
                "Invalid pk \"x\\y\" – object does not exist. – é",
            ))
        })
        .await
        .expect_err("child failed");
        let ListError::Validation(response) = response else {
            panic!("a `validate()` failure is a validation failure");
        };
        assert_eq!(
            body(response),
            "{\"0\":{\"non_field_errors\":[\"Invalid pk \\\"x\\\\y\\\" – object does not exist. – é\"]}}"
        );
    }

    /// A child failure DRF does not catch leaves `to_internal_value` as it is:
    /// the request answers the `ViewError`, not a 400 with an index entry.
    #[tokio::test]
    async fn a_fatal_child_error_is_not_nested() {
        let input = json("[null, 1]");
        let result = list_of(Some(&input), |_index, _item| async move {
            Err::<Validated, ChildError>(ChildError::fatal(ViewError::Unknown("boom".to_owned())))
        })
        .await;
        match result {
            Err(ListError::Fatal(ViewError::Unknown(message))) => assert_eq!(message, "boom"),
            other => panic!("expected a fatal error, got {other:?}"),
        }
    }
}
