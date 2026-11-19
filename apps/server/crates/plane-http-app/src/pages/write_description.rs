// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `PagesDescriptionViewSet.partial_update` (`plane/app/views/page/base.py:518-575`),
//! route 130 `PATCH …/projects/{project_id}/pages/{page_id}/description/`.
//!
//! ## Order
//!
//! 1. `Page.objects.get(Q(owned_by=user) | Q(access=0), pk=…,
//!    workspace__slug=…, projects__id=…, project_pages__deleted_at__isnull=True)`
//!    (`base.py:518-523`) — before the body is parsed, so a malformed body on a
//!    missing page is the 404. (`ProjectPagePermission` has already answered
//!    403 for every case where this `get` could miss.)
//! 2. `is_locked` → `400 {"error_code": 4701, "error_message": "PAGE_LOCKED"}`
//!    (`base.py:526-534`), then `archived_at` → 4702 `PAGE_ARCHIVED`
//!    (`base.py:536-543`). Both bodies are two keys **in that order**
//!    (`plane/utils/error_codes.py:12-13`).
//! 3. `old_description_html = page.description_html` and
//!    `existing_instance = json.dumps({"description_html": old},
//!    cls=DjangoJSONEncoder)` (`base.py:545-550`) — Django's encoder, i.e.
//!    `", "`/`": "` separators with `ensure_ascii=True`.
//! 4. `PageBinaryUpdateSerializer(page, data=request.data, partial=True)`;
//!    `is_valid()` validates the three declared fields **in declaration order**
//!    (`description_binary`, `description_html`, `description_json`) and runs
//!    each `validate_<field>` right after its own field validation, so the
//!    error keys follow that order (`rest_framework/serializers.py:493-529`).
//! 5. `serializer.save()` → `update()` assigns the three fields and calls
//!    `instance.save()` — a full save (H26 + H01).
//! 6. `page_transaction` **only** when `request.data.get("description_html")`
//!    is truthy, with the **raw** body value (`base.py:556-562`); then
//!    `track_page_version` on every valid call, even an empty body
//!    (`base.py:565-569`). Both are published after the write.
//! 7. `200 {"message": "Updated successfully"}`.
//!
//! ## What the serializer validates
//!
//! `description_binary` and `description_html` are `CharField`s with
//! `allow_blank=True` (`serializers/page.py:176-178`): DRF trims them, refuses
//! non-scalars, and runs `ProhibitNullCharactersValidator`
//! (`rest_framework/fields.py:549-568`). Their `validate_*` hooks
//! (`serializers/page.py:180-221`) are **not** in the generated table
//! (`write/generated/page_binary_update_serializer.rs` has the three field
//! calls only), so this module calls the validators from
//! [`crate::write::description`] between the field calls, in DRF's place.
//!
//! The hook's returned values are what `update()` writes: the **decoded bytes**
//! for a truthy `description_binary` ([`PageBinary`]), the **sanitised** HTML
//! for a truthy `description_html`, and the JSON value as parsed for
//! `description_json` — including an explicit `null`, which reaches the
//! `NOT NULL` column and becomes the family 400
//! `{"error": "The payload is not valid"}` (`views/base.py:209-213`).
//!
//! ## Statements
//!
//! | Step | Django | Here |
//! | --- | --- | --- |
//! | lookup | 1 | 1 |
//! | save | 1 `UPDATE` | 1 |
//! | tasks | 2 publishes | 2 publishes, in the same order |

use plane_core::error::APPLICATION_JSON;
use plane_drf::error::ViewError;
use plane_drf::view::{BoxFuture, ViewCtx, ViewResponse};
use plane_jobs::celery::args::TaskArg;
use plane_jobs::celery::task_name;
use plane_jobs::queue::Job;
use plane_query::json::Json;

use crate::common::{client, jobs, lookup};
use crate::pages::write_action::{self, PageWrite};
use crate::views::write_support::{Body, body_get};
use crate::write::{CharOptions, Detail, Serializer, description};

/// `errors` from `plane/utils/error_codes.py:12-13`.
const PAGE_LOCKED: i64 = 4701;
const PAGE_ARCHIVED: i64 = 4702;

/// The `CharField(required=False, allow_blank=True)` options both text fields
/// carry (`serializers/page.py:176-177`), as the generated table records them.
fn char_options() -> CharOptions {
    CharOptions {
        allow_null: false,
        allow_blank: true,
        trim_whitespace: true,
        max_length: None,
    }
}

/// `{"error_code": 4701, "error_message": "PAGE_LOCKED"}` (`base.py:526-534`).
fn locked_body() -> ViewResponse {
    two_key_error(400, PAGE_LOCKED, "PAGE_LOCKED")
}

/// `{"error_code": 4702, "error_message": "PAGE_ARCHIVED"}` (`base.py:536-543`).
fn archived_body() -> ViewResponse {
    two_key_error(400, PAGE_ARCHIVED, "PAGE_ARCHIVED")
}

/// The two-key body, `error_code` first.
fn two_key_error(status: u16, code: i64, message: &str) -> ViewResponse {
    ViewResponse::raw(
        status,
        APPLICATION_JSON,
        Json::Object(vec![
            ("error_code".to_owned(), Json::Int(code)),
            ("error_message".to_owned(), Json::Str(message.to_owned())),
        ])
        .to_drf(),
    )
}

/// `CharField`'s `ProhibitNullCharactersValidator`
/// (`rest_framework/fields.py:564-568`, `rest_framework/validators.py:161-178`).
///
/// `CharField.__init__` appends two character validators after the length ones;
/// a NUL is DRF's `"Null characters are not allowed."`. Rust strings cannot
/// carry a lone surrogate, so `ProhibitSurrogateCharactersValidator` has no
/// reachable input here.
fn char_validator_detail(text: &str) -> Option<Detail> {
    if text.contains('\0') {
        return Some(Detail::new(
            "Null characters are not allowed.",
            "null_characters_not_allowed",
        ));
    }
    None
}

/// `json.dumps({"description_html": html}, cls=DjangoJSONEncoder)`
/// (`base.py:549`): Django's encoder is `ensure_ascii=True` with the default
/// `", "`/`": "` separators.
fn existing_instance(html: &str) -> Result<String, ViewError> {
    let value = plane_serde::pyjson::Value::Dict({
        let mut dict = plane_serde::pyjson::PyDict::new();
        dict.insert(
            plane_serde::pyjson::PyStr::new("description_html"),
            plane_serde::pyjson::Value::Str(plane_serde::pyjson::PyStr::new(html)),
        );
        dict
    });
    let bytes = plane_serde::to_vec(plane_serde::Dialect::Django, &value)
        .map_err(|error| ViewError::Unknown(error.to_string()))?;
    String::from_utf8(bytes)
        .map_err(|error| ViewError::Unknown(format!("json.dumps produced non-UTF-8: {error}")))
}

/// Python truthiness of the raw body value (`if request.data.get(...)`).
fn truthy(value: &plane_serde::pyjson::Value) -> bool {
    use plane_serde::pyjson::Value as Py;
    match value {
        Py::Null => false,
        Py::Bool(flag) => *flag,
        Py::Int(int) => int.as_i64().is_some_and(|value| value != 0),
        Py::Float(float) => *float != 0.0,
        Py::Str(text) => text.as_str().is_some_and(|text| !text.is_empty()),
        Py::List(items) => !items.is_empty(),
        Py::Dict(members) => !members.is_empty(),
    }
}

/// `request.data.get(key)` as the task argument `.delay()` would carry.
///
/// A JSON member keeps its type (`int` stays an int in the Celery body, exactly
/// as `page_transaction.delay(new_description_html=123)` sends it); a form
/// member is always a `str`.
fn request_task_arg(
    data: &plane_drf::parsers::Data,
    key: &str,
) -> Result<Option<TaskArg>, ViewError> {
    Ok(match body_get(data, key)? {
        Body::Missing => None,
        Body::Form(text) => Some(TaskArg::Str(text.to_owned())),
        Body::Json(value) => Some(TaskArg::Json(value.clone())),
    })
}

/// `PagesDescriptionViewSet.partial_update` (`base.py:518-575`).
pub fn partial_update<'a>(ctx: &'a ViewCtx<'a>) -> BoxFuture<'a, Result<ViewResponse, ViewError>> {
    Box::pin(async move {
        let client = client(ctx)?;
        let slug = ctx
            .kwarg("slug")
            .map(str::to_string)
            .ok_or(ViewError::Key)?;
        let project_id =
            lookup::uuid_kwarg("project_id", ctx.kwarg("project_id").ok_or(ViewError::Key)?)?;
        let page_id = lookup::uuid_kwarg("page_id", ctx.kwarg("page_id").ok_or(ViewError::Key)?)?;
        let user = jobs::user_id(ctx)?;

        let mut page: PageWrite =
            write_action::load_description_page(client, &slug, project_id, page_id, user).await?;

        if page.is_locked {
            return Ok(locked_body());
        }
        if page.archived_at.is_some() {
            return Ok(archived_body());
        }

        let old_description_html = page.description_html.clone();
        let existing_instance = existing_instance(&old_description_html)?;

        // `PageBinaryUpdateSerializer(page, data=request.data, partial=True)`.
        let mut serializer = Serializer::new(ctx, true)?;

        // `description_binary`: the field, then `validate_description_binary`.
        let mut binary = None;
        if let Some(text) = serializer.char_field("description_binary", char_options()) {
            if let Some(detail) = char_validator_detail(&text) {
                serializer.reject("description_binary", detail);
            } else {
                match description::validate_page_binary(&text) {
                    Ok(value) => binary = Some(value),
                    Err(message) => {
                        serializer.reject("description_binary", Detail::new(message, "invalid"))
                    }
                }
            }
        }

        // `description_html`: the field, then `validate_description_html`.
        let mut html = None;
        if let Some(text) = serializer.char_field("description_html", char_options()) {
            if let Some(detail) = char_validator_detail(&text) {
                serializer.reject("description_html", detail);
            } else {
                match description::validate_html_reason(&text) {
                    // A falsy input is returned unchanged (`if not value`).
                    Ok(None) => html = Some(text),
                    Ok(Some(clean)) => html = Some(clean),
                    Err(message) => {
                        serializer.reject("description_html", Detail::new(message, "invalid"));
                    }
                }
            }
        }

        // `description_json = serializers.JSONField(required=False,
        // allow_null=True)`.
        let description_json = serializer.json_field("description_json", true);
        let description_json_present = serializer.value("description_json").is_some();

        let _ = match serializer.finish() {
            Ok(values) => values,
            Err(errors) => return Ok(errors.into_response()),
        };

        // `update()`: assign what the validated data carried, then `save()`.
        if let Some(html) = html {
            page.description_html = html;
        }
        if let Some(value) = binary {
            match value {
                description::PageBinary::Bytes(bytes) => page.description_binary = Some(bytes),
                // The falsy input `""` reaches the `bytea` column as a `str`;
                // psycopg sends it as text and Postgres refuses the assignment
                // (`DataError`), which no view catches — the family 500.
                description::PageBinary::Blank(text) => {
                    return Err(ViewError::Unknown(format!(
                        "psycopg.errors.DatatypeMismatch: column \"description_binary\" is of type bytea \
                         but expression is of type text (received {text:?})"
                    )));
                }
            }
        }
        if description_json_present {
            // An explicit `null` reaches the `NOT NULL` column: `IntegrityError`
            // → the family 400 `{"error": "The payload is not valid"}`.
            page.description_json = description_json;
        }
        page.save(client, user).await?;

        // `if request.data.get("description_html"): page_transaction.delay(…)`
        // with the **raw** request HTML (`base.py:556-562`).
        let data = ctx.data()?;
        let raw_html = request_task_arg(data, "description_html")?;
        if let Some(raw_html) = raw_html.filter(task_arg_truthy) {
            let task = task_name::PAGE_TRANSACTION;
            let message = page_transaction_message(
                raw_html,
                Some(old_description_html.clone()),
                TaskArg::uuid(page_id),
            );
            let job = Job::from_message(&message);
            jobs::dispatch_task(ctx, task, &message, job).await?;
        }

        // `track_page_version.delay(…)` on every valid call (`base.py:565-569`).
        let task = task_name::TRACK_PAGE_VERSION;
        let message = plane_jobs::celery::tasks::track_page_version(
            page_id,
            TaskArg::json_text(existing_instance),
            user,
        );
        let job = Job::from_message(&message);
        jobs::dispatch_task(ctx, task, &message, job).await?;

        Ok(ViewResponse::raw(
            200,
            APPLICATION_JSON,
            Json::Object(vec![(
                "message".to_owned(),
                Json::Str("Updated successfully".to_owned()),
            )])
            .to_drf(),
        ))
    })
}

/// `page_transaction.delay(new_description_html=…, old_description_html=…,
/// page_id=…)` (`base.py:557-561`) with the **raw** body value.
///
/// `plane_jobs::celery::tasks::page_transaction` takes a `String`, which is the
/// shape every call site in `TASKS.md` records — but Django passes
/// `request.data.get("description_html")` unchanged, so a JSON member `123`
/// travels as a number in the Celery body while the column stores `"123"`
/// (`CharField.to_internal_value`'s `str(data)`). This is the same message with
/// the argument left as it arrived; the kwargs are in Django's order, which is
/// `inspect.getcallargs`' order of the task signature
/// (`plane/bgtasks/page_transaction_task.py`).
fn page_transaction_message(
    new_description_html: TaskArg,
    old_description_html: Option<String>,
    page_id: TaskArg,
) -> plane_jobs::celery::Message {
    plane_jobs::celery::Message::new(
        task_name::PAGE_TRANSACTION,
        Vec::new(),
        vec![
            ("new_description_html", new_description_html),
            (
                "old_description_html",
                old_description_html.map_or(TaskArg::Null, TaskArg::Str),
            ),
            ("page_id", page_id),
        ],
    )
}

/// `if request.data.get("description_html")` for a task argument.
fn task_arg_truthy(value: &TaskArg) -> bool {
    match value {
        TaskArg::Null => false,
        TaskArg::Bool(flag) => *flag,
        TaskArg::Int(int) => *int != 0,
        TaskArg::Float(float) => *float != 0.0,
        TaskArg::Str(text) | TaskArg::JsonText(text) => !text.is_empty(),
        TaskArg::List(items) => !items.is_empty(),
        TaskArg::Map(entries) => !entries.is_empty(),
        TaskArg::Uuid(_) => true,
        TaskArg::Json(value) => truthy(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write::generated::page_binary_update_serializer;

    #[test]
    fn existing_instance_is_django_json_dumps() {
        assert_eq!(
            existing_instance("<p>a & b</p>").expect("json"),
            r#"{"description_html": "<p>a & b</p>"}"#
        );
        // `ensure_ascii=True` and the default `", "`/`": "` separators.
        assert_eq!(
            existing_instance("caf\u{e9} \u{1F600}").expect("json"),
            r#"{"description_html": "caf\u00e9 \ud83d\ude00"}"#
        );
        assert_eq!(
            existing_instance("a \"quoted\" line\n").expect("json"),
            "{\"description_html\": \"a \\\"quoted\\\" line\\n\"}"
        );
        assert_eq!(
            existing_instance("").expect("json"),
            r#"{"description_html": ""}"#
        );
    }

    /// The three-key body is `error_code` then `error_message`, and the numbers
    /// are `plane/utils/error_codes.py`'s.
    #[test]
    fn the_locked_and_archived_bodies_are_two_keys_in_order() {
        for (response, expected) in [
            (
                locked_body(),
                r#"{"error_code":4701,"error_message":"PAGE_LOCKED"}"#,
            ),
            (
                archived_body(),
                r#"{"error_code":4702,"error_message":"PAGE_ARCHIVED"}"#,
            ),
        ] {
            match response {
                ViewResponse::Raw { status, body, .. } => {
                    assert_eq!(status, 400);
                    assert_eq!(String::from_utf8(body).expect("utf8"), expected);
                }
                other => panic!("expected a rendered body, got {other:?}"),
            }
        }
    }

    /// The hand-written field calls agree with the generated table: same order
    /// (`description_binary`, `description_html`, `description_json`) and the
    /// same `allow_null`/`allow_blank`.
    #[test]
    fn the_field_calls_match_the_generated_table() {
        let fields = page_binary_update_serializer::FIELDS;
        let names: Vec<&str> = fields.iter().map(|field| field.name).collect();
        assert_eq!(
            names,
            ["description_binary", "description_html", "description_json"]
        );
        for field in &fields[..2] {
            let options = char_options();
            assert_eq!(field.allow_null, options.allow_null, "{}", field.name);
            assert_eq!(field.allow_blank, options.allow_blank, "{}", field.name);
            assert_eq!(field.max_length, options.max_length, "{}", field.name);
        }
        assert!(fields[2].allow_null, "description_json allows null");
    }

    #[test]
    fn the_nul_validator_is_drf_s_message() {
        assert_eq!(char_validator_detail("plain"), None);
        assert_eq!(
            char_validator_detail("a\0b"),
            Some(Detail::new(
                "Null characters are not allowed.",
                "null_characters_not_allowed"
            ))
        );
    }

    #[test]
    fn task_truthiness_follows_python() {
        use plane_serde::pyjson;
        for (text, expected) in [
            ("null", false),
            ("false", false),
            ("true", true),
            ("0", false),
            ("0.0", false),
            ("1", true),
            ("\"\"", false),
            ("\" \"", true),
            ("[]", false),
            ("[1]", true),
            ("{}", false),
            ("{\"a\": 1}", true),
        ] {
            let value: pyjson::Value = pyjson::loads(text.as_bytes(), "utf-8").expect("json");
            assert_eq!(truthy(&value), expected, "{text}");
        }
        assert!(!task_arg_truthy(&TaskArg::Str(String::new())));
        assert!(task_arg_truthy(&TaskArg::Str(" ".to_owned())));
        assert!(!task_arg_truthy(&TaskArg::Null));
    }
}
