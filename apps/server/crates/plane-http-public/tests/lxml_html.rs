// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `lxml` corpus: gate verdicts, and what the stored value must become.
//!
//! `tests/golden/lxml_html.jsonl` is recorded by
//! `tools/inventory/lxml_html_corpus.py` from the real `lxml` in
//! `apps/api/.venv` plus `nh3.clean` with Plane's allow-list
//! (`plane/utils/content_validator.py:206-243`). One record per input:
//!
//! ```json
//! {"input": "<p>a</p><p>b</p>", "outcome": "ok",
//!  "lxml": "<div><p>a</p><p>b</p></div>", "stored": "<div>..."}
//! ```
//!
//! This test covers the half that is ported — the gate — and asserts that the
//! unported half keeps saying so. When set D ports the libxml2 serialiser,
//! `normalize` starts returning `Ok` and the `lxml`/`stored` fields become
//! assertions here (`--check` keeps the file fresh).

use plane_http_public::common::html::{self, HtmlNormalizeError, InvalidHtml};

#[derive(Debug, serde::Deserialize)]
struct Record {
    input: String,
    outcome: String,
    #[serde(default)]
    lxml: Option<String>,
    #[serde(default)]
    stored: Option<String>,
}

fn records() -> Vec<Record> {
    let corpus = include_str!("golden/lxml_html.jsonl");
    corpus
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("corpus line is a record"))
        .collect()
}

#[test]
fn the_gate_agrees_with_lxml_on_every_case() {
    let records = records();
    assert_eq!(records.len(), 70, "corpus case count");
    let mut failures = Vec::new();
    for record in &records {
        let expected_ok = record.outcome == "ok";
        match (html::check(&record.input), expected_ok) {
            (Ok(()), true) | (Err(InvalidHtml), false) => {}
            (Ok(()), false) => failures.push(format!(
                "{:?}: the gate accepts it, lxml refused it",
                record.input
            )),
            (Err(_), true) => failures.push(format!(
                "{:?}: the gate refuses it, lxml produced {:?}",
                record.input, record.lxml
            )),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The normaliser is not ported: an accepted input must be an explicit
/// failure, never the input passed through (which would store different bytes
/// than Django).
#[test]
fn the_unported_normaliser_is_an_explicit_error() {
    for record in records() {
        match html::normalize(&record.input) {
            Err(HtmlNormalizeError::Empty) => assert_eq!(
                record.outcome, "error",
                "{:?} is accepted by lxml",
                record.input
            ),
            Err(HtmlNormalizeError::Unsupported) => {
                assert_eq!(record.outcome, "ok", "{:?}", record.input);
                assert!(record.lxml.is_some());
                assert!(record.stored.is_some());
            }
            Ok(value) => panic!(
                "normalize({:?}) returned {value:?}; set D must assert it against {:?}",
                record.input, record.lxml
            ),
        }
    }
}
