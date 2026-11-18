// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! Byte-for-byte check of `plane-mail`'s template renderer, plain-text
//! generator and `EmailMultiAlternatives` multipart writer against Django's
//! output.
//!
//! `tests/golden/email_templates.json` is recorded by
//! `apps/server/tools/mail/email_template_corpus.py` from the real Django
//! template engine and `plane/utils/email.py`. The boundary, `Date` and
//! `Message-ID` are the only generated values; the corpus masks them and this
//! test passes the same placeholders.

use std::fs;

use plane_mail::{Message, TemplateError, generate_plain_text_from_html, render};

#[derive(serde::Deserialize)]
struct Corpus {
    templates: Vec<TemplateCase>,
    plain_text: Vec<PlainTextCase>,
    multipart: String,
}

#[derive(serde::Deserialize)]
struct TemplateCase {
    name: String,
    template: String,
    context: serde_json::Value,
    html: String,
    text: String,
}

#[derive(serde::Deserialize)]
struct PlainTextCase {
    name: String,
    html: String,
    text: String,
}

fn corpus() -> Corpus {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/email_templates.json"
    );
    let text = fs::read_to_string(path).expect("read tests/golden/email_templates.json");
    serde_json::from_str(&text).expect("parse tests/golden/email_templates.json")
}

#[test]
fn templates_match_django() {
    let corpus = corpus();
    assert!(corpus.templates.len() >= 3);
    for case in &corpus.templates {
        let html = render(&case.template, &case.context)
            .unwrap_or_else(|error| panic!("{}: {error}", case.name));
        assert_eq!(html, case.html, "html for {}", case.name);
        let text = generate_plain_text_from_html(&html);
        assert_eq!(text, case.text, "plain text for {}", case.name);
    }
}

#[test]
fn plain_text_matches_django() {
    let corpus = corpus();
    assert!(corpus.plain_text.len() >= 5);
    for case in &corpus.plain_text {
        assert_eq!(
            generate_plain_text_from_html(&case.html),
            case.text,
            "case {}",
            case.name
        );
    }
}

#[test]
fn multipart_bytes_match_django() {
    let corpus = corpus();
    let message = Message::with_html(
        "Your unique Plane login code is A1B2C3",
        "\n\nHello from Plane\n\n",
        "<html><body><p>Hello from Plane</p></body></html>",
        "Team Plane <team@mailer.plane.so>",
        vec!["ada@example.com".to_string()],
    );
    let bytes = message
        .as_bytes_with_boundary(
            "<DATE>",
            "<MESSAGE-ID>",
            Some("<BOUNDARY-0000000000000000000000000>"),
        )
        .expect("bytes");
    assert_eq!(String::from_utf8(bytes).expect("utf-8"), corpus.multipart);
}

#[test]
fn unported_templates_fail_loudly() {
    let error =
        render("emails/not/ported.html", &serde_json::Value::Null).expect_err("unknown template");
    assert!(matches!(error, TemplateError::UnknownTemplate(_)));
}
