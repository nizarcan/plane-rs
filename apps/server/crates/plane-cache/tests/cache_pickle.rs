// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The two cached workspace endpoints, byte for byte against Django.
//!
//! `tools/golden/serializers_cache_pickle.py` recorded, from the rich fixture
//! clone (`plane_fix_tpl`), both the Redis pickle and a tagged view of
//! `serializer.data` with CPython's string sharing marked:
//!
//! * `GET /api/workspaces/{slug}/estimates/` — `WorkspaceEstimateSerializer`,
//!   two estimates and thirteen points, 3041 bytes;
//! * `GET /api/workspaces/{slug}/labels/` — `LabelSerializer`, ten labels,
//!   1495 bytes.
//!
//! This test rebuilds each tree through [`PyInterner`] and asserts the pickled
//! bytes are identical. That is the property the differential harness cannot
//! check above 4 KiB (it compares type and size only), and the reason the local
//! `Interner` copies existed: an over-shared value (two equal database strings
//! merged) or an under-shared one (a dict key emitted once per row) changes the
//! memo indices and the frame length.
//!
//! The recorded `s` flag is CPython's own answer
//! (`sys.intern(value) is value`), so this also pins the interner's policy:
//! keys and `ChoiceField` values are shared; database strings are fresh except
//! the empty string and single Latin-1 characters.

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use plane_cache::pickle::{dumps, loads};
use plane_cache::{PyInterner, PyValue};

const CORPUS: &str = include_str!("golden/cache_pickle.jsonl");

#[derive(Debug, Deserialize)]
struct Record {
    case: String,
    path: String,
    #[serde(default)]
    user_id: String,
    status: u16,
    pickle_hex: String,
    payload: serde_json::Value,
}

#[test]
fn cached_workspace_payloads_are_byte_identical_to_django() {
    let records: Vec<Record> = CORPUS
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("golden record"))
        .collect();
    assert_eq!(records.len(), 2, "one record per cached endpoint");

    let mut failures = Vec::new();
    for record in &records {
        let mut strings = PyInterner::new();
        let data = build(&record.payload, &mut strings);
        // `plane/utils/cache.py:25-51` stores the dict literal
        // `{"data": response.data, "status": response.status_code}`, whose keys
        // are interned literals.
        let data_key = strings.key("data");
        let status_key = strings.key("status");
        let value = PyValue::mapping(vec![
            (data_key, data),
            (status_key, PyValue::int(i64::from(record.status))),
        ]);
        let actual = dumps(&value);
        let expected = match decode_hex(&record.pickle_hex) {
            Ok(bytes) => bytes,
            Err(error) => {
                failures.push(format!("{}: bad golden hex: {error}", record.case));
                continue;
            }
        };
        if actual != expected {
            failures.push(format!(
                "{} ({} for user {}):\n  expected {} bytes\n  actual   {} bytes\n  first difference at {:?}",
                record.case,
                record.path,
                record.user_id,
                expected.len(),
                actual.len(),
                first_difference(&expected, &actual)
            ));
            continue;
        }
        // The bytes are the contract; decoding both sides is a cheap extra
        // check that a mis-encoded opcode still produced the right length.
        assert_eq!(
            loads(&actual).expect("decode built pickle"),
            loads(&expected).expect("decode golden pickle"),
            "{}: decoded trees differ",
            record.case
        );
    }
    assert!(
        failures.is_empty(),
        "{} of {} cached payloads differ:\n{}",
        failures.len(),
        records.len(),
        failures.join("\n")
    );
}

fn build(value: &Value, strings: &mut PyInterner) -> PyValue {
    let kind = value["t"].as_str().expect("tagged node");
    match kind {
        "none" => PyValue::none(),
        "bool" => PyValue::bool(value["v"].as_bool().expect("bool")),
        "int" => PyValue::int(value["v"].as_str().expect("int text").parse().expect("int")),
        "float" => PyValue::float(
            value["v"]
                .as_str()
                .expect("float repr")
                .parse()
                .expect("float"),
        ),
        "str" => {
            let text = value["v"].as_str().expect("str");
            if value["s"].as_bool().unwrap_or(false) {
                strings.choice(text)
            } else {
                strings.value(text)
            }
        }
        "uuid" => PyValue::uuid(Uuid::parse_str(value["v"].as_str().expect("uuid")).expect("uuid")),
        "list" => PyValue::list(
            value["v"]
                .as_array()
                .expect("list")
                .iter()
                .map(|item| build(item, strings))
                .collect(),
        ),
        "return_list" => PyValue::return_list(
            value["v"]
                .as_array()
                .expect("return_list")
                .iter()
                .map(|item| build(item, strings))
                .collect(),
        ),
        "return_dict" => {
            let entries = value["v"]
                .as_array()
                .expect("return_dict")
                .iter()
                .map(|entry| {
                    let pair = entry.as_array().expect("pair");
                    let key = pair[0]["v"].as_str().expect("dict key is a str");
                    (strings.key(key), build(&pair[1], strings))
                })
                .collect();
            PyValue::return_mapping(entries)
        }
        "dict" => {
            let entries = value["v"]
                .as_array()
                .expect("dict")
                .iter()
                .map(|entry| {
                    let pair = entry.as_array().expect("pair");
                    let key = pair[0]["v"].as_str().expect("dict key is a str");
                    (strings.key(key), build(&pair[1], strings))
                })
                .collect();
            PyValue::mapping(entries)
        }
        other => panic!("unmodelled golden node {other:?}"),
    }
}

fn decode_hex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) {
        return Err("odd length".to_owned());
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Ok(byte - b'0'),
            b'a'..=b'f' => Ok(byte - b'a' + 10),
            b'A'..=b'F' => Ok(byte - b'A' + 10),
            _ => Err(format!("not a hex digit: {}", char::from(byte))),
        };
        out.push(digit(pair[0])? << 4 | digit(pair[1])?);
    }
    Ok(out)
}

fn first_difference(expected: &[u8], actual: &[u8]) -> Option<(usize, u8, u8)> {
    expected
        .iter()
        .zip(actual)
        .position(|(expected, actual)| expected != actual)
        .map(|index| (index, expected[index], actual[index]))
        .or_else(|| {
            (expected.len() != actual.len()).then(|| {
                let index = expected.len().min(actual.len());
                (
                    index,
                    expected.get(index).copied().unwrap_or(0),
                    actual.get(index).copied().unwrap_or(0),
                )
            })
        })
}
