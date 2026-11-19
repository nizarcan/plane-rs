// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Python `repr()` for the JSON values a webhook task stores.
//!
//! `save_webhook_log` (`plane/bgtasks/webhook_task.py:104-115`) passes a
//! `dict` into a `TextField`: Django's `str()` is the dict's `repr`, so
//! `request_headers` and `request_body` are
//! `{'Content-Type': 'application/json', ...}` with single quotes, `None` for
//! a null and `True`/`False` for booleans. The shape is fixed by
//! `WebhookLog.request_headers`/`request_body` (`webhook_task.py:328-352`):
//! only `str`, `int`, `float`, `bool`, `None`, `list` and `dict` occur.
//!
//! A Python `repr` of a `str` chooses `'` unless the text contains a single
//! quote and no double quote, in which case it uses `"`; it escapes the quote
//! and `\`, writes `\n`, `\t`, `\r` and `\xNN`/`\uNNNN`/`\UNNNNNNNN` for
//! non-printable code points, and leaves printable text (including
//! non-ASCII) verbatim (`Objects/unicodeobject.c`, `unicode_repr`).

use plane_serde::pyjson::Value;

/// `repr(value)` for a value `json.loads` can produce.
#[must_use]
pub fn python_repr(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("None"),
        Value::Bool(true) => out.push_str("True"),
        Value::Bool(false) => out.push_str("False"),
        Value::Int(number) => out.push_str(&number.to_decimal_string()),
        Value::Float(number) => match plane_serde::float_repr(*number) {
            Some(text) => out.push_str(&text),
            None if number.is_nan() => out.push_str("nan"),
            None if number.is_sign_negative() => out.push_str("-inf"),
            None => out.push_str("inf"),
        },
        Value::Str(text) => write_str(out, text),
        Value::List(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Dict(dict) => {
            out.push('{');
            for (index, (key, item)) in dict.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                write_str(out, key);
                out.push_str(": ");
                write_value(out, item);
            }
            out.push('}');
        }
    }
}

/// Python's `repr` for a `str`.
fn write_str(out: &mut String, text: &plane_serde::pyjson::PyStr) {
    let Some(text) = text.as_str() else {
        // A lone surrogate cannot be represented in Rust's `str`; the lossy
        // form keeps the printable characters, which is what a webhook log
        // built from a valid UTF-8 header/body carries anyway.
        let lossy = text.to_string_lossy();
        write_str_text(out, &lossy);
        return;
    };
    write_str_text(out, text);
}

fn write_str_text(out: &mut String, text: &str) {
    // `pyurl::repr_str` is the same Python `repr` the response-header field
    // uses; keep one implementation so the two log columns cannot drift.
    out.push_str(&plane_net::pyurl::repr_str(text));
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::pyjson;

    fn repr(text: &str) -> String {
        python_repr(&pyjson::loads(text.as_bytes(), "utf-8").expect("json"))
    }

    #[test]
    fn scalars_match_python() {
        assert_eq!(repr("null"), "None");
        assert_eq!(repr("true"), "True");
        assert_eq!(repr("false"), "False");
        assert_eq!(repr("1"), "1");
        assert_eq!(repr("1.0"), "1.0");
        assert_eq!(repr("-2.5"), "-2.5");
        assert_eq!(repr("\"a\""), "'a'");
        assert_eq!(repr("\"it's\""), "\"it's\"");
        assert_eq!(repr("\"bo'th\""), "\"bo'th\"");
        assert_eq!(repr("\"a\\\\b\""), "'a\\\\b'");
        assert_eq!(repr("\"a\\nb\""), "'a\\nb'");
        assert_eq!(repr("\"\\u0001\""), "'\\x01'");
        assert_eq!(repr("\"h\\u00e9\""), "'hé'");
    }

    #[test]
    fn containers_match_python() {
        assert_eq!(repr("[1, \"a\", null]"), "[1, 'a', None]");
        assert_eq!(repr("{\"b\": 1, \"a\": [true]}"), "{'b': 1, 'a': [True]}");
    }
}
