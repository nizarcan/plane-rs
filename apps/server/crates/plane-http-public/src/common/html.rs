// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! The `lxml` parse gate `/api/v1/` puts in front of `description_html`.
//!
//! `plane/api/serializers/issue.py:102-109`:
//!
//! ```python
//! try:
//!     if data.get("description_html", None) is not None:
//!         parsed = html.fromstring(data["description_html"])
//!         parsed_str = html.tostring(parsed, encoding="unicode")
//!         data["description_html"] = parsed_str
//! except Exception:
//!     raise serializers.ValidationError("Invalid HTML passed")
//! ```
//!
//! Two things are observable, and only the first is implemented here:
//!
//! 1. **The gate.** `html.fromstring` raises `lxml.etree.ParserError("Document
//!    is empty")` when the input parses to no element at all — an empty or
//!    whitespace-only string, a lone comment, a lone processing instruction, or
//!    a lone end tag. That is a 400 `{"non_field_errors": ["Invalid HTML
//!    passed"]}` and nothing is written. [`check`] reproduces it, and the
//!    corpus `tests/golden/lxml_html.jsonl` records libxml2's verdict for 40
//!    inputs (`tools/inventory/lxml_html_corpus.py` regenerates it).
//! 2. **The normalisation.** Everything else is parsed in libxml2's recover
//!    mode and re-serialised by `html.tostring`: tag and attribute names
//!    lowercased, attribute values quoted with `'` when they contain `"`,
//!    `>` written as `&gt;` in text, entities decoded and re-encoded, comments
//!    kept inside a wrapper, several top-level nodes wrapped in a `<div>`,
//!    bare text wrapped in a `<span>`, `<script>`/`<style>` moved to `<head>`
//!    in a document, implied end tags inserted (`<li>`, `<p>`, `<td>`, …),
//!    `<br/>` written `<br>`, and so on. That is libxml2's HTML4 parser, not
//!    something `ammonia` does (`nh3` parses HTML5 and sanitises while it
//!    serialises), and it is **not** ported here: `normalize` returns
//!    [`HtmlNormalizeError::Unsupported`] instead of silently storing a
//!    different string. Set D owns it; the corpus records the expected output
//!    for every case so the port is a table check, not a guess.
//!
//! A corpus case whose input the gate rejects never reaches the normaliser, so
//! the gate alone keeps set D's write paths correct for every input Plane's
//! editor does not produce. For an input libxml2 accepts, the stored value is
//! `nh3.clean(lxml_normalized)`; when the normaliser is missing the safest
//! behaviour is a 500, not a silently different row.

use plane_core::error::ApiError;
use plane_drf::error::ViewError;

/// The `ValidationError` detail `plane/api/serializers/issue.py:109` raises.
pub const INVALID_HTML: &str = "Invalid HTML passed";

/// Why the gate refused a `description_html` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidHtml;

impl std::fmt::Display for InvalidHtml {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(INVALID_HTML)
    }
}

impl std::error::Error for InvalidHtml {}

/// The 400 a rejected `description_html` produces.
///
/// DRF runs `validate()` before `create()`, and a `ValidationError` raised
/// there without a field name lands under `non_field_errors`
/// (`rest_framework/serializers.py:504-514`):
/// `{"non_field_errors": ["Invalid HTML passed"]}`.
#[must_use]
pub fn invalid_html_error() -> ViewError {
    let mut body = serde_json::Map::new();
    body.insert(
        "non_field_errors".to_string(),
        serde_json::Value::Array(vec![serde_json::Value::String(INVALID_HTML.to_string())]),
    );
    ViewError::Api(ApiError::ValidationError(serde_json::Value::Object(body)))
}

/// Why the normaliser did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlNormalizeError {
    /// `html.fromstring` raised: the input has no element.
    Empty,
    /// libxml2 accepted the input and set D has not ported its serialiser yet.
    Unsupported,
}

impl std::fmt::Display for HtmlNormalizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => f.write_str(INVALID_HTML),
            Self::Unsupported => f.write_str(
                "lxml HTML normalisation is not ported yet \
                 (plane/api/serializers/issue.py:102-109); see plane-http-public::common::html",
            ),
        }
    }
}

impl std::error::Error for HtmlNormalizeError {}

/// `html.fromstring(value)`'s accept/reject boundary.
///
/// `Ok(())` when libxml2 would return an element, [`InvalidHtml`] when it
/// raises `ParserError("Document is empty")`.
///
/// The rule is "does the document contain an element or text at all": a start
/// tag (even an unterminated one — `"<p"` parses to an empty `<span>`) or any
/// non-whitespace text is enough; whitespace, comments, processing
/// instructions and stray end tags are not. `NUL` bytes are text to libxml2
/// (it replaces them with U+FFFD), so `"\0"` is accepted.
///
/// # Errors
///
/// [`InvalidHtml`] when the input has no element.
pub fn check(value: &str) -> Result<(), InvalidHtml> {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'<' {
            // Text. libxml2 keeps any non-whitespace byte, including NUL.
            let rest = &value[index..];
            let next_tag = rest.find('<').unwrap_or(rest.len());
            // libxml2's blank set is ASCII: space, `\t`, `\n`, `\r`, `\x0c`.
            // U+00A0 and U+2003 are text, so they make the document non-empty
            // (`"\u{a0}"` parses to `<span>\u{a0}</span>`).
            if !rest[..next_tag]
                .bytes()
                .all(|byte| byte.is_ascii_whitespace())
            {
                return Ok(());
            }
            index += next_tag;
            continue;
        }
        // A markup construct.
        let rest = &value[index..];
        if rest.starts_with("<!--") {
            match rest.find("-->") {
                Some(end) => {
                    index += end + 3;
                    continue;
                }
                // An unterminated comment swallows the rest.
                None => return Err(InvalidHtml),
            }
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            match rest.find('>') {
                Some(end) => {
                    index += end + 1;
                    continue;
                }
                None => return Err(InvalidHtml),
            }
        }
        if rest.starts_with("</") {
            // An end tag on its own creates no element.
            match rest.find('>') {
                Some(end) => {
                    index += end + 1;
                    continue;
                }
                None => return Err(InvalidHtml),
            }
        }
        // `<` followed by a letter starts a tag; libxml2 keeps it even when the
        // `>` is missing (`"<p"` is `<span></span>`).
        if rest[1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            return Ok(());
        }
        // `<` followed by anything else (`"<"`, `"<>"`, `"< p>"`) is text,
        // which is non-whitespace.
        return Ok(());
    }
    Err(InvalidHtml)
}

/// `html.tostring(html.fromstring(value), encoding="unicode")`.
///
/// # Errors
///
/// [`HtmlNormalizeError::Empty`] when libxml2 refuses the input, and
/// [`HtmlNormalizeError::Unsupported`] for every accepted input until the
/// serialiser is ported (see the module docs).
pub fn normalize(_value: &str) -> Result<String, HtmlNormalizeError> {
    check(_value).map_err(|_| HtmlNormalizeError::Empty)?;
    Err(HtmlNormalizeError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every verdict below was recorded from the real `lxml` in
    /// `apps/api/.venv` (`lxml.etree.ParserError: Document is empty`); see
    /// `tests/golden/lxml_html.jsonl`.
    #[test]
    fn the_gate_matches_lxml() {
        // The cases libxml2 refuses: no element comes out of the parser.
        for rejected in [
            "",
            "\n\t ",
            "  ",
            "<!-- only comment -->",
            "<?xml version='1.0'?>",
            "</p>",
        ] {
            assert_eq!(check(rejected), Err(InvalidHtml), "{rejected:?}");
        }
        // The cases libxml2 accepts, including the surprising ones.
        for accepted in [
            "hello",
            "\0",
            "\0a",
            "<",
            "<>",
            "< p>",
            "<p>",
            "<p",
            "<p ",
            "&bogus;",
            "<p> </p>",
            "+",
            "<!-- c --><p>x</p>",
            "<p>x</p><!-- c -->",
            "<p>x</p></p>",
            "text<p>x</p>",
        ] {
            assert_eq!(check(accepted), Ok(()), "{accepted:?}");
        }
    }

    #[test]
    fn the_rejection_is_drfs_non_field_error() {
        let error = invalid_html_error();
        let ViewError::Api(api) = error else {
            panic!("expected an API error");
        };
        assert_eq!(api.status(), 400);
        assert_eq!(
            serde_json::to_string(&match api.render() {
                Ok(response) => String::from_utf8(response.body).expect("utf8"),
                Err(error) => panic!("render: {error}"),
            })
            .expect("json"),
            r#""{\"non_field_errors\":[\"Invalid HTML passed\"]}""#
        );
    }

    /// The normaliser is deliberately not ported yet; it must not pretend.
    #[test]
    fn normalization_is_not_silently_identity() {
        assert_eq!(normalize(""), Err(HtmlNormalizeError::Empty));
        assert_eq!(normalize("<p>x</p>"), Err(HtmlNormalizeError::Unsupported));
    }
}
