// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Compare [`plane_tasks::bs4::serialize`] with bs4 4.12.3, case by case.
//!
//! The corpus is recorded from `apps/api/.venv` by
//! `tools/inventory/bs4_corpus.py`; `--check` keeps it fresh. Every case
//! compares the whole serialised document, so attribute order, quoting,
//! void-tag rendering, entity handling and malformed-input recovery are all
//! pinned at once.

use serde_json::Value;

const CORPUS: &str = include_str!("golden/bs4_html.json");

#[test]
fn every_corpus_case_matches_bs4() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("the corpus is JSON");
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 45, "the corpus lost cases");
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let html = case["html"].as_str().expect("html");
        let expected = case["output"].as_str().expect("output");
        assert_eq!(
            plane_tasks::bs4::serialize(html),
            expected,
            "case {name}: {html:?}"
        );
        let stripped = case["stripped"].as_str().expect("stripped");
        assert_eq!(
            plane_tasks::bs4::strip_tags(html),
            stripped,
            "strip_tags case {name}: {html:?}"
        );
    }
}
