// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! django-redis value decoding.
//!
//! `django-redis` stores integers as the raw Redis string and everything else
//! as a pickle (`py:django_redis/client/default.py:563-596`); Python 3.12 uses
//! protocol 4 (`pickle.DEFAULT_PROTOCOL`). Throttle histories are
//! `PickleSerializer` frames of a list of floats, and `plane/utils/cache.py`
//! caches `{"data": ..., "status": ...}` dictionaries the same way.
//!
//! `serde-pickle` decodes the frames into Python's built-in values; this module
//! turns those into a deterministic JSON-ish representation so the comparator
//! can use `serde_json` for everything. Bytes and non-string dictionary keys
//! get explicit marker objects: a `HashMap` with an integer key would otherwise
//! silently lose information.

use std::str::FromStr;

use serde_json::{Map, Value};

/// One decoded Redis string.
#[derive(Debug, Clone, PartialEq)]
pub enum RedisValue {
    /// A raw integer, which django-redis does not pickle.
    Integer(i64),
    /// A pickle frame, decoded.
    Pickled(Value),
    /// A pickle frame `serde-pickle` cannot decode (an arbitrary Python object
    /// with a state the crate does not model). The harness compares its
    /// presence and byte length only.
    Unreadable,
    /// Not a django-redis encoding at all (no pickle header, not an integer).
    /// UTF-8 is kept verbatim, other bytes as a `$bytes` marker.
    Raw(Value),
}

/// Decode the value of one Redis `string` key.
pub fn decode_string(bytes: &[u8]) -> RedisValue {
    // django-redis tries `int(value)` first, so b"42" is the integer 42.
    if let Ok(text) = std::str::from_utf8(bytes)
        && !text.is_empty()
        && let Ok(value) = text.parse::<i64>()
    {
        return RedisValue::Integer(value);
    }
    if bytes.first() == Some(&0x80) {
        return match serde_pickle::value_from_slice(bytes, Default::default()) {
            Ok(value) => RedisValue::Pickled(pickle_to_json(&value)),
            Err(_) => RedisValue::Unreadable,
        };
    }
    RedisValue::Raw(match std::str::from_utf8(bytes) {
        Ok(text) => Value::String(text.to_string()),
        Err(_) => bytes_marker(bytes),
    })
}

fn bytes_marker(bytes: &[u8]) -> Value {
    let mut marker = Map::new();
    marker.insert("$bytes".to_string(), Value::String(hex(bytes)));
    Value::Object(marker)
}

/// Convert a decoded pickle into JSON.
///
/// Dictionaries keep string keys as objects; a dictionary with any non-string
/// key becomes `{"$dict": [[key, value], ...]}` so key type and order survive.
/// Sets are arrays in their `BTreeSet` order, which serde-pickle makes
/// deterministic across processes.
pub fn pickle_to_json(value: &serde_pickle::Value) -> Value {
    use serde_pickle::Value as P;
    match value {
        P::None => Value::Null,
        P::Bool(value) => Value::Bool(*value),
        P::I64(value) => number_i64(*value),
        P::Int(value) => Value::from_str(&value.to_string()).unwrap_or(Value::Null),
        P::F64(value) => number_f64(*value),
        P::Bytes(bytes) => bytes_marker(bytes),
        P::String(text) => Value::String(text.clone()),
        P::List(items) | P::Tuple(items) => {
            Value::Array(items.iter().map(pickle_to_json).collect())
        }
        P::Set(items) | P::FrozenSet(items) => Value::Array(
            items
                .iter()
                .map(|item| pickle_to_json(&item.clone().into_value()))
                .collect(),
        ),
        P::Dict(entries) => {
            // `uuid.UUID.__getstate__` is its 128-bit integer, so a pickled
            // Django `UUID` arrives as `{"int": <u128>}`. Turn it back into
            // the UUID string so the normaliser placeholders it like the same
            // UUID in a response body.
            if let Some(uuid) = uuid_from_state(entries) {
                return Value::String(uuid);
            }
            let all_strings = entries
                .keys()
                .all(|key| matches!(key, serde_pickle::HashableValue::String(_)));
            if all_strings {
                let mut map = Map::with_capacity(entries.len());
                for (key, value) in entries {
                    map.insert(hashable_key_text(key), pickle_to_json(value));
                }
                Value::Object(map)
            } else {
                let mut marker = Map::new();
                marker.insert(
                    "$dict".to_string(),
                    Value::Array(
                        entries
                            .iter()
                            .map(|(key, value)| {
                                Value::Array(vec![
                                    Value::String(hashable_key_text(key)),
                                    pickle_to_json(value),
                                ])
                            })
                            .collect(),
                    ),
                );
                Value::Object(marker)
            }
        }
    }
}

fn hashable_key_text(key: &serde_pickle::HashableValue) -> String {
    use serde_pickle::HashableValue as H;
    match key {
        H::None => "None".to_string(),
        H::Bool(value) => value.to_string(),
        H::I64(value) => value.to_string(),
        H::Int(value) => value.to_string(),
        H::F64(value) => format!("{value}"),
        H::Bytes(bytes) => format!("<bytes:{}>", hex(bytes)),
        H::String(text) => text.clone(),
        H::Tuple(items) => format!(
            "({})",
            items
                .iter()
                .map(hashable_key_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        H::FrozenSet(items) => format!(
            "frozenset({{{}}})",
            items
                .iter()
                .map(hashable_key_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// A `{"int": <128-bit>}` dictionary is how a pickled `uuid.UUID` appears.
fn uuid_from_state(
    entries: &std::collections::BTreeMap<serde_pickle::HashableValue, serde_pickle::Value>,
) -> Option<String> {
    if entries.len() != 1 {
        return None;
    }
    let (key, value) = entries.iter().next()?;
    match key {
        serde_pickle::HashableValue::String(name) if name == "int" => {}
        _ => return None,
    }
    let number = match value {
        serde_pickle::Value::I64(value) => u128::try_from(*value).ok()?,
        serde_pickle::Value::Int(value) => value.to_string().parse::<u128>().ok()?,
        _ => return None,
    };
    if number > u128::from(u64::MAX) << 64 {
        return None;
    }
    let b = number.to_be_bytes();
    Some(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0],
        b[1],
        b[2],
        b[3],
        b[4],
        b[5],
        b[6],
        b[7],
        b[8],
        b[9],
        b[10],
        b[11],
        b[12],
        b[13],
        b[14],
        b[15]
    ))
}

fn number_i64(value: i64) -> Value {
    Value::Number(serde_json::Number::from(value))
}

fn number_f64(value: f64) -> Value {
    serde_json::Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or_else(|| Value::String(format!("{value}")))
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn unhex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks(2)
            .map(|pair| {
                let digits: String = pair.iter().map(|byte| *byte as char).collect();
                u8::from_str_radix(&digits, 16).expect("hex")
            })
            .collect()
    }

    #[test]
    fn integers_are_stored_unpickled() {
        assert_eq!(decode_string(b"42"), RedisValue::Integer(42));
        assert_eq!(decode_string(b"-7"), RedisValue::Integer(-7));
    }

    #[test]
    fn throttle_histories_decode_to_float_arrays() {
        // Python: pickle.dumps([1789000000.123456, 1788999940.0], 4)
        let frame = unhex("80049517000000000000005d94284741daa87e5007e6b44741daa87e41000000652e");
        assert_eq!(
            decode_string(&frame),
            RedisValue::Pickled(json!([1789000000.123456, 1788999940.0]))
        );
        // The empty history is a valid frame too.
        assert_eq!(
            decode_string(&unhex("80045d942e")),
            RedisValue::Pickled(json!([]))
        );
    }

    #[test]
    fn cache_dictionaries_decode_to_objects() {
        // Python: pickle.dumps({"data": {"a": 1, "b": [True, None, "x"]},
        //                       "status": 200}, 4)
        let frame = unhex(
            "8004952f000000000000007d94288c0464617461947d94288c0161944b018c0162945d9428884e8c01789465758c06737461747573944bc8752e",
        );
        assert_eq!(
            decode_string(&frame),
            RedisValue::Pickled(json!({
                "data": {"a": 1, "b": [true, null, "x"]},
                "status": 200
            }))
        );
    }

    #[test]
    fn non_string_dictionary_keys_are_kept_explicitly() {
        // Python: pickle.dumps({1: "one", "two": 2.5}, 4)
        let frame =
            unhex("8004951c000000000000007d94284b018c036f6e65948c0374776f94474004000000000000752e");
        assert_eq!(
            decode_string(&frame),
            RedisValue::Pickled(json!({
                "$dict": [["1", "one"], ["two", 2.5]]
            }))
        );
    }

    #[test]
    fn bytes_values_get_a_marker() {
        // Python: pickle.dumps(b"bytes", 4)
        let frame = unhex("800495090000000000000043056279746573942e");
        assert_eq!(
            decode_string(&frame),
            RedisValue::Pickled(json!({"$bytes": "6279746573"}))
        );
        assert_eq!(
            decode_string(b"\xff\xfe"),
            RedisValue::Raw(json!({"$bytes": "fffe"}))
        );
    }

    #[test]
    fn plain_utf8_is_kept_verbatim() {
        assert_eq!(decode_string(b"hello"), RedisValue::Raw(json!("hello")));
        assert_eq!(decode_string(b""), RedisValue::Raw(json!("")));
    }

    #[test]
    fn broken_frames_are_flagged_not_guessed() {
        assert_eq!(decode_string(b"\x80\x04\x95"), RedisValue::Unreadable);
        assert_eq!(
            decode_string(b"\x80\x04\x95\xff\xff\xff\xff\xff\xff\xff\xff\xff"),
            RedisValue::Unreadable
        );
    }
}
