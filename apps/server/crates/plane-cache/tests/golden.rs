// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The pickle codec against CPython's bytes.
//!
//! `tools/cache/pickle_corpus.py` records, for every case, the
//! identity-annotated value (`tree`), the value `pickle.loads` gives back
//! (`value`) and `binascii.hexlify(pickle.dumps(value, 4))`. This test
//!
//! 1. builds `tree` (sharing the `Arc`s the `{"r": n}` references name),
//! 2. encodes it and compares every byte with `hex`,
//! 3. decodes `hex` and compares the result with `value`.
//!
//! Regenerate with (from `apps/api`):
//!
//! ```sh
//! PYTHONPATH=. .venv/bin/python ../server/tools/cache/pickle_corpus.py \
//!     --out ../server/crates/plane-cache/tests/golden/pickle-corpus.json
//! ```

mod common;

use common::{build, same, unhex};
use plane_cache::pickle;
use serde_json::Value as Json;

const CORPUS: &str = include_str!("golden/pickle-corpus.json");

#[test]
fn every_case_encodes_and_decodes_like_cpython() {
    let document: Json = serde_json::from_str(CORPUS).expect("corpus is valid JSON");
    assert_eq!(
        document["pickle_default_protocol"].as_u64(),
        Some(4),
        "the corpus must have been recorded with protocol 4"
    );
    let cases = document["cases"].as_array().expect("cases array");
    assert!(
        cases.len() >= 500,
        "the corpus must hold at least 500 cases, has {}",
        cases.len()
    );

    let mut encoded_mismatches = Vec::new();
    let mut decoded_mismatches = Vec::new();
    for case in cases {
        let name = case["name"].as_str().expect("name");
        let tree = build(case["tree"].clone(), &mut Vec::new());
        let expected_value = build(case["value"].clone(), &mut Vec::new());
        let expected_bytes = unhex(case["hex"].as_str().expect("hex"));

        let actual = pickle::dumps(&tree);
        if actual != expected_bytes {
            encoded_mismatches.push(format!(
                "{name}: expected {}\n              actual   {}",
                plane_cache::hex(&expected_bytes),
                plane_cache::hex(&actual)
            ));
            continue;
        }
        match pickle::loads(&expected_bytes) {
            Ok(decoded) => {
                if !same(&decoded, &expected_value) {
                    decoded_mismatches.push(format!("{name}: {decoded:?} != {expected_value:?}"));
                }
            }
            Err(error) => decoded_mismatches.push(format!("{name}: {error}")),
        }
    }

    assert!(
        encoded_mismatches.is_empty(),
        "{} of {} cases encoded differently:\n{}",
        encoded_mismatches.len(),
        cases.len(),
        encoded_mismatches.join("\n")
    );
    assert!(
        decoded_mismatches.is_empty(),
        "{} of {} cases decoded differently:\n{}",
        decoded_mismatches.len(),
        cases.len(),
        decoded_mismatches.join("\n")
    );
}

#[test]
fn corpus_covers_every_value_shape() {
    let document: Json = serde_json::from_str(CORPUS).expect("corpus is valid JSON");
    let text = document.to_string();
    for tag in [
        "\"i\"", "\"f\"", "\"s\"", "\"s8\"", "\"b\"", "\"l\"", "\"t\"", "\"d\"", "\"e\"", "\"z\"",
        "\"u\"", "\"D\"", "\"T\"", "\"H\"", "\"X\"", "\"Z\"", "\"M\"", "\"L\"", "\"r\"",
    ] {
        assert!(text.contains(tag), "the corpus has no {tag} node");
    }
}
