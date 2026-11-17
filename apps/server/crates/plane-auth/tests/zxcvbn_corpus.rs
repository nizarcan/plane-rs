// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `tools/auth/zxcvbn_corpus.json`: 546 passwords scored by the real
//! `zxcvbn==4.4.28` through `plane.authentication.utils.password.
//! is_password_strong`, recorded by `tools/auth/record_corpora.py`.
//!
//! The Rust `zxcvbn` 3.1.1 crate truncates its input to the first 100
//! characters (`zxcvbn-3.1.1/src/lib.rs:156-158`); Plane's windows are 128, so
//! the crate is only proven for windows ≤ 100 characters. This test pins that
//! split — **0/830** differences at ≤ 100 and **33/262** above — so a crate
//! upgrade that changes either side fails loudly and the decision in
//! `auth_flow/password.rs` is revisited.
//!
//! The corpora include random ASCII, Unicode, emoji, >128-character strings,
//! dictionary words and the fixture passwords.

use plane_auth::auth_flow::{PasswordStrengthError, is_password_strong};
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
/// the expensive part of this file (~20 s for 1 092 windows).
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
    assert!(corpus.cases.len() >= 500);
}

#[test]
fn the_crate_agrees_on_every_window_at_most_100_characters() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    let mut checked = 0;
    let scores = scores();
    for (case, (first, last)) in corpus.cases.iter().zip(scores) {
        for (window, expected, actual) in [
            (&case.first, case.first_score, *first),
            (&case.last, case.last_score, *last),
        ] {
            if window.chars().count() > 100 {
                continue;
            }
            assert_eq!(
                actual, expected,
                "zxcvbn({:?}) (password {:?})",
                window, case.password
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 830, "the short-window population changed");
}

#[test]
fn the_crate_differs_exactly_on_the_windows_it_truncates() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    let mut first_differences = 0;
    let mut last_differences = 0;
    let mut strong_differences = 0;
    let mut long_windows = 0;
    let scores = scores();
    for (case, (first, last)) in corpus.cases.iter().zip(scores) {
        let first = *first;
        let last = *last;
        if case.first.chars().count() > 100 {
            long_windows += 1;
            if first != case.first_score {
                first_differences += 1;
            }
        } else {
            assert_eq!(first, case.first_score, "short first window changed");
        }
        if case.last.chars().count() > 100 {
            long_windows += 1;
            if last != case.last_score {
                last_differences += 1;
            }
        } else {
            assert_eq!(last, case.last_score, "short last window changed");
        }
        if (first >= 3 || last >= 3) != case.strong {
            strong_differences += 1;
        }
    }
    assert_eq!(long_windows, 262, "the long-window population changed");
    assert_eq!(first_differences, 16);
    assert_eq!(last_differences, 17);
    assert_eq!(strong_differences, 3);
}

#[test]
fn is_password_strong_matches_python_where_the_crate_is_proven() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    let mut answered = 0;
    for case in &corpus.cases {
        let length = case.first.chars().count().max(case.last.chars().count());
        if length <= 100 {
            assert_eq!(
                is_password_strong(&case.password),
                Ok(case.strong),
                "is_password_strong({:?})",
                case.password
            );
            answered += 1;
        } else {
            // A 500 (the deliberate not-implemented path), never a wrong
            // password policy.
            assert_eq!(
                is_password_strong(&case.password),
                Err(PasswordStrengthError::NotPorted { length })
            );
        }
    }
    assert!(answered >= 400);
}
