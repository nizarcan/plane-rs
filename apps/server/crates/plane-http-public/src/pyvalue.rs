// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Python `str(value)` for the opaque JSON values the v1 views hand to the
//! ORM as filter values or to a `CharField` as a column value.
//!
//! Django's `CharField.get_prep_value` is `str(value)` for everything that is
//! not a `str`/`None`, and a raw body value used as a lookup value goes through
//! the same conversion. Several v1 views compare `str(request.data.get(…))`
//! against a column (`api/views/state.py:294`), so the exact Python spelling
//! — `True`/`False`, `None`, `[1, 2]`, `{'a': 'b'}` — is observable in the
//! query result.
//!
//! `dict` iteration order is insertion order, which is what the parsed body
//! keeps.

use plane_serde::pyjson::Value as Json;

/// Python `str(value)`.
pub(crate) fn python_str(value: &Json) -> String {
    match value {
        Json::Null => "None".to_owned(),
        Json::Bool(value) => {
            if *value {
                "True".to_owned()
            } else {
                "False".to_owned()
            }
        }
        Json::Int(value) => value.to_string(),
        Json::Float(value) => plane_query::pyfloat::repr(*value),
        Json::Str(value) => value.as_str().unwrap_or_default().to_owned(),
        Json::List(items) => {
            let parts: Vec<String> = items.iter().map(python_repr).collect();
            format!("[{}]", parts.join(", "))
        }
        Json::Dict(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(key, value)| {
                    format!(
                        "'{}': {}",
                        key.as_str().unwrap_or_default(),
                        python_repr(value)
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

/// Python `repr(value)` for the elements of a composite `str()`.
fn python_repr(value: &Json) -> String {
    match value {
        Json::Str(text) => quote(text.as_str().unwrap_or_default()),
        other => python_str(other),
    }
}

/// CPython's `repr` quoting for a `str` (single quotes unless the value
/// contains one and no double quote).
fn quote(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for character in text.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use plane_serde::pyjson::PyStr;

    #[test]
    fn scalars_use_python_spelling() {
        assert_eq!(python_str(&Json::Null), "None");
        assert_eq!(python_str(&Json::Bool(true)), "True");
        assert_eq!(python_str(&Json::Bool(false)), "False");
        assert_eq!(python_str(&Json::from(7)), "7");
        assert_eq!(python_str(&Json::Str(PyStr::new("x"))), "x");
        assert_eq!(python_str(&Json::Float(1.0)), "1.0");
    }

    #[test]
    fn containers_use_python_repr() {
        let list = Json::List(vec![
            Json::from(1),
            Json::Str(PyStr::new("a")),
            Json::Bool(true),
        ]);
        assert_eq!(python_str(&list), "[1, 'a', True]");
    }
}
