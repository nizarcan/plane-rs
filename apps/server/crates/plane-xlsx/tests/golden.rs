// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Compare this crate's workbooks with openpyxl 3.1.2's, entry by entry.
//!
//! The corpus is recorded by `tools/inventory/xlsx_corpus.py` from the pinned
//! `apps/api/.venv`; every entry's text is compared after replacing the
//! `docProps/core.xml` timestamps (the second timestamp family besides the ZIP
//! DOS times, which never enter the decompressed bytes).
//!
//! Regenerate with:
//!
//! ```text
//! apps/api/.venv/bin/python apps/server/tools/inventory/xlsx_corpus.py
//! ```

use std::io::Read as _;

use chrono::{TimeZone as _, Utc};
use flate2::read::DeflateDecoder;
use plane_xlsx::{Cell, workbook};
use serde_json::Value;

const FIXED: &str = "2026-01-05T01:02:03Z";
const CORPUS: &str = include_str!("golden/workbook.json");

fn now() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 5, 1, 2, 3)
        .single()
        .expect("a fixed instant")
}

fn rows(case: &Value) -> Vec<Vec<Cell>> {
    case["rows"]
        .as_array()
        .expect("rows")
        .iter()
        .map(|row| {
            row.as_array()
                .expect("row")
                .iter()
                .map(|cell| {
                    let kind = cell[0].as_str().expect("kind");
                    match kind {
                        "s" => Cell::Text(cell[1].as_str().expect("text").to_string()),
                        "i" => Cell::Int(cell[1].as_i64().expect("int")),
                        "f" => Cell::Float(cell[1].as_f64().expect("float")),
                        "b" => Cell::Bool(cell[1].as_bool().expect("bool")),
                        other => panic!("unknown cell kind {other}"),
                    }
                })
                .collect()
        })
        .collect()
}

/// Read a ZIP by walking its local headers in order. The writer emits every
/// entry in order and never uses a data descriptor, so this is enough.
fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::new();
    let mut index = 0usize;
    while index + 4 <= bytes.len() {
        let signature = u32::from_le_bytes(bytes[index..index + 4].try_into().expect("signature"));
        if signature != 0x0403_4b50 {
            break;
        }
        let method = u16::from_le_bytes(bytes[index + 8..index + 10].try_into().expect("method"));
        let compressed =
            u32::from_le_bytes(bytes[index + 18..index + 22].try_into().expect("csize")) as usize;
        let name_length =
            u16::from_le_bytes(bytes[index + 26..index + 28].try_into().expect("namelen")) as usize;
        let extra_length =
            u16::from_le_bytes(bytes[index + 28..index + 30].try_into().expect("extralen"))
                as usize;
        let name_start = index + 30;
        let name = String::from_utf8(bytes[name_start..name_start + name_length].to_vec())
            .expect("entry name");
        let data_start = name_start + name_length + extra_length;
        let data = &bytes[data_start..data_start + compressed];
        let body = if method == 8 {
            let mut out = Vec::new();
            DeflateDecoder::new(data)
                .read_to_end(&mut out)
                .expect("inflate");
            out
        } else {
            data.to_vec()
        };
        entries.push((name, body));
        index = data_start + compressed;
    }
    entries
}

#[test]
fn every_case_matches_openpyxl_312() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus json");
    assert_eq!(corpus["openpyxl"], "3.1.2", "the corpus pins openpyxl");
    let cases = corpus["cases"].as_object().expect("cases");
    assert!(!cases.is_empty());
    for (name, case) in cases {
        let rows = rows(case);
        let bytes = workbook(&rows, now()).unwrap_or_else(|error| panic!("{name}: {error}"));
        let actual = unzip(&bytes);
        let expected = case["entries"].as_array().expect("entries");
        let actual_names: Vec<&str> = actual.iter().map(|(name, _)| name.as_str()).collect();
        let expected_names: Vec<&str> = expected
            .iter()
            .map(|entry| entry["name"].as_str().expect("name"))
            .collect();
        assert_eq!(actual_names, expected_names, "{name}: part set and order");
        for ((part, body), expected) in actual.iter().zip(expected) {
            let text = String::from_utf8(body.clone()).expect("utf-8 part");
            let text = text.replace(FIXED, "<TS>");
            assert_eq!(
                text,
                expected["text"].as_str().expect("text"),
                "{name}: {part}"
            );
        }
    }
}
