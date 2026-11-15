// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! API-level checks for `plane_serde::pyjson`: the accessors handlers use,
//! and the end-to-end behaviour of values Python allows and Rust's `str` does
//! not (lone surrogates, arbitrary-precision integers).

use plane_serde::pyjson::{LoadErrorKind, PyInt, Value, loads};
use plane_serde::{Dialect, Error, to_vec};

#[test]
fn accessors_cover_a_handler_body() {
    let body = br#"{
        "name": "Fix login",
        "priority": "high",
        "estimate": 2.5,
        "points": 3,
        "is_draft": false,
        "parent": null,
        "labels": ["bug", "backend"],
        "assignees": [{"id": "8a1b2c3d-4e5f-4a6b-8c9d-0e1f2a3b4c5d"}],
        "meta": {"nested": {"count": 12}}
    }"#;
    let value = loads(body, "utf-8").expect("valid JSON");
    assert_eq!(value.get("name").and_then(Value::as_str), Some("Fix login"));
    assert_eq!(value.get("priority").and_then(Value::as_str), Some("high"));
    assert_eq!(value.get("estimate").and_then(Value::as_f64), Some(2.5));
    assert_eq!(value.get("points").and_then(Value::as_i64), Some(3));
    assert_eq!(value.get("points").and_then(Value::as_u64), Some(3));
    assert_eq!(value.get("is_draft").and_then(Value::as_bool), Some(false));
    assert!(value.get("parent").is_some_and(Value::is_null));
    assert_eq!(
        value.get("labels").and_then(|labels| labels.index(1)),
        Some(&Value::Str("backend".into()))
    );
    assert_eq!(
        value
            .path(&["meta", "nested", "count"])
            .and_then(Value::as_i64),
        Some(12)
    );
    assert_eq!(
        value
            .get("assignees")
            .and_then(|list| list.index(0))
            .and_then(|assignee| assignee.get("id"))
            .and_then(Value::as_str),
        Some("8a1b2c3d-4e5f-4a6b-8c9d-0e1f2a3b4c5d")
    );
    assert_eq!(value.get("missing"), None);
    assert_eq!(value.len(), Some(9));
    assert_eq!(value.get("labels").and_then(Value::len), Some(2));
    assert!(!value.is_null());
}

#[test]
fn objects_keep_insertion_order_and_last_value() {
    let value = loads(br#"{"z": 1, "a": 2, "z": 3, "m": 4}"#, "utf-8").unwrap();
    let dict = value.as_dict().unwrap();
    let keys: Vec<&str> = dict.keys().filter_map(|key| key.as_str()).collect();
    assert_eq!(keys, ["z", "a", "m"]);
    assert_eq!(value.get("z").and_then(Value::as_i64), Some(3));
}

#[test]
fn integers_are_arbitrary_precision() {
    let value = loads(b"123456789012345678901234567890", "utf-8").unwrap();
    assert_eq!(
        value.as_int().map(PyInt::to_decimal_string).as_deref(),
        Some("123456789012345678901234567890")
    );
    assert_eq!(value.as_i64(), None);
    // through both dialects, as a bare number
    assert_eq!(
        to_vec(Dialect::Drf, &value).unwrap(),
        b"123456789012345678901234567890"
    );
    assert_eq!(
        to_vec(Dialect::Django, &value).unwrap(),
        b"123456789012345678901234567890"
    );

    let value = loads(b"-9223372036854775808", "utf-8").unwrap();
    assert_eq!(value.as_i64(), Some(i64::MIN));
    assert_eq!(
        to_vec(Dialect::Drf, &value).unwrap(),
        b"-9223372036854775808"
    );
}

#[test]
fn integers_over_the_digit_limit_are_a_plain_value_error() {
    let body = "9".repeat(4301);
    let error = loads(body.as_bytes(), "utf-8").unwrap_err();
    assert_eq!(error.kind(), LoadErrorKind::Json);
    assert_eq!(
        error.message(),
        "Exceeds the limit (4300 digits) for integer string conversion: value has 4301 \
         digits; use sys.set_int_max_str_digits() to increase the limit"
    );
    // A float lexeme is not subject to the limit.
    let body = format!("{}.0", "9".repeat(4301));
    assert!(
        loads(body.as_bytes(), "utf-8")
            .unwrap()
            .as_f64()
            .unwrap()
            .is_infinite()
    );
}

#[test]
fn lone_surrogates_reach_the_utf8_encoder() {
    // json.loads keeps the lone surrogate; DRF's renderer ends with
    // str.encode("utf-8"), which refuses it with a document position.
    let value = loads(br#""\ud800""#, "utf-8").unwrap();
    assert!(value.as_str().is_none());
    let text = value.as_pystr().unwrap();
    assert!(text.has_surrogates());
    match to_vec(Dialect::Drf, &value) {
        Err(Error::Custom(message)) => assert_eq!(
            message,
            "'utf-8' codec can't encode character '\\ud800' in position 1: \
             surrogates not allowed"
        ),
        other => panic!("expected a UnicodeEncodeError, got {other:?}"),
    }

    // Django's encoder escapes it instead.
    assert_eq!(to_vec(Dialect::Django, &value).unwrap(), br#""\ud800""#);

    // A run of surrogates is reported as a range, at the right position.
    let value = loads(br#"[1, 2, "\ud800\ud800"]"#, "utf-8").unwrap();
    match to_vec(Dialect::Drf, &value) {
        Err(Error::Custom(message)) => assert_eq!(
            message,
            "'utf-8' codec can't encode characters in position 6-7: \
             surrogates not allowed"
        ),
        other => panic!("expected a UnicodeEncodeError, got {other:?}"),
    }

    // A lone surrogate in a key behaves the same way.
    let value = loads(br#"{"\ud800": 1}"#, "utf-8").unwrap();
    match to_vec(Dialect::Drf, &value) {
        Err(Error::Custom(message)) => assert_eq!(
            message,
            "'utf-8' codec can't encode character '\\ud800' in position 2: \
             surrogates not allowed"
        ),
        other => panic!("expected a UnicodeEncodeError, got {other:?}"),
    }
}

#[test]
fn a_valid_surrogate_pair_becomes_one_astral_character() {
    let value = loads(br#""\ud83d\ude80""#, "utf-8").unwrap();
    assert_eq!(value.as_str(), Some("🚀"));
    assert_eq!(
        to_vec(Dialect::Django, &value).unwrap(),
        br#""\ud83d\ude80""#
    );
    assert_eq!(to_vec(Dialect::Drf, &value).unwrap(), "\"🚀\"".as_bytes());
}

#[test]
fn overflowed_floats_parse_and_then_fail_to_render_like_drf() {
    let value = loads(b"1e400", "utf-8").unwrap();
    assert_eq!(value.as_f64(), Some(f64::INFINITY));
    assert!(matches!(
        to_vec(Dialect::Drf, &value),
        Err(Error::NonFinite(found)) if found == f64::INFINITY
    ));
    assert_eq!(to_vec(Dialect::Django, &value).unwrap(), b"Infinity");
}

#[test]
fn top_level_null_renders_as_an_empty_drf_body() {
    let value = loads(b"null", "utf-8").unwrap();
    assert_eq!(to_vec(Dialect::Drf, &value).unwrap(), b"");
    assert_eq!(to_vec(Dialect::Django, &value).unwrap(), b"null");
}

#[test]
fn a_parsed_body_re_renders_compactly() {
    let body = br#"{ "b": [1, 2.5, "x"], "a": {"k": null} }"#;
    let value = loads(body, "utf-8").unwrap();
    assert_eq!(
        to_vec(Dialect::Drf, &value).unwrap(),
        br#"{"b":[1,2.5,"x"],"a":{"k":null}}"#
    );
}

#[test]
fn deep_values_parse_and_drop_without_recursion() {
    let depth = 9997;
    let body = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
    let value = loads(body.as_bytes(), "utf-8").unwrap();
    let mut current = &value;
    for level in 0..depth - 1 {
        let items = current
            .as_list()
            .unwrap_or_else(|| panic!("level {level} is not a list"));
        assert_eq!(items.len(), 1);
        current = &items[0];
    }
    assert!(current.as_list().is_some_and(<[Value]>::is_empty));
    drop(value);
}

#[test]
fn error_details_are_the_drf_parse_error_payload() {
    let error = loads(br#"{"a":1,}"#, "utf-8").unwrap_err();
    assert_eq!(
        error.parse_error_detail().as_deref(),
        Some(
            "JSON parse error - Expecting property name enclosed in double quotes: \
             line 1 column 8 (char 7)"
        )
    );

    let body = format!("{}{}", "[".repeat(9998), "]".repeat(9998));
    let error = loads(body.as_bytes(), "utf-8").unwrap_err();
    assert_eq!(error.kind(), LoadErrorKind::Recursion);
    assert_eq!(
        error.message(),
        "maximum recursion depth exceeded while decoding a JSON array from a unicode string"
    );
    assert!(!error.is_parse_error());
}
