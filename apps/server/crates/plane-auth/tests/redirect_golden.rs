// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `tests/golden/redirect.json`, recorded from the real
//! `plane/utils/path_validator.py`, Django 5.2.17's
//! `url_has_allowed_host_and_scheme` and CPython 3.12.13's
//! `urllib.parse.urljoin` by `tools/auth/record_corpora.py`.
//!
//! Regenerate from `apps/api` with
//! `.venv/bin/python ../server/tools/auth/record_corpora.py`.

use plane_auth::auth_flow::{
    allowed_hosts, get_safe_redirect_url, url_has_allowed_host_and_scheme,
    url_has_allowed_host_and_scheme_https, urlencode, urljoin, validate_next_path,
};
use serde::Deserialize;

const CORPUS: &str = include_str!("golden/redirect.json");

#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
    url_checks: Vec<UrlCheck>,
    joins: Vec<Join>,
}

#[derive(Deserialize)]
struct Case {
    web_url: Option<String>,
    app_base_url: Option<String>,
    admin_base_url: Option<String>,
    space_base_url: Option<String>,
    base_url: String,
    next_path: Option<String>,
    /// `[[key, value], …]` — a list of pairs so the Rust side keeps the order
    /// `urlencode` sees.
    params: Vec<(String, String)>,
    allowed_hosts: Outcome<Vec<String>>,
    validated: Outcome<String>,
    redirect: Outcome<String>,
    space_url: String,
    space_allowed: Option<bool>,
}

#[derive(Deserialize)]
struct UrlCheck {
    url: String,
    allowed_hosts: Vec<String>,
    allow: bool,
    allow_require_https: bool,
}

#[derive(Deserialize)]
struct Join {
    base: String,
    relative: String,
    result: Outcome<String>,
}

/// Python's `{"value": …}` / `{"exception": …}` split.
#[derive(Deserialize)]
#[serde(untagged)]
enum Outcome<T> {
    Value { value: T },
    Exception { exception: String },
}

#[test]
fn the_recorded_corpus_matches() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    assert!(
        corpus.cases.len() >= 5000,
        "the corpus shrank: {}",
        corpus.cases.len()
    );
    assert!(corpus.url_checks.len() >= 200);
    assert!(corpus.joins.len() >= 200);

    let mut validated_ok = 0;
    let mut redirect_ok = 0;
    for case in &corpus.cases {
        let next = case.next_path.as_deref();
        let validated = validate_next_path(next);
        match &case.validated {
            Outcome::Value { value } => {
                assert_eq!(
                    validated.as_ref().ok(),
                    Some(value),
                    "validate_next_path({:?})",
                    case.next_path
                );
                validated_ok += 1;
            }
            Outcome::Exception { exception } => {
                let error = validated.as_ref().expect_err(&format!(
                    "validate_next_path({:?}) should have raised {exception}",
                    case.next_path
                ));
                assert_eq!(format!("ValueError: {}", error.message()), *exception);
            }
        }

        let hosts = allowed_hosts(
            case.web_url.as_deref(),
            case.app_base_url.as_deref(),
            case.admin_base_url.as_deref(),
            case.space_base_url.as_deref(),
        );
        match &case.allowed_hosts {
            Outcome::Value { value } => {
                assert_eq!(
                    hosts.as_ref().ok(),
                    Some(value),
                    "get_allowed_hosts({:?}, {:?}, {:?}, {:?})",
                    case.web_url,
                    case.app_base_url,
                    case.admin_base_url,
                    case.space_base_url
                );
            }
            Outcome::Exception { exception } => {
                let error = hosts
                    .as_ref()
                    .expect_err("allowed hosts should have raised");
                assert_eq!(format!("ValueError: {}", error.message()), *exception);
            }
        }

        match &case.redirect {
            Outcome::Value { value } => {
                let hosts = hosts
                    .as_ref()
                    .expect("a valid allow-list for a successful redirect");
                let redirect = get_safe_redirect_url(&case.base_url, next, &case.params, hosts)
                    .expect(
                        "a successful Python redirect means validate_next_path and \
                         get_allowed_hosts both succeeded",
                    );
                assert_eq!(
                    &redirect, value,
                    "get_safe_redirect_url({:?}, {:?}, {:?})",
                    case.base_url, case.next_path, case.params
                );
                redirect_ok += 1;
            }
            Outcome::Exception { exception } => {
                // Python's error is whichever call raised first: the path
                // validation, or (after building the URL) `get_allowed_hosts`.
                let our_error = match &case.validated {
                    Outcome::Value { .. } => hosts
                        .as_ref()
                        .expect_err("the recorded redirect raised, so one of its calls did"),
                    Outcome::Exception { .. } => validated.as_ref().expect_err("the path raised"),
                };
                assert_eq!(format!("ValueError: {}", our_error.message()), *exception);
            }
        }

        if let Some(expected) = case.space_allowed {
            let hosts = hosts
                .as_ref()
                .expect("space_allowed is set only when the hosts parsed");
            assert_eq!(
                url_has_allowed_host_and_scheme(&case.space_url, hosts),
                expected,
                "url_has_allowed_host_and_scheme({:?})",
                case.space_url
            );
        }
    }

    // Both branches must be exercised, or a constant would pass.
    assert!(validated_ok > 100 && validated_ok < corpus.cases.len() - 100);
    assert!(redirect_ok > 100);
}

#[test]
fn the_url_check_corpus_matches() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    let mut allowed = 0;
    let mut denied = 0;
    for case in &corpus.url_checks {
        let result = url_has_allowed_host_and_scheme(&case.url, &case.allowed_hosts);
        assert_eq!(
            result, case.allow,
            "url_has_allowed_host_and_scheme({:?}, {:?})",
            case.url, case.allowed_hosts
        );
        let result = url_has_allowed_host_and_scheme_https(&case.url, &case.allowed_hosts, true);
        assert_eq!(
            result, case.allow_require_https,
            "url_has_allowed_host_and_scheme({:?}, {:?}, require_https=True)",
            case.url, case.allowed_hosts
        );
        if case.allow {
            allowed += 1;
        } else {
            denied += 1;
        }
    }
    assert!(allowed > 30 && denied > 30);
}

#[test]
fn the_urljoin_corpus_matches() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    let mut ok = 0;
    for case in &corpus.joins {
        match &case.result {
            Outcome::Value { value } => {
                let joined = urljoin(&case.base, &case.relative).expect("join");
                assert_eq!(
                    &joined, value,
                    "urljoin({:?}, {:?})",
                    case.base, case.relative
                );
                ok += 1;
            }
            Outcome::Exception { exception } => {
                let error = urljoin(&case.base, &case.relative).expect_err("join should raise");
                assert_eq!(format!("ValueError: {}", error.message()), *exception);
            }
        }
    }
    assert!(ok > 150);
}

#[test]
fn urlencode_orders_like_python() {
    // `urlencode` is exercised through the redirect corpus; this pins the
    // empty and multi-pair shapes directly.
    assert_eq!(urlencode(&[]), "");
    assert_eq!(
        urlencode(&[
            ("a".to_string(), "1".to_string()),
            ("b".to_string(), "2".to_string()),
        ]),
        "a=1&b=2"
    );
}
