// Copyright (c) 2023-present Plane Software, Inc. and contributors
// SPDX-License-Identifier: AGPL-3.0-only
// See the LICENSE file for details.

//! §2d of `plans/WORK_ITEMS.md`: the description validators the work-item (and
//! draft, and comment) serializers run.
//!
//! | Python | Here |
//! | --- | --- |
//! | `validate_html_content` (`plane/utils/content_validator.py:209-239`) | [`validate_html_content`] |
//! | `validate_binary_data` (`plane/utils/content_validator.py:29-68`) | [`validate_binary_data`] |
//! | `strip_tags` (`plane/utils/html_processor.py:28-31`) | [`strip_tags`] (re-export of `plane_db::hooks::html`) |
//!
//! ## The error bodies
//!
//! `IssueCreateSerializer.validate` raises
//! `serializers.ValidationError({"error": "html content is not valid"})` and
//! `ValidationError({"description_binary": "Invalid binary data"})`
//! (`plane/app/serializers/issue.py:145-157`). DRF's `as_serializer_error`
//! wraps every dict value in a one-element list, so the bodies are
//! `{"error": ["html content is not valid"]}` and
//! `{"description_binary": ["Invalid binary data"]}` — [`html_invalid_error`] /
//! [`binary_invalid_error`].
//!
//! ## What is reachable
//!
//! The HTML branch always is. The **binary** branch is unreachable through the
//! API: `Issue.description_binary` is a `BinaryField` with `editable=False`
//! (`plane/db/models/issue.py:140`), so DRF builds it as a read-only
//! `ModelField` and a client value never reaches `attrs` (verified against the
//! real serializer: sending `description_binary` yields no error and no
//! validated key). It is ported because the draft serializer has the identical
//! branch (`plane/app/serializers/draft.py:92-95`) and a generated field table
//! could make it reachable; do not call it on the issue path expecting Django
//! to return 400.
//!
//! ## HTML
//!
//! `assert_sanitized` is the serializer's step, complete: truthiness, the 10 MB
//! limit measured on the UTF-8 bytes (`len(html.encode("utf-8"))`), then
//! `nh3.clean` with Plane's tag/attribute/protocol tables (see [`super::html`]
//! and its corpus) — the sanitized string *replaces* `description_html`.

use plane_drf::view::ViewResponse;

pub use plane_db::hooks::html::strip_tags;

/// `MAX_SIZE` (`plane/utils/content_validator.py:15`).
pub const MAX_SIZE: usize = 10 * 1024 * 1024;

/// `SUSPICIOUS_BINARY_PATTERNS` (`content_validator.py:17-24`), in order.
const SUSPICIOUS_BINARY_PATTERNS: [&str; 6] = [
    "<html",
    "<!doctype",
    "<script",
    "javascript:",
    "data:",
    "<iframe",
];

/// `validate_html_content(html)`'s three outcomes
/// (`content_validator.py:211-243`).
///
/// The Python returns `(is_valid, error_message, clean_html)`: a falsy input is
/// `(True, None, None)` (the serializer's `if attrs["description_html"]` guard
/// makes that unreachable), an over-limit input is `(False, "HTML content
/// exceeds…", None)` and the serializer reports the generic message, and a
/// normal input is `(True, None, clean)` — the sanitized string *replaces* the
/// field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HtmlValidation {
    /// Falsy input: nothing to do, keep the value as it is.
    Unchanged,
    /// Sanitize-and-replace.
    Sanitized(String),
    /// Over the 10 MB limit (or `nh3` raised, which cannot happen here):
    /// `{"error": ["html content is not valid"]}`.
    Invalid,
}

/// `validate_html_content` (`plane/utils/content_validator.py:209-243`).
#[must_use]
pub fn validate_html_content(html: &str) -> HtmlValidation {
    if html.is_empty() {
        return HtmlValidation::Unchanged;
    }
    // `len(html_content.encode("utf-8")) > MAX_SIZE`.
    if html.len() > MAX_SIZE {
        return HtmlValidation::Invalid;
    }
    HtmlValidation::Sanitized(super::html::sanitize(html))
}

/// `{"error": ["html content is not valid"]}` with DRF's 400.
#[must_use]
pub fn html_invalid_error() -> ViewResponse {
    super::field_string_error("error", "html content is not valid")
}

/// `validate_binary_data(data)` for the string form the serializer sees.
///
/// `true` means valid. The Python accepts `bytes` too, but a DRF
/// `ModelField` hands over the raw body value, i.e. a `str`.
#[must_use]
pub fn validate_binary_data(data: &str) -> bool {
    // `if not data: return True, None`.
    if data.is_empty() {
        return true;
    }
    let Some(decoded) = base64_decode(data) else {
        return false;
    };
    // `if len(binary_data) > MAX_SIZE` / `if len(binary_data) < 4`.
    if decoded.len() > MAX_SIZE || decoded.len() < 4 {
        return false;
    }
    // `decoded_text = binary_data.decode("utf-8", errors="ignore")[:200]`;
    // patterns are ASCII, so an ASCII lowercase is the same test.
    let text = utf8_ignore_prefix(&decoded, 200).to_ascii_lowercase();
    !SUSPICIOUS_BINARY_PATTERNS
        .iter()
        .any(|pattern| text.contains(pattern))
}

/// `{"description_binary": ["Invalid binary data"]}` with DRF's 400.
#[must_use]
pub fn binary_invalid_error() -> ViewResponse {
    super::field_string_error("description_binary", "Invalid binary data")
}

/// `base64.b64decode(text)` with its default `validate=False` leniency.
///
/// CPython's `binascii.a2b_base64` (non-strict) discards every byte outside the
/// base64 alphabet, then requires the remaining length to be a multiple of
/// four with at most two trailing `=` (`binascii.Error: Invalid base64-encoded
/// string …`). An empty result is `Some(vec![])` — the caller's length checks
/// reject it, exactly where Python does.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    let mut alphabet = Vec::with_capacity(bytes.len());
    let mut padding = 0usize;
    for &byte in bytes {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' => {
                if padding > 0 {
                    // Data after padding is invalid (`a2b_base64` stops there
                    // and reports "Excess data after padding").
                    return None;
                }
                alphabet.push(base64_value(byte));
            }
            b'=' => padding += 1,
            _ => {}
        }
    }
    if padding > 2 {
        return None;
    }
    if !(alphabet.len() + padding).is_multiple_of(4) {
        return None;
    }
    if padding > 0 && alphabet.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(alphabet.len() * 3 / 4);
    for chunk in alphabet.chunks(4) {
        match chunk.len() {
            4 => {
                let value = (chunk[0] << 18) | (chunk[1] << 12) | (chunk[2] << 6) | chunk[3];
                out.push((value >> 16) as u8);
                out.push((value >> 8) as u8);
                out.push(value as u8);
            }
            3 => {
                let value = (chunk[0] << 18) | (chunk[1] << 12) | (chunk[2] << 6);
                out.push((value >> 16) as u8);
                out.push((value >> 8) as u8);
            }
            2 => {
                let value = (chunk[0] << 18) | (chunk[1] << 12);
                out.push((value >> 16) as u8);
            }
            _ => return None,
        }
    }
    Some(out)
}

fn base64_value(byte: u8) -> u32 {
    u32::from(match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        _ => 63,
    })
}

/// `bytes.decode("utf-8", errors="ignore")[:limit]` without decoding the rest.
///
/// Invalid sequences are skipped one byte at a time, which is what CPython's
/// UTF-8 decoder does for `errors="ignore"`.
fn utf8_ignore_prefix(bytes: &[u8], limit: usize) -> String {
    let mut out = String::with_capacity(limit * 4);
    let mut index = 0;
    let mut chars = 0;
    while index < bytes.len() && chars < limit {
        match std::str::from_utf8(&bytes[index..]) {
            Ok(text) => {
                for character in text.chars() {
                    if chars == limit {
                        break;
                    }
                    out.push(character);
                    chars += 1;
                }
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if let Ok(text) = std::str::from_utf8(&bytes[index..index + valid]) {
                    for character in text.chars() {
                        if chars == limit {
                            return out;
                        }
                        out.push(character);
                        chars += 1;
                    }
                }
                index += valid + 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from `plane.utils.content_validator` and
    /// `plane.utils.html_processor` (CPython 3.12.13, nh3 0.2.18) by
    /// `tools/descriptions/gen_description_corpus.py`.
    const GOLDEN: &str = include_str!("../../tests/golden/description-validators.json");

    #[test]
    fn validate_html_content_matches_nh3() {
        let parsed: serde_json::Value = serde_json::from_str(GOLDEN).expect("golden json");
        let cases = parsed["validate_html_content"].as_array().expect("cases");
        assert!(cases.len() > 30, "corpus shrank: {}", cases.len());
        for case in cases {
            let input = case["input"].as_str().expect("input");
            let expected = match case["clean"].as_str() {
                Some(clean) => HtmlValidation::Sanitized(clean.to_owned()),
                None => HtmlValidation::Unchanged,
            };
            assert_eq!(validate_html_content(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn validate_binary_data_matches_python() {
        let parsed: serde_json::Value = serde_json::from_str(GOLDEN).expect("golden json");
        let cases = parsed["validate_binary_data"].as_array().expect("cases");
        assert!(cases.len() > 15, "corpus shrank: {}", cases.len());
        for case in cases {
            let input = case["input"].as_str().expect("input");
            let expected = case["is_valid"].as_bool().expect("is_valid");
            assert_eq!(validate_binary_data(input), expected, "input {input:?}");
        }
    }

    #[test]
    fn the_error_bodies_are_drf_lists() {
        for (response, expected) in [
            (
                html_invalid_error(),
                r#"{"error":["html content is not valid"]}"#,
            ),
            (
                binary_invalid_error(),
                r#"{"description_binary":["Invalid binary data"]}"#,
            ),
        ] {
            match response {
                ViewResponse::Raw { status, body, .. } => {
                    assert_eq!(status, 400);
                    assert_eq!(String::from_utf8(body).expect("utf8"), expected);
                }
                _ => panic!("expected a rendered body"),
            }
        }
    }

    #[test]
    fn the_html_limit_is_the_utf8_byte_length() {
        // 10 MB of ASCII is over the limit only by one byte; a 4-byte character
        // counts as four.
        let at_limit = "a".repeat(MAX_SIZE);
        assert!(matches!(
            validate_html_content(&at_limit),
            HtmlValidation::Sanitized(_)
        ));
        let over = "a".repeat(MAX_SIZE + 1);
        assert_eq!(validate_html_content(&over), HtmlValidation::Invalid);
        let unicode = "\u{1F600}".repeat(MAX_SIZE / 4 + 1);
        assert_eq!(validate_html_content(&unicode), HtmlValidation::Invalid);
    }
}
