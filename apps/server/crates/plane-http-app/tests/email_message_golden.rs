// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Byte-for-byte check of the SMTP message builder against Django's output.
//!
//! `tests/golden/email_messages.json` is recorded by
//! `apps/server/tools/mail/email_message_corpus.py` from the real
//! `EmailMultiAlternatives.message().as_bytes(linesep="\r\n")`. `Date` and
//! `Message-ID` are the two generated headers; the corpus masks them to
//! `<DATE>`/`<MESSAGE-ID>` and this test passes the same strings, so every
//! other byte (header order, encoded words, folding, MIME shell, transfer
//! encoding and body) is compared exactly.
//!
//! Regenerate the corpus after a Django or drf/email-behaviour change:
//!
//! ```sh
//! cd apps/api && .venv/bin/python ../server/tools/mail/email_message_corpus.py
//! ```

use std::fs;

use plane_http_app::instances::mail::{MailError, Message, quoteaddr, sanitize_address};

#[derive(serde::Deserialize)]
struct Corpus {
    messages: Vec<Case>,
    envelopes: Vec<EnvelopeCase>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    subject: String,
    body: String,
    from_email: String,
    to: Vec<String>,
    cc: Vec<String>,
    reply_to: Vec<String>,
    /// `None` for the two cases Django rejects with `BadHeaderError`.
    bytes: Option<String>,
    error: Option<String>,
}

#[derive(serde::Deserialize)]
struct EnvelopeCase {
    input: String,
    sanitized: Option<String>,
    quoteaddr: Option<String>,
    error: Option<String>,
}

fn corpus() -> Corpus {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/email_messages.json"
    );
    let text = fs::read_to_string(path).expect("read tests/golden/email_messages.json");
    serde_json::from_str(&text).expect("parse tests/golden/email_messages.json")
}

#[test]
fn message_bytes_match_django() {
    let corpus = corpus();
    let mut compared = 0;
    for case in &corpus.messages {
        let message = Message {
            subject: case.subject.clone(),
            body: case.body.clone(),
            from_email: case.from_email.clone(),
            to: case.to.clone(),
            cc: case.cc.clone(),
            reply_to: case.reply_to.clone(),
        };
        match &case.bytes {
            Some(expected) => {
                let actual = message
                    .as_bytes("<DATE>", "<MESSAGE-ID>")
                    .unwrap_or_else(|error| panic!("{}: {error}", case.name));
                assert_eq!(
                    String::from_utf8(actual).expect("utf-8"),
                    *expected,
                    "case {}",
                    case.name
                );
                compared += 1;
            }
            None => {
                assert_eq!(
                    case.error.as_deref(),
                    Some("bad_header"),
                    "case {} has neither bytes nor a BadHeaderError",
                    case.name
                );
                let error = message
                    .as_bytes("<DATE>", "<MESSAGE-ID>")
                    .expect_err(&format!("{}: expected BadHeaderError", case.name));
                assert!(
                    matches!(error, MailError::BadHeader(_)),
                    "case {}: {error:?}",
                    case.name
                );
            }
        }
    }
    assert!(compared >= 16, "only {compared} messages compared");
}

#[test]
fn envelopes_match_django() {
    let corpus = corpus();
    let mut compared = 0;
    for case in &corpus.envelopes {
        match (&case.sanitized, &case.quoteaddr) {
            (Some(sanitized), Some(quoted)) => {
                let actual = sanitize_address(&case.input)
                    .unwrap_or_else(|error| panic!("{}: {error}", case.input));
                assert_eq!(actual, *sanitized, "sanitize_address({:?})", case.input);
                assert_eq!(
                    quoteaddr(&actual).as_deref(),
                    Some(quoted.as_str()),
                    "quoteaddr({:?})",
                    case.input
                );
                compared += 1;
            }
            _ => {
                assert_eq!(case.error.as_deref(), Some("invalid"), "{}", case.input);
                assert!(
                    sanitize_address(&case.input).is_err(),
                    "expected sanitize_address({:?}) to fail",
                    case.input
                );
            }
        }
    }
    assert!(compared >= 5, "only {compared} envelopes compared");
}
