// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `plane_core::csv` against documents recorded from CPython's `csv.writer`.
//!
//! The corpus is produced by `tools/inventory/csv_corpus.py`, which pushes each
//! case through the real chain the view uses (`sanitize_csv_row` and then
//! `csv.writer(..., quoting=csv.QUOTE_ALL)`), so a divergence in quoting,
//! doubled quotes, the `\r\n` terminator, `None`-as-`""` or the formula-prefix
//! rule shows up as a whole-document mismatch.
//!
//! Regenerate with
//! `apps/api/.venv/bin/python apps/server/tools/inventory/csv_corpus.py`.

use plane_core::csv::{sanitize_csv_row, write_row};

const CORPUS: &str = include_str!("golden/csv-writer.json");

/// One corpus case: rows of `str`/`null` and the CSV document Python produced.
struct Case {
    name: String,
    rows: Vec<Vec<Option<String>>>,
    csv: String,
}

fn cases() -> Vec<Case> {
    let document: serde_json::Value =
        serde_json::from_str(CORPUS).expect("the golden corpus is valid JSON");
    assert_eq!(
        document["python"], "3.12.13",
        "the corpus must come from the pinned interpreter (DECISIONS.md D9)"
    );
    document["cases"]
        .as_array()
        .expect("cases is an array")
        .iter()
        .map(|case| Case {
            name: case["name"].as_str().expect("case name").to_owned(),
            rows: case["rows"]
                .as_array()
                .expect("rows is an array")
                .iter()
                .map(|row| {
                    row.as_array()
                        .expect("row is an array")
                        .iter()
                        .map(|value| value.as_str().map(str::to_owned))
                        .collect()
                })
                .collect(),
            csv: case["csv"].as_str().expect("csv is a string").to_owned(),
        })
        .collect()
}

#[test]
fn every_case_matches_pythons_csv_writer() {
    let cases = cases();
    assert!(cases.len() >= 9, "the corpus lost cases");
    for case in cases {
        let sanitized: Vec<Vec<Option<std::borrow::Cow<'_, str>>>> = case
            .rows
            .iter()
            .map(|row| sanitize_csv_row(row.iter().map(|value| value.as_deref())))
            .collect();
        let mut document = String::new();
        for row in &sanitized {
            write_row(&mut document, row.iter().map(|value| value.as_deref()));
        }
        assert_eq!(document, case.csv, "case {:?} differs", case.name);
    }
}

#[test]
fn the_header_is_the_views_header() {
    // `plane/app/views/workspace/base.py:363-373`.
    let header = [
        "Actor name",
        "Issue ID",
        "Project",
        "Created at",
        "Updated at",
        "Action",
        "Field",
        "Old value",
        "New value",
    ];
    let mut document = String::new();
    write_row(&mut document, header.map(Some));
    assert_eq!(
        document,
        "\"Actor name\",\"Issue ID\",\"Project\",\"Created at\",\
         \"Updated at\",\"Action\",\"Field\",\"Old value\",\"New value\"\r\n"
    );
}
