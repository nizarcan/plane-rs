// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! `tests/golden/reset_tokens.json`, recorded from Django 5.2.17's
//! `PasswordResetTokenGenerator` and `django.utils.http`'s
//! `urlsafe_base64_{encode,decode}` by `tools/auth/record_corpora.py`.
//!
//! `_now` is pinned with a fixed `datetime` in the check cases, so the corpus
//! is independent of the wall clock.

use chrono::{DateTime, Utc};
use plane_auth::auth_flow::{
    PASSWORD_RESET_TIMEOUT, TokenUser, check_token, make_token_with_timestamp, num_seconds,
    urlsafe_base64_decode, urlsafe_base64_encode,
};
use serde::Deserialize;

const CORPUS: &str = include_str!("golden/reset_tokens.json");

#[derive(Deserialize)]
struct Corpus {
    timeout: i64,
    tokens: Vec<TokenCase>,
    checks: Vec<CheckCase>,
    base64: Vec<Base64Case>,
}

#[derive(Deserialize)]
struct TokenCase {
    secret: String,
    pk: String,
    password: String,
    email: String,
    last_login: Option<String>,
    timestamp: i64,
    token: String,
}

#[derive(Deserialize)]
struct CheckCase {
    secret: String,
    pk: String,
    password: String,
    email: String,
    last_login: Option<String>,
    now: String,
    token: String,
    valid: bool,
}

#[derive(Deserialize)]
struct Base64Case {
    input: Option<String>,
    encoded: String,
    decoded: Outcome,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Outcome {
    Value { value: String },
    Exception { exception: String },
}

fn parse(value: &str) -> DateTime<Utc> {
    if let Ok(value) = DateTime::parse_from_rfc3339(value) {
        return value.with_timezone(&Utc);
    }
    // `datetime.now()` is naive; the recorder writes `isoformat()` without an
    // offset, and the process timezone is UTC.
    chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S")
        .expect("the corpus writes an ISO datetime")
        .and_utc()
}

fn user<'a>(case: &'a TokenCase) -> TokenUser<'a> {
    TokenUser {
        pk: &case.pk,
        password: &case.password,
        email: Some(&case.email),
        last_login: case.last_login.as_deref().map(parse),
    }
}

#[test]
fn the_timeout_is_three_days() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    assert_eq!(corpus.timeout, PASSWORD_RESET_TIMEOUT);
    assert_eq!(PASSWORD_RESET_TIMEOUT, 259_200);
}

#[test]
fn every_recorded_token_matches() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    assert!(corpus.tokens.len() >= 2_000, "the corpus shrank");
    for case in &corpus.tokens {
        assert_eq!(
            make_token_with_timestamp(&user(case), case.timestamp, &case.secret),
            case.token,
            "token for pk={} ts={} secret_len={}",
            case.pk,
            case.timestamp,
            case.secret.len()
        );
    }
}

#[test]
fn every_check_decision_matches() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    assert!(corpus.checks.len() >= 100);
    let mut valid = 0;
    for case in &corpus.checks {
        let user = TokenUser {
            pk: &case.pk,
            password: &case.password,
            email: Some(&case.email),
            last_login: case.last_login.as_deref().map(parse),
        };
        let now = parse(&case.now);
        assert_eq!(
            check_token(&user, &case.token, now, &case.secret),
            case.valid,
            "check_token({:?}) at {}",
            case.token,
            case.now
        );
        if case.valid {
            valid += 1;
        }
        // Half the variants are valid, half are not.
        assert_eq!(
            num_seconds(now),
            (now - DateTime::from_timestamp(978_307_200, 0).expect("epoch")).num_seconds()
        );
    }
    assert!(valid > 10 && valid < corpus.checks.len() - 10);
}

#[test]
fn urlsafe_base64_round_trips_and_rejects_like_python() {
    let corpus: Corpus = serde_json::from_str(CORPUS).expect("corpus json");
    assert!(corpus.base64.len() >= 16);
    for case in &corpus.base64 {
        if let Some(input) = &case.input {
            assert_eq!(
                urlsafe_base64_encode(input.as_bytes()),
                case.encoded,
                "encode({input:?})"
            );
        }
        match &case.decoded {
            Outcome::Value { value } => {
                let decoded = urlsafe_base64_decode(&case.encoded).expect("decode");
                assert_eq!(
                    String::from_utf8(decoded).expect("utf-8"),
                    *value,
                    "decode({:?})",
                    case.encoded
                );
            }
            Outcome::Exception { exception } if exception.starts_with("ValueError:") => {
                let error = urlsafe_base64_decode(&case.encoded).expect_err("decode should fail");
                // Django wraps `binascii.Error`/`LookupError` as `ValueError`;
                // only the class is observable to the reset view.
                assert!(!error.to_string().is_empty());
            }
            Outcome::Exception { exception } if exception.starts_with("UnicodeDecodeError:") => {
                // `smart_str(urlsafe_base64_decode(...))` in the view: the
                // bytes decode, the UTF-8 conversion fails.
                let decoded = urlsafe_base64_decode(&case.encoded).expect("base64 decodes");
                assert!(
                    String::from_utf8(decoded).is_err(),
                    "expected invalid UTF-8 for {:?}",
                    case.encoded
                );
            }
            Outcome::Exception { exception } => panic!("unexpected oracle {exception}"),
        }
    }
}
