// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Small API-level checks for behaviour the golden corpus exercises through
//! the typed helpers, plus the public entry points.

use std::collections::BTreeMap;

use chrono::{TimeZone, Timelike, Utc};
use plane_serde::values::{DrfDateTime, KombuBytes, PyMapKey};
use plane_serde::{Dialect, Error, PyDecimal, to_vec, to_writer, transcode_json};
use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};

#[test]
fn top_level_none_matches_json_renderer() {
    // JSONRenderer.render(None) == b""; json.dumps(None) == "null".
    assert_eq!(to_vec(Dialect::Drf, &None::<u8>).unwrap(), b"");
    assert_eq!(to_vec(Dialect::Django, &None::<u8>).unwrap(), b"null");
    assert_eq!(to_vec(Dialect::Kombu, &None::<u8>).unwrap(), b"null");
}

#[test]
fn nested_none_is_null() {
    assert_eq!(to_vec(Dialect::Drf, &vec![None::<u8>]).unwrap(), b"[null]");
}

#[test]
fn none_map_keys_become_the_string_null() {
    let map: BTreeMap<Option<&str>, i32> = [(None, 2), (Some("a"), 1)].into_iter().collect();
    // Python: json.dumps({None: 2, "a": 1}) -> {"null": 2, "a": 1}
    assert_eq!(
        to_vec(Dialect::Django, &map).unwrap(),
        br#"{"null": 2, "a": 1}"#
    );
}

#[test]
fn py_map_key_covers_python_key_rules() {
    struct Mixed;

    impl Serialize for Mixed {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(5))?;
            map.serialize_entry(&PyMapKey::Int(1), &"a")?;
            map.serialize_entry(&PyMapKey::Bool(true), &"b")?;
            map.serialize_entry(&PyMapKey::Null, &"c")?;
            map.serialize_entry(&PyMapKey::Float(0.1), &"d")?;
            map.serialize_entry(&PyMapKey::Str("s"), &"e")?;
            map.end()
        }
    }

    assert_eq!(
        to_vec(Dialect::Drf, &Mixed).unwrap(),
        br#"{"1":"a","true":"b","null":"c","0.1":"d","s":"e"}"#
    );
}

#[test]
fn to_writer_writes_the_same_bytes() {
    let mut out = Vec::new();
    to_writer(Dialect::Django, &mut out, &vec![1.0, 1e-5]).unwrap();
    assert_eq!(out, b"[1.0, 1e-05]");
}

#[test]
fn non_finite_floats_follow_allow_nan() {
    assert!(matches!(
        to_vec(Dialect::Drf, &f64::NAN),
        Err(Error::NonFinite(value)) if value.is_nan()
    ));
    assert_eq!(to_vec(Dialect::Django, &f64::NAN).unwrap(), b"NaN");
    assert_eq!(to_vec(Dialect::Kombu, &f64::INFINITY).unwrap(), b"Infinity");
    assert_eq!(
        to_vec(Dialect::Django, &f64::NEG_INFINITY).unwrap(),
        b"-Infinity"
    );
}

#[test]
fn drf_datetime_renders_like_the_field() {
    let dt = Utc
        .with_ymd_and_hms(2026, 11, 15, 12, 34, 56)
        .single()
        .expect("valid instant")
        + chrono::TimeDelta::microseconds(120_000);
    assert_eq!(
        to_vec(Dialect::Drf, &DrfDateTime(dt)).unwrap(),
        br#""2026-11-15T12:34:56.120000Z""#
    );
    // The string form a cached `serializer.data` holds: the same bytes without
    // the JSON quotes, through both `Display` and `to_drf_string`.
    let value = DrfDateTime(dt);
    assert_eq!(value.to_drf_string(), "2026-11-15T12:34:56.120000Z");
    assert_eq!(value.to_string(), "2026-11-15T12:34:56.120000Z");

    // A non-UTC offset (Kolkata, +05:30) and a whole second (no fractional
    // part), which is where `isoformat()` and a hand-written format diverge.
    let offset = dt.with_timezone(&chrono::FixedOffset::east_opt(19_800).expect("offset"));
    let value = DrfDateTime(offset.with_nanosecond(0).expect("valid"));
    assert_eq!(value.to_drf_string(), "2026-11-15T18:04:56+05:30");
}

#[test]
fn kombu_bytes_envelope() {
    assert_eq!(
        to_vec(Dialect::Kombu, &KombuBytes(b"hello".to_vec())).unwrap(),
        br#"{"__type__": "bytes", "__value__": "hello"}"#
    );
    assert_eq!(
        to_vec(Dialect::Kombu, &KombuBytes(vec![0xFF, 0xFE])).unwrap(),
        br#"{"__type__": "base64", "__value__": "//4="}"#
    );
}

#[test]
fn decimals_quantize_like_drf() {
    let value: PyDecimal = "1.1000".parse().expect("decimal");
    let decimal = plane_serde::values::DrfDecimal::new(value, 12, 4);
    assert_eq!(to_vec(Dialect::Drf, &decimal).unwrap(), br#""1.1000""#);
}

#[test]
fn transcode_keeps_first_position_and_last_value_for_duplicates() {
    let mut out = Vec::new();
    transcode_json(Dialect::Drf, r#"{"a": 1, "b": 2, "a": 3}"#, &mut out).unwrap();
    assert_eq!(out, br#"{"a":3,"b":2}"#);
}

#[test]
fn transcode_follows_python_number_rules() {
    let cases = [
        ("-0", "0"),
        ("1.50", "1.5"),
        ("1E2", "100.0"),
        ("1e400", "Infinity"),
        (
            "123456789012345678901234567890",
            "123456789012345678901234567890",
        ),
    ];
    for (input, expected) in cases {
        let mut out = Vec::new();
        transcode_json(Dialect::Django, input, &mut out).unwrap();
        assert_eq!(out, expected.as_bytes(), "input {input}");
    }

    let mut out = Vec::new();
    assert!(matches!(
        transcode_json(Dialect::Drf, "1e400", &mut out),
        Err(Error::NonFinite(_))
    ));
}

#[test]
fn transcode_rejects_malformed_json() {
    for input in ["", "{", "[1,]", "{\"a\":}", "\"unterminated", "1 2", "01"] {
        let mut out = Vec::new();
        assert!(
            transcode_json(Dialect::Drf, input, &mut out).is_err(),
            "input {input:?} should fail"
        );
    }
}

#[test]
fn enum_representation_matches_serde_json() {
    #[derive(Serialize)]
    enum Example {
        Unit,
        Newtype(i32),
        Tuple(i32, i32),
        Struct { a: i32 },
    }

    for value in [
        Example::Unit,
        Example::Newtype(1),
        Example::Tuple(1, 2),
        Example::Struct { a: 1 },
    ] {
        let serde_json = serde_json::to_vec(&value).expect("serde_json");
        assert_eq!(
            to_vec(Dialect::Drf, &value).expect("plane-serde"),
            serde_json
        );
    }
}
