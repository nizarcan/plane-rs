// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `lxml` corpus: gate verdicts, what the stored value must become.
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
//! `lxml` is what `plane/api/serializers/issue.py:102-109` puts in
//! `description_html`; `stored` is the row after `validate_html_content`
//! sanitises it. Both are asserted: the write path is only byte-identical when
//! the normaliser and the sanitizer agree with the two Python libraries.

use plane_http_app::write::html::sanitize;
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

#[test]
fn the_normaliser_matches_lxml_on_every_case() {
    let mut failures = Vec::new();
    for record in records() {
        let expected = record.lxml.as_deref();
        match (html::normalize(&record.input), expected) {
            (Err(HtmlNormalizeError::Empty), None) => {}
            (Ok(actual), Some(expected)) if actual == expected => {}
            (Ok(actual), Some(expected)) => failures.push(format!(
                "{:?}: normalised to {actual:?}, lxml produced {expected:?}",
                record.input
            )),
            (Ok(actual), None) => failures.push(format!(
                "{:?}: normalised to {actual:?}, lxml refused it",
                record.input
            )),
            (Err(error), Some(expected)) => failures.push(format!(
                "{:?}: {error}, lxml produced {expected:?}",
                record.input
            )),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The sanitised value is the row Plane stores: `nh3.clean(lxml_normalized)`
/// with the allow-list of `plane/utils/content_validator.py`.
#[test]
fn the_sanitised_value_matches_nh3_on_every_case() {
    let mut failures = Vec::new();
    for record in records() {
        let Some(expected) = record.stored.as_deref() else {
            continue;
        };
        let Ok(normalised) = html::normalize(&record.input) else {
            failures.push(format!("{:?}: the normaliser refused it", record.input));
            continue;
        };
        let actual = sanitize(&normalised);
        if actual != expected {
            failures.push(format!(
                "{:?}: sanitised to {actual:?}, nh3 produced {expected:?}",
                record.input
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A wider corpus recorded by `tools/inventory/lxml_html_extra_corpus.py`:
/// every construct the editor emits, the malformed shapes clients send, and
/// combinations that exercise libxml2's implied-end-tag table. The same
/// `lxml` / libxml2 pins as `golden/lxml_html.jsonl`.
#[test]
fn the_normaliser_matches_the_extra_corpus() {
    #[derive(serde::Deserialize)]
    struct Extra {
        input: String,
        #[serde(default)]
        lxml: Option<String>,
    }
    let corpus = include_str!("golden/lxml_html_extra.jsonl");
    assert!(corpus.lines().count() >= 700, "corpus case count");
    let mut failures = Vec::new();
    for line in corpus.lines().filter(|line| !line.trim().is_empty()) {
        let record: Extra = serde_json::from_str(line).expect("corpus line is a record");
        match (html::normalize(&record.input), record.lxml.as_deref()) {
            (Err(HtmlNormalizeError::Empty), None) => {}
            (Ok(actual), Some(expected)) if actual == expected => {}
            (Ok(actual), Some(expected)) => failures.push(format!(
                "{:?}: normalised to {actual:?}, lxml produced {expected:?}",
                record.input
            )),
            (Ok(actual), None) => failures.push(format!(
                "{:?}: normalised to {actual:?}, lxml refused it",
                record.input
            )),
            (Err(error), Some(expected)) => failures.push(format!(
                "{:?}: {error}, lxml produced {expected:?}",
                record.input
            )),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
