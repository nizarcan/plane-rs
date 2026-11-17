// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Python `repr()` for the values an error message interpolates.
//!
//! Several views build a message with an f-string over a value they did not
//! validate, so the response body contains Python's `repr`:
//!
//! * `WorkspaceInvitationsViewset.create`
//!   (`plane/app/views/workspace/invite.py:105-109`):
//!   `f"Invalid email - {email} provided a valid email address is required to
//!   send the invite"`, where `email` is one element of the raw `emails` list —
//!   normally a dict such as `{'email': 'x', 'role': 5}`;
//! * `WorkspaceUserLinkSerializer.validate_url`
//!   (`plane/app/serializers/workspace.py:201-208`) raises with a dict, not a
//!   repr, so it is not a caller.
//!
//! The rules are CPython 3.12's:
//!
//! | Type | `repr` |
//! | --- | --- |
//! | `None` | `None` |
//! | `bool` | `True` / `False` |
//! | `int` | the decimal digits, any precision |
//! | `float` | `float.__repr__` (`1.0`, `1e+16`, `-0.0`) |
//! | `str` | [`plane_net::pyurl::repr_str`] (quote choice and escapes) |
//! | `list` | `[a, b]`, `[]` when empty |
//! | `dict` | `{k: v}`, `{}` when empty, insertion order |
//!
//! A string that cannot be represented as UTF-8 (a WTF-8 lone surrogate from
//! `json.loads`) falls back to a lossy rendering; CPython would print
//! `'\ud800'`. That value can only reach the invitation message, which the
//! fixture and the scenarios never send.

use plane_serde::pyjson::Value;

/// `repr(value)` for a value `json.loads` produced.
#[must_use]
pub fn python_repr(value: &Value) -> String {
    let mut out = String::with_capacity(16);
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("None"),
        Value::Bool(true) => out.push_str("True"),
        Value::Bool(false) => out.push_str("False"),
        Value::Int(number) => out.push_str(&number.to_decimal_string()),
        Value::Float(number) => {
            if number.is_nan() {
                // `repr(float("nan"))`; `json.loads("NaN")` reaches here.
                out.push_str("nan");
            } else if number.is_infinite() {
                out.push_str(if *number > 0.0 { "inf" } else { "-inf" });
            } else if let Some(repr) = plane_serde::float_repr(*number) {
                out.push_str(&repr);
            }
        }
        Value::Str(text) => match text.as_str() {
            Some(text) => out.push_str(&plane_net::pyurl::repr_str(text)),
            // See the module docs: a lone surrogate has no `&str` form.
            None => out.push_str(&String::from_utf8_lossy(text.as_wtf8())),
        },
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
        Value::Dict(entries) => {
            out.push('{');
            for (index, (key, item)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                match key.as_str() {
                    Some(key) => out.push_str(&plane_net::pyurl::repr_str(key)),
                    None => out.push_str(&String::from_utf8_lossy(key.as_wtf8())),
                }
                out.push_str(": ");
                write_value(out, item);
            }
            out.push('}');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::pyjson::loads;

    fn repr_of(json: &str) -> String {
        python_repr(&loads(json.as_bytes(), "utf-8").expect("parse"))
    }

    /// The expectations are CPython 3.12.13's `repr(json.loads(...))`.
    #[test]
    fn scalars_match_python() {
        assert_eq!(repr_of("null"), "None");
        assert_eq!(repr_of("true"), "True");
        assert_eq!(repr_of("false"), "False");
        assert_eq!(repr_of("5"), "5");
        assert_eq!(repr_of("-0"), "0");
        assert_eq!(
            repr_of("123456789012345678901234567890"),
            "123456789012345678901234567890"
        );
        assert_eq!(repr_of("1.0"), "1.0");
        assert_eq!(repr_of("1e16"), "1e+16");
        assert_eq!(repr_of("0.0001"), "0.0001");
        assert_eq!(repr_of("-0.0"), "-0.0");
        assert_eq!(repr_of(r#""abc""#), "'abc'");
        assert_eq!(repr_of(r#""a'b""#), "\"a'b\"");
        assert_eq!(repr_of(r#""a\"b'c""#), "'a\"b\\'c'");
        assert_eq!(repr_of(r#""a\nb""#), "'a\\nb'");
        assert_eq!(repr_of(r#""caf\u00e9""#), "'café'");
    }

    #[test]
    fn containers_match_python() {
        assert_eq!(repr_of("[]"), "[]");
        assert_eq!(repr_of("{}"), "{}");
        assert_eq!(repr_of(r#"[1, "a", null]"#), "[1, 'a', None]");
        assert_eq!(repr_of(r#"[[]]"#), "[[]]");
        // Insertion order, and `: ` / `, ` separators.
        assert_eq!(
            repr_of(r#"{"email": "x@y", "role": 5}"#),
            "{'email': 'x@y', 'role': 5}"
        );
        assert_eq!(
            repr_of(r#"{"email": {"role": 5}, "list": [true, false]}"#),
            "{'email': {'role': 5}, 'list': [True, False]}"
        );
    }
}
