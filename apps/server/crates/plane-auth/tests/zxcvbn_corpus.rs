// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `tools/auth/zxcvbn_corpus.json`: 742 passwords scored by the real
//! `zxcvbn==4.4.28` through `plane.authentication.utils.password.
//! is_password_strong`, recorded by `tools/auth/record_corpora.py`.
//!
//! The crate is vendored at `apps/server/vendor/zxcvbn/` with two changes
//! (`VENDORED.md`): the input cap is raised from 100 to 128 characters, the
//! window `is_password_strong` scores, and one-character l33t matches are
//! dropped as Python drops them. This test requires the crate's **score** to
//! equal Python's on every first/last window — 1 484 windows, 654 of them
//! longer than 100 characters, across repeats, dictionary words, keyboard
//! walks, dates, l33t, sequences, Unicode and random input — and the policy
//! answer for every password, with **0** differences.
//!
//! The recorded scores include the ones that disagreed before the vendor
//! patch: 33 long windows scored differently with the 100-character cap, and
//! `"1" * 128` scored 0 instead of 1 because of the l33t bug.

use plane_auth::auth_flow::is_password_strong;
use serde::Deserialize;

const CORPUS: &str = include_str!("../../../tools/auth/zxcvbn_corpus.json");

#[derive(Deserialize)]
struct Corpus {
    zxcvbn: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    password: String,
    first: String,
    last: String,
    first_score: u8,
    last_score: u8,
    strong: bool,
}

fn score(password: &str) -> u8 {
    zxcvbn::zxcvbn(password, &[]).score() as u8
}

/// The crate's score for both windows of every case, computed once: scoring is
/// the expensive part of this file (zxcvbn is super-linear on random input).
fn scores() -> &'static [(u8, u8)] {
    use std::sync::OnceLock;
    static SCORES: OnceLock<Vec<(u8, u8)>> = OnceLock::new();
    SCORES.get_or_init(|| {
        let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
        corpus
            .cases
            .iter()
            .map(|case| (score(&case.first), score(&case.last)))
            .collect()
    })
}

#[test]
fn the_corpus_is_from_the_pinned_python_zxcvbn() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    assert_eq!(corpus.zxcvbn, "4.4.28");
    assert_eq!(corpus.cases.len(), 742);
}

#[test]
fn every_window_score_matches_python() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    let mut short = 0;
    let mut long = 0;
    let mut differences = Vec::new();
    for (case, (first, last)) in corpus.cases.iter().zip(scores()) {
        for (window, expected, actual) in [
            (&case.first, case.first_score, *first),
            (&case.last, case.last_score, *last),
        ] {
            if window.chars().count() > 100 {
                long += 1;
            } else {
                short += 1;
            }
            if actual != expected {
                differences.push((window.clone(), expected, actual, case.password.clone()));
            }
        }
    }
    assert_eq!(short, 830, "the short-window population changed");
    assert_eq!(long, 654, "the long-window population changed");
    assert!(
        differences.is_empty(),
        "{} score differences, first: {:?}",
        differences.len(),
        differences.first()
    );
}

#[test]
fn is_password_strong_matches_python_on_every_password() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    let mut differences = Vec::new();
    for case in &corpus.cases {
        let actual = is_password_strong(&case.password);
        if actual != case.strong {
            differences.push((case.password.clone(), case.strong, actual));
        }
    }
    assert!(
        differences.is_empty(),
        "{} policy differences, first: {:?}",
        differences.len(),
        differences.first()
    );
}
